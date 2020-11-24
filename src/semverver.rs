//! Utilities related to running `semverver` analysis.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::path::Path;
use std::env;
use std::io;
use std::sync::{Arc, RwLock};
use std::error::Error;
use std::process::Command;

use cargo::core::Package;
use cargo::core::Dependency;
use cargo::core::Workspace;
use petgraph::Direction;
use semver::VersionReq;

#[derive(Clone, Copy, Debug)]
pub enum SemverBump {
    Major,
    Minor,
    Patch,
}

#[derive(Clone, Debug)]
pub enum Action {
    PackageVerBump { pkg: Package, bump: SemverBump },
    DependencyReqBump { pkg: Package, dep: Dependency, req: VersionReq }
}

#[cfg(not(feature = "semverver"))]
pub fn run_semver_analysis<'a>(
    ws: &Workspace,
    _pkgs: impl Iterator<Item = &'a Package>,
) -> Result<Vec<Action>, Box<dyn Error>> {
    Err("Semver analysis is unsupported, recompile with \"semverver\" feature".into())
}

#[cfg(feature = "semverver")]
pub fn run_semver_analysis<'a>(
    ws: &Workspace,
    predicate: impl Fn(&Package) -> bool,
) -> Result<Vec<Action>, Box<dyn Error>> {
    // The algorithm below, given a local Cargo workspace its changed packages,
    // analyzes which packages may need a MAJOR, MINOR or PATCH semver version
    // increment across the workspace.
    //
    // As a quick reminder, we define these (http://semver.org) as:
    // * MAJOR - when you make incompatible API changes,
    // * MINOR - when you add functionality in backwards compatible manner,
    // * PATCH - when you make backwards compatible bug fixes.
    // MAJOR-level increment can contain MINOR or PATCH-level changes.
    // MINOR-level increment can contain PATCH-level changes.
    //
    // Packages can, simplifying, declare dependence on other packages
    // on either of these three compat. levels or by requiring an exact version.
    //
    // Thus, if we modify a package in a way that requires us to increment its
    // version, we need to consider what happens to its dependents.
    // Either:
    // 1. Dependency version was incremented in a way that's compatible with
    // the declared requirement of the dependent package, no need to do anything.
    //
    // 2. The increment happens in backwards *incompatible* way from the
    // dependent's package point of view. To update its dependency on said
    // package, we need to increase dependent package's version as well.

    // The dependent package (B) may need an appropriate increment if its
    // dependency (A) was incremented, in the following circumstances:
    // | A \ B | MAJOR                           | MINOR                            | PATCH                      |
    // |-------|---------------------------------|----------------------------------|----------------------------|
    // | MAJOR | B re-exports A's now-broken API | B re-exports A's newly-added API | B does not expose A's API  |
    // |       | (but continues to compile)      | (but continues to compile)       | (but continues to compile) |
    // |-------|---------------------------------|----------------------------------|----------------------------|
    // | MINOR | N/A                             | B re-exports A's newly-added API | B does not expose A's API  |
    // |-------|---------------------------------|----------------------------------|----------------------------|
    // | PATCH | N/A                             | N/A                              | B does not expose A's API  |
    //
    // If A incremented MAJOR/MINOR version, then we need to perform semantic
    // analysis on B to see if impacts its public API (we run `cargo semver`).
    //
    // NOTE: We treat breaking changes of 0.x APIs as semantically MAJOR, even
    // though it's enough to only bump a MINOR version.
    //
    // To do all of these above across the entire workspace, we need to:
    // 1. Narrow down dependency graph to transitive dependents of changed
    // packages, including themselves (others are not impacted)
    // 2. Topologically sort the graph (we need to process all dependencies
    // first in order to process a dependent only once)
    // 3. Mark all changed packages for semantic analysis
    // 4. Process packages:
    //   - if a package is marked for semantic analysis, run `cargo semver` and
    //     increment its version accordingly
    //   - otherwise, if marked as needing PATCH bump, increment it
    //   - For dependents that need to update their semver requirement, based
    //     on our newly-incremented version:
    //     * Bump their semver requirement
    //     * Mark as needing at least a PATCH bump
    //     * If we bumped MAJOR/MINOR, then mark it for semantic analysis
    //
    // NOTE: This doesn't take into account Cargo features due to combinatorial
    // explosion of possible variants and needing to perform a full workspace
    // resolution, which can also optionally impact the dependency graph.
    // As such, this should be treated as a good-enough approximation.

    // 1. Narrow down dependency graph to transitive dependents of changed
    // packages, including themselves (others are not impacted)

    let pkgs = crate::util::members_deep(ws);

    // FIXME: Temporarily assume that packages for which predicate is true are
    // the "root" changed ones
    let changed = pkgs.clone().into_iter().filter(predicate).collect();
    // NOTE: We use always true predicate to select the entire graph - even if
    // we changed a couple of packages, we need to analyze the entire transitive
    // graph for semver-compat (even if we won't publish the dependents)
    // FIXME: Provide a better way to process only non-dev deps
    let (mut graph, map) = crate::util::changed_dependents(pkgs, &changed, false, |_| true);

    log::debug!("Changed packages: {}", changed.len());
    log::debug!("Changed transitive dependents: {}", graph.node_count());

    // 2. Topologically sort the graph (we need to process all dependencies
    // first in order to process a dependent only once)
    let topo = petgraph::algo::toposort(&graph, None).map_err(|cycle| {
        log::warn!("Cycle encountered, did you disable dev-dependencies?");
        // Recreate the cycle ourselves for a better error message
        recreate_cycle(&graph, cycle.node_id())
    })?;

    // 3. Mark all changed packages for semantic analysis
    #[derive(Clone, Copy, Debug)]
    enum Requires {
        Nothing,
        PatchBump,
        SemanticAnalysis
    }
    let mut requires = vec![Requires::Nothing; graph.raw_nodes().len()];
    for idx in changed.iter().filter_map(|c| map.get(&c.name())) {
        requires[idx.index()] = Requires::SemanticAnalysis;
    }

    // 4. Process packages:
    let mut analysis = Vec::<Action>::new();
    for idx in topo {
        let pkg = graph[idx].clone();
        log::trace!("Processing package {} (idx {})", pkg.name(), idx.index());

        // If a package is marked for semantic analysis, run `cargo semver` and
        // increment its version accordingly.
        // Otherwise, if marked as needing PATCH bump, increment it.
        let bump = match requires[idx.index()] {
            Requires::Nothing => continue,
            Requires::PatchBump => SemverBump::Patch,
            // FIXME: Cargo semver does not work correctly in a workspace setting
            Requires::SemanticAnalysis => match cargo_semver(pkg.manifest_path()) {
                // Until a crate doesn't define 1.0-level public API it's fine
                // to only bump MINOR version
                Ok(SemverBump::Major) if pkg.version().major == 0 => SemverBump::Minor,
                Ok(bump) => bump,
                Err(err) => {
                    log::warn!("Error running cargo semver for `{}`: {}", pkg.name(), err);
                    continue;
                }
            },
        };
        let mut new_version = pkg.version().clone();
        match bump {
            SemverBump::Major => new_version.increment_major(),
            SemverBump::Minor => new_version.increment_minor(),
            SemverBump::Patch => new_version.increment_patch(),
        }

        analysis.push(Action::PackageVerBump { pkg: pkg.clone(), bump });

        // For dependents...
        let dependents: Vec<_> = graph.neighbors_directed(idx, Direction::Incoming).collect();
        for rev_idx in dependents {
            let rev_dep = &mut graph[rev_idx];
            // that need to update their semver requirement (i.e. new version
            // doesn't match anymore)...
            if rev_dep.dependencies().iter().filter(|d| d.package_name() == pkg.name())
                .all(|dep| dep.version_req().matches(&new_version)) {
                log::trace!("Skipping dependent `{}` as it seems its deps are compatible", rev_dep.name());
                continue;
            }
            log::trace!("Continuing with dependent `{}`", rev_dep.name());
            // Mark as needing at least a PATCH bump.
            // If we bumped MAJOR/MINOR, then mark it for semantic analysis.
            requires[rev_idx.index()] = match bump {
                SemverBump::Major | SemverBump::Minor => Requires::SemanticAnalysis,
                SemverBump::Patch => Requires::PatchBump,
            };
            log::trace!("Marking package `{}` (idx {}) as {:?}",
                rev_dep.name(),
                rev_idx.index(),
                requires[rev_idx.index()]
            );
            // Bump their semver requirement accordingly
            let rev_dep_name = rev_dep.name().clone();
            let summary = rev_dep.manifest_mut().summary_mut();
            *summary = summary.clone()
                .map_dependencies(|mut dep| {
                    let us = dep.package_name() == pkg.name();
                    if us && !dep.version_req().matches(&new_version) {
                        // Attempt to create least permissive new requirement
                        let new_req = VersionReq::parse(&new_version.to_string())
                            .expect("bare version requirement to be valid");
                        assert!(new_req.matches(&new_version));

                        dep.set_version_req(new_req.clone());
                        log::trace!("Setting new req. `{}` for dep `{}`", new_req, rev_dep_name);

                        analysis.push(Action::DependencyReqBump {
                            pkg: pkg.clone(),
                            dep: dep.clone(),
                            req: new_req
                        });

                        dep
                    } else {
                        dep
                    }
                });
        }
    }

    Ok(analysis)
}

/// Runs `cargo semver` for a package defined in the manifest path.
fn cargo_semver(manifest_path: impl AsRef<Path>) -> Result<SemverBump, Box<dyn Error>> {
    let mut manifest_path = manifest_path.as_ref().to_owned();
    manifest_path.pop();

    let mut cmd = Command::new("cargo");
    cmd.arg("semver");
    log::debug!("Running cargo semver in {}", manifest_path.display());
    cmd.current_dir(manifest_path);

    let output = cmd.output()?;

    // TODO: Handle cargo semver signalling patch-level deps
    Ok(if output.status.success() {
        // FIXME: Make sure it's only PATCH-level
        SemverBump::Patch
    } else {
        let stderr = std::str::from_utf8(&output.stderr)?;
        eprintln!("{}", &stderr);
        if stderr.contains("thread 'rustc' panicked at") {
            return Err(stderr.into());
        } else if let Some(idx) = stderr.find("could not compile `") {
            let newline = stderr[idx..].find('\n').unwrap_or(stderr.len());
            return Err(stderr[idx..][..newline].into());
        } else {
            SemverBump::Major
        }
    })
}


fn recreate_cycle(
    graph: &petgraph::Graph<Package, crate::util::DepKindFmt, petgraph::Directed>,
    cycle_root: petgraph::graph::NodeIndex,
) -> String {
    // FIXME: Replace with petgraph's DFS
    use petgraph::visit::EdgeRef;
    use petgraph::graph::NodeIndex;
    use cargo::core::dependency::DepKind;

    let mut path = Vec::new();
    let mut stack = Vec::new();
    let mut processing = HashSet::new();

    enum Action {
        Enter { node: NodeIndex },
        VisitEdge { target: NodeIndex, kind: DepKind },
    }

    stack.push(Action::Enter { node: cycle_root });
    for edge in graph.edges_directed(cycle_root, Direction::Outgoing) {
        stack.push(Action::VisitEdge { target: edge.target(), kind: edge.weight().0 });
    }

    while let Some(action) = stack.pop() {
        let (cur, kind) = match action {
            Action::Enter { node } => {
                path.pop();
                processing.remove(&node);
                continue;
            },
            Action::VisitEdge { target, kind } => (target, kind),
        };

        stack.push(Action::Enter { node: cur });
        path.push((cur, kind));

        if !processing.insert(cur) {
            let backtrace = path.iter().skip_while(|(dst, _)| *dst != cur).skip(1);
            eprintln!("{}...", graph[cur].name());
            for (dst, kind) in backtrace {
                eprintln!("... depends on {} ({:?})", graph[*dst].name(), kind);
            }

            return format!("Cycle detected: {}", graph[cur].name());
        } else {
            for edge in graph.edges_directed(cur, Direction::Outgoing) {
                stack.push(Action::VisitEdge {
                    target: edge.target(),
                    kind: edge.weight().0,
                });
            }
        }
    }
    unreachable!()
}

// FIXME: Use in-process execution with functions below

fn sysroot() -> String {
    option_env!("SYSROOT")
    .map(String::from)
    .or_else(|| env::var("SYSROOT").ok())
    .or_else(|| {
        let home = option_env!("RUSTUP_HOME").or(option_env!("MULTIRUST_HOME"));
        let toolchain = option_env!("RUSTUP_TOOLCHAIN").or(option_env!("MULTIRUST_TOOLCHAIN"));
        home.and_then(|home| toolchain.map(|toolchain| format!("{}/toolchains/{}", home, toolchain)))
    })
    .or_else(|| {
        Command::new("rustc")
            .arg("--print")
            .arg("sysroot")
            .output()
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|s| s.trim().to_owned())
    })
    .expect("need to specify SYSROOT or use rustup")
}

/// Obtain the paths to the produced rlib and the dependency output directory.
pub fn rlib_and_dep_output(
    workspace: &Workspace,
    name: &str,
    current: bool,
    // matches: &getopts::Matches,
    target: Option<&str>,
    features: Option<&str>,
    all_features: Option<bool>,
    no_default_features: Option<bool>,
) -> Result<(PathBuf, PathBuf), Box<dyn Error>> {
    let mut opts = cargo::ops::CompileOptions::new(
        workspace.config(),
        cargo::core::compiler::CompileMode::Build,
    )?;
    // we need the build plan to find our build artifacts
    opts.build_config.build_plan = true;

    let compile_kind = if let Some(target) = target {
        let target = cargo::core::compiler::CompileTarget::new(&target)?;

        let kind = cargo::core::compiler::CompileKind::Target(target);
        opts.build_config.requested_kinds = vec![kind];
        kind
    } else {
        cargo::core::compiler::CompileKind::Host
    };

    if let Some(s) = features {
        opts.features = s.split(' ').map(str::to_owned).collect();
    }

    opts.all_features = all_features.unwrap_or(false);
    opts.no_default_features = no_default_features.unwrap_or(false);

    env::set_var(
        "RUSTFLAGS",
        format!("-C metadata={}", if current { "new" } else { "old" }),
    );

    // Capture build plan from a separate Cargo invocation
    let output = VecWrite(Arc::new(RwLock::new(Vec::new())));

    let mut file_write = cargo::core::Shell::from_write(Box::new(output.clone()));
    file_write.set_verbosity(cargo::core::Verbosity::Quiet);

    let old_shell = std::mem::replace(&mut *workspace.config().shell(), file_write);

    cargo::ops::compile(workspace, &opts)?;

    let _ = std::mem::replace(&mut *workspace.config().shell(), old_shell);
    let plan_output = output.read()?;

    // actually compile things now
    opts.build_config.build_plan = false;

    let compilation = cargo::ops::compile(workspace, &opts)?;
    env::remove_var("RUSTFLAGS");

    let build_plan: BuildPlan = serde_json::from_slice(&plan_output)?;

    // TODO: handle multiple outputs gracefully
    for i in &build_plan.invocations {
        if let Some(kind) = i.target_kind.get(0) {
            if kind.contains("lib") && i.package_name == name {
                let deps_output = &compilation.deps_output[&compile_kind];

                return Ok((i.outputs[0].clone(), deps_output.clone()));
            }
        }
    }

    Err("lost build artifact".into())
}

#[derive(Debug, serde::Deserialize)]
struct Invocation {
    package_name: String,
    target_kind: Vec<String>,
    outputs: Vec<PathBuf>,
}

#[derive(Debug, serde::Deserialize)]
struct BuildPlan {
    invocations: Vec<Invocation>,
}

/// Thread-safe byte buffer that implements `io::Write`.
#[derive(Clone)]
struct VecWrite(Arc<RwLock<Vec<u8>>>);

impl VecWrite {
    pub fn read(&self) -> io::Result<std::sync::RwLockReadGuard<'_, Vec<u8>>> {
        self.0
            .read()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "lock poison"))
    }
    pub fn write(&self) -> io::Result<std::sync::RwLockWriteGuard<'_, Vec<u8>>> {
        self.0
            .write()
            .map_err(|_| io::Error::new(io::ErrorKind::Other, "lock poison"))
    }
}

impl io::Write for VecWrite {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let mut lock = Self::write(self)?;
        io::Write::write(&mut *lock, data)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

use crate::util::members_deep;
use cargo::{
    core::{package::Package, Dependency, Source, SourceId, Workspace},
    sources::registry::RegistrySource,
};
use log::{trace, warn};
use petgraph::dot::{self, Dot};
use petgraph::{Directed, Graph};
use std::{
    collections::{HashMap, HashSet},
    fs::OpenOptions,
    io::Write,
    path::PathBuf,
};

/// Generate the packages we should be releasing
pub fn packages_to_release<F>(
    ws: &Workspace<'_>,
    predicate: F,
    write_dot_graph: Option<PathBuf>,
) -> Result<Vec<Package>, anyhow::Error>
where
    F: Fn(&Package) -> bool,
{
    packages_to_release_inner(ws, predicate, members_deep, write_dot_graph)
}

fn packages_to_release_inner<'cfg, F, C>(
    ws: &Workspace<'cfg>,
    predicate: F,
    package_collector: C,
    write_dot_graph: Option<PathBuf>,
) -> Result<Vec<Package>, anyhow::Error>
where
    F: Fn(&Package) -> bool,
    C: Fn(&Workspace<'cfg>) -> Vec<Package>,
{
    // inspired by the work of `cargo-publish-all`: https://gitlab.com/torkleyy/cargo-publish-all
    ws.config()
        .shell()
        .status("Resolving", "Dependency Tree")
        .expect("Writing to Shell doesn't fail");

    let mut graph = Graph::<Package, (), Directed, u32>::new();
    let members = package_collector(ws);

    let (members, to_ignore): (Vec<_>, Vec<_>) = members.iter().partition(|m| predicate(m));

    let ignored = to_ignore
        .into_iter()
        .map(|m| m.name())
        .collect::<HashSet<_>>();

    ws.config()
        .shell()
        .status("Syncing", "Versions from crates.io")
        .expect("Writing to Shell doesn't fail");

    let mut already_published = HashSet::new();
    let mut registry = RegistrySource::remote(
        SourceId::crates_io(ws.config()).expect(
            "Your main registry (usually crates.io) can't be read. Please check your .cargo/config",
        ),
        &Default::default(),
        ws.config(),
    );
    let lock = ws.config().acquire_package_cache_lock();

    registry
        .update()
        .expect("Updating from remote registry failed :( .");

    for m in members.iter() {
        let dep = Dependency::parse(
            m.name(),
            Some(&m.version().to_string()),
            registry.source_id(),
        )
        .expect("Parsing our dependency doesn't fail");
        registry
            .query(&dep, &mut |_| {
                already_published.insert(m.name());
            })
            .expect("Quering the local registry doesn't fail");
    }

    // drop the global package lock
    drop(lock);

    let map = members
        .iter()
        .filter_map(|&member| {
            if ignored.contains(&member.name()) || already_published.contains(&member.name()) {
                return None;
            }
            Some((member.name(), graph.add_node(member.clone())))
        })
        .collect::<HashMap<_, _>>();

    for member in members {
        let current_index = match map.get(&member.name()) {
            Some(i) => i,
            _ => continue, // ignore entries we are not expected to publish
        };

        for dep in member.dependencies() {
            if let Some(dep_index) = map.get(&dep.package_name()) {
                graph.add_edge(*current_index, *dep_index, ());
            } else if already_published.contains(&dep.package_name()) {
                trace!("All good, it's on crates.io");
            } else {
                // we are looking at a dependency, we won't include in the set of
                // ones we are about to publish. Let's make sure, this won't block
                // us from doing so though.
                trace!("Checking dependency for problems: {}", dep.package_name());
                let source = dep.source_id();
                if source.is_default_registry() {
                    trace!("All good, it's on crates.io")
                } else if source.is_path() && dep.is_locked() {
                    // this is a pretty big indicator that something is going to fail later...
                    if ignored.contains(&dep.package_name()) {
                        warn!("{} lock depends on {}, which is expected to not be published. This might fail.", member.name(), dep.package_name())
                    }
                }
            }
        }
    }

    if let Some(dest) = write_dot_graph {
        let mut dest = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(dest)?;
        graphviz(&graph, &mut dest)?;
    }

    // cannot use `toposort` for graphs that are cyclic in a undirected sense
    // but are not in a directed way
    let mut cycles = vec![];
    let mut toposorted_indices = vec![];
    let strongly_connected_sets = petgraph::algo::kosaraju_scc(&graph);
    for strongly_connected in strongly_connected_sets {
        match strongly_connected.len() {
            0 => unreachable!("Strongly connected components are at least size 1. qed"),
            1 => toposorted_indices.push(strongly_connected[0]),
            _ => cycles.push(strongly_connected),
        }
    }
    if !cycles.is_empty() {
        assert!(petgraph::algo::is_cyclic_directed(&graph));
        let cycles = cycles
            .iter()
            .map(|x| {
                x.iter()
                    .map(|i| graph.node_weight(*i).unwrap())
                    .map(|pkg| pkg.name())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        anyhow::bail!("Contains cycles: {:?}", cycles);
    }

    // reverse in place, the output of `scc_karaju` is in reverse order
    toposorted_indices.reverse();

    let packages = toposorted_indices
        .into_iter()
        .map(|i| graph.node_weight(i).unwrap().clone())
        .rev()
        .collect::<Vec<_>>();

    Ok(packages)
}

/// Render a graphviz (aka dot graph) to a file.
fn graphviz(graph: &Graph<Package, ()>, dest: &mut impl Write) -> anyhow::Result<()> {
    let config = &[dot::Config::EdgeNoLabel, dot::Config::NodeNoLabel][..];
    let dot = Dot::with_attr_getters(
        graph,
        config,
        &|_graph, _edge_ref| -> String { "".to_owned() },
        &|_graph, (_idx, pkg)| -> String {
            format!(
                r#"label="{}:{}""#,
                pkg.name().to_string(),
                pkg.version().to_string().as_str()
            )
        },
    );
    dest.write_all(format!("{:?}", &dot).as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cargo::core::manifest::Manifest;
    use cargo::core::*;
    use cargo::util::toml::TomlManifest;
    use cargo::Config;

    use anyhow::Result;
    use itertools::Itertools;
    use semver::Version;
    use std::path::Path;

    /// Test helper to create a `struct Manifest`
    /// that is only living in memory, but could be written to disk.
    fn make_manifest(
        config: &Config,
        base: &std::path::Path,
        name: &'static str,
        version: Version,
        source_id: SourceId,
        dependencies: impl AsRef<[Dependency]>,
    ) -> Manifest {
        let toml_manifest = format!(
            r###"
[package]
name = "{name}"
version = "{version}"
edition = "2018"
description = "{name}"
publish = false

[dependencies]
"###,
            name = name,
            version = version
        );

        let toml_manifest =
            dependencies
                .as_ref()
                .iter()
                .fold(toml_manifest, |toml_manifest, dep| {
                    toml_manifest
                        + format!(
                            r###"
{name} = "{version}""###,
                            name = dep.package_name(),
                            version = dep.version_req()
                        )
                        .as_str()
                });

        let toml_manifest = toml_manifest.as_str();
        let toml_manifest: TomlManifest = toml::from_str(toml_manifest).unwrap();
        let (manifest, _paths) = TomlManifest::to_real_manifest(
            &std::rc::Rc::new(toml_manifest),
            source_id,
            base,
            &config,
        )
        .unwrap();

        manifest
    }

    use cargo::core::VirtualManifest;

    #[derive(Default, Debug, Clone)]
    struct Krate {
        name: &'static str,
        version: Option<Version>,
        dependencies: Vec<Dependency>,
    }

    impl Krate {
        pub fn version(&mut self, major: u64, minor: u64, patch: u64) -> &mut Self {
            self.version = Some(Version::new(major, minor, patch));
            self
        }

        pub fn add_dependency(
            &mut self,
            dependency: &'static str,
            version_req: &'static str,
        ) -> Result<&mut Self> {
            // TODO make this pretty
            let config = Config::default().unwrap();
            let source_id = SourceId::crates_io(&config)?;

            let dependency = Dependency::parse(dependency, version_req.into(), source_id)?;
            self.dependencies.push(dependency);
            Ok(self)
        }
    }

    #[derive(Default, Debug, Clone)]
    struct WorkspaceBuilder {
        base: PathBuf,
        krates: Vec<Krate>,
    }

    impl WorkspaceBuilder {
        pub fn add_crate(&mut self, name: &'static str) -> &mut Krate {
            let krate = Krate {
                name,
                version: None,
                dependencies: vec![],
            };
            self.krates.push(krate);
            self.krates.last_mut().unwrap()
        }

        pub fn build(self, base: impl AsRef<Path>) -> Result<Workspace<'static>> {
            let config = {
                let config = Config::default().unwrap();
                Box::leak(Box::new(config))
            };
            let base = base.as_ref();

            let source_id = SourceId::crates_io(&*config).unwrap();

            let manifests = self
                .krates
                .iter()
                .map(
                    |Krate {
                         name,
                         version,
                         dependencies,
                     }| {
                        Ok(make_manifest(
                            config,
                            base,
                            name,
                            version.clone().expect("Must have version. qed"),
                            source_id,
                            dependencies,
                        ))
                    },
                )
                .collect::<Result<Vec<Manifest>>>()?;

            let vconfig = WorkspaceConfig::Root(WorkspaceRootConfig::new(
                base,
                &Some(
                    manifests
                        .iter()
                        .map(|manifest| manifest.name().as_str().to_owned())
                        .collect(),
                ),
                &None,
                &Some(vec![]),
                &None,
            ));

            // crate the filesystem tree
            {
                std::fs::create_dir_all(base).unwrap();
                let content = format!(
                    r###"
    [workspace]
    members = [
        {}
    ]
    "###,
                    Itertools::intersperse(
                        manifests
                            .iter()
                            .map(|manifest| format!(r#""./{}""#, manifest.name().as_str())),
                        ", ".to_owned()
                    )
                    .collect::<String>()
                );
                std::fs::write(base.join("Cargo.toml"), content.as_bytes()).unwrap();
                for manifest in manifests.iter() {
                    let name = manifest.name().as_str();
                    let manifest_path = base.join(name);
                    std::fs::create_dir_all(manifest_path.join("src")).unwrap();
                    std::fs::write(
                        manifest_path.join("Cargo.toml"),
                        toml::to_string(manifest.original())
                            .unwrap()
                            .as_str()
                            .as_bytes(),
                    )
                    .unwrap();
                    std::fs::write(
                        manifest_path.join("src").join("lib.rs"),
                        format!(
                            r###"pub fn {name}() {{
                        println!("{name}")
                    }}
    "###,
                            name = name
                        )
                        .as_bytes(),
                    )
                    .unwrap();
                }
            }

            let vmanifest = VirtualManifest::new(
                vec![],
                HashMap::default(),
                vconfig,
                None,
                Features::default(),
                None,
            );

            let ws = Workspace::new_virtual(
                base.to_path_buf(),
                base.join("Cargo.toml"),
                vmanifest,
                &*config,
            )?;
            Ok(ws)
        }
    }

    /// Setup the following directory structure
    /// ```
    /// $OUT_DIR/integration
    /// ├── Cargo.toml
    /// ├── closing
    /// │   ├── Cargo.toml
    /// │   └── src
    /// │       └── lib.rs
    /// ├── dx
    /// │   ├── Cargo.toml
    /// │   └── src
    /// │       └── lib.rs
    /// ├── dy
    /// │   ├── Cargo.toml
    /// │   └── src
    /// │       └── lib.rs
    /// └── top
    ///     ├── Cargo.toml
    ///     └── src
    ///         └── lib.rs
    /// ```
    ///
    /// with the `Cargo.toml` in the `base` directory,
    /// containing only a `workspace` declaration.
    #[test]
    fn diamond() -> Result<()> {
        let target_dir = PathBuf::from(env!("OUT_DIR")).join("diamond");

        let mut wsb = WorkspaceBuilder::default();
        wsb.add_crate("top")
            .version(0, 1, 2)
            .add_dependency("dx", "1.11")?
            .add_dependency("dy", "15")?;
        wsb.add_crate("dx")
            .version(1, 11, 111)
            .add_dependency("closing", "1.6.4")?;
        wsb.add_crate("dy")
            .version(15, 100, 0)
            .add_dependency("closing", "1.6.1")?;
        wsb.add_crate("closing").version(1, 6, 9);

        let ws = wsb.build(target_dir)?;
        let to_release = packages_to_release(&ws, |_pkg| true, Some(PathBuf::from("diamond.dot")))
            .expect("There are no cycles in a diamond shaped, directed, dependency graph. qed");
        assert_eq!(to_release.len(), 4);
        Ok(())
    }
}

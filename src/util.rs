use cargo::{
    core::{dependency::DepKind, package::Package, Workspace},
    util::interning::InternedString,
    sources::PathSource,
};
use log::{warn, trace};
use std::{
    collections::{HashSet, HashMap},
    error::Error, fs,
};
use toml_edit::{Document, InlineTable, Item, Table, Value};
use petgraph::{Directed, Graph, graph::NodeIndex};
use git2::Repository;

#[derive(Debug, Clone, Copy, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct DepKindFmt(pub cargo::core::dependency::DepKind);
impl std::fmt::Display for DepKindFmt {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.0 {
            DepKind::Build => "build",
            DepKind::Development => "dev",
            DepKind::Normal => "regular",
        }.fmt(f)
    }
}

/// Calculate the dependency graph of packages that transitively depend on `changed` packages
pub fn changed_dependents<'a, F>(all_members: Vec<Package>, changed: &HashSet<Package>, with_dev: bool, predicate: F)
    -> (Graph::<Package, DepKindFmt, Directed, u32>, HashMap<InternedString, NodeIndex<u32>>)
where
    F: Fn(&Package) -> bool,
{
    let mut graph = Graph::new();

    // Collect all members that are selected with aim of being published or are
    // in the changed package set
    let map = all_members
        .iter()
        .filter(|member| predicate(&member) || changed.contains(member))
        .map(|member| (member.name(), graph.add_node(member.clone())))
        .collect::<HashMap<_, _>>();

    // Create a dependency graph of the member packages collected above
    for member in &all_members {
        // ignore entries we are not expected to publish
        let member_index = if let Some(i) = map.get(&member.name()) { i } else { continue };

        for dep in member.dependencies() {
            if let Some(dep_index) = map.get(&dep.package_name()) {
                // FIXME: Provide a nicer way to collect different deps
                if with_dev || dep.kind() != DepKind::Development {
                    graph.add_edge(*member_index, *dep_index, DepKindFmt(dep.kind()));
                }
            }
        }
    }

    log::debug!("Initial graph: {}", graph.node_count());
    if log::log_enabled!(log::Level::Trace) {
        use std::io::Write;
        let mut file = fs::File::create("dependents.before.dot").unwrap();
        let _ = std::write!(file, "{}", petgraph::dot::Dot::new(&graph));
    }

    // Retain packages in the graph that are transitively dependent on
    // a changed package, including themselves
    let mut transitively_dependent = HashSet::new();

    let mut visited = HashSet::new();
    let mut stack: Vec<_> = changed.iter().filter_map(|x| map.get(&x.name()).copied()).collect();
    while let Some(idx) = stack.pop() {
        visited.insert(idx);
        for idx in graph.neighbors_directed(idx, petgraph::Direction::Incoming) {
            if visited.insert(idx) {
                stack.push(idx);
            }
        }
        transitively_dependent.insert(idx);
    }

    graph.retain_nodes(|_, idx| transitively_dependent.contains(&idx));
    // Node indices were surely invalidated, so reconstruct the name -> idx map
    let map = graph
        .node_weights_mut()
        .enumerate()
        .map(|(idx, x)| (x.name(), NodeIndex::from(idx as u32)))
        .collect();

    if log::log_enabled!(log::Level::Trace) {
        use std::io::Write;
        let mut file = fs::File::create("dependents.after.dot").unwrap();
        let _ = std::write!(file, "{}", petgraph::dot::Dot::new(&graph));
    }

    (graph, map)
}

/// Calculate changed packages in a Cargo workspace since a Git `reference`.
///
/// This calculates changed files deltas between trees of HEAD and a given reference,
/// whose file names are used to naively match to a changed package.
pub fn changed_packages(ws: &Workspace, reference: &str) -> Result<HashSet<Package>, String> {
    ws.config()
        .shell()
        .status("Calculating", format!("git diff since {:}", reference))
        .expect("Writing to Shell doesn't fail");

    let path = ws.root();
    let repo = Repository::open(&path)
        .map_err(|e| format!("Workspace isn't a git repo: {:?}", e))?;
    let current_head = repo.head()
        .and_then(|b| b.peel_to_commit())
        .and_then(|c| c.tree())
        .map_err(|e| format!("Could not determine current git HEAD: {:?}", e))?;
    let main = repo
        .resolve_reference_from_short_name(reference)
        .and_then(|d| d.peel_to_commit())
        .and_then(|c| c.tree())
        .map_err(|e| format!("Reference not found in git repository: {:?}", e))?;

    let diff = repo
        .diff_tree_to_tree(Some(&current_head), Some(&main), None)
        .map_err(|e| format!("Diffing failed: {:?}", e))?;

    let files = diff
        .deltas()
        .filter_map(|d| d.new_file().path())
        .filter_map(|d| if d.is_file() { d.parent() } else { Some(d) })
        .map(|l| path.join(l))
        .collect::<Vec<_>>();

    trace!("Files changed since: {:#?}", files);

    let mut packages = HashSet::new();

    // FIXME: Can this overspecify packages? E.g. if we have:
    // 1. a_workspace/Cargo.toml
    // 2. a_workspace/crate_a/Cargo.toml
    // 3. a_workspace/crate_b/Cargo.toml
    // then for a dirty `workspace/crate_a/file.rs we will mark *both* 1. and 2.
    // package as dirty
    for m in members_deep(ws) {
        let root = m.root();
        for f in files.iter() {
            if f.starts_with(root) {
                packages.insert(m);
                break;
            }
        }
    }

    Ok(packages)
}

// Find all members of the workspace, into the total depth
pub fn members_deep<'a>(ws: &'a Workspace) -> Vec<Package> {
    let mut total_list = Vec::new();
    for m in ws.members() {
        total_list.push(m.clone());
        for dep in m.dependencies() {
            let source = dep.source_id();
            if source.is_path() {
                let dst = source
                    .url()
                    .to_file_path()
                    .expect("It was just checked before. qed");
                let mut src = PathSource::new(&dst, source, ws.config());
                let pkg = src.root_package().expect("Path must have a package");
                if !ws.is_member(&pkg) {
                    total_list.push(pkg);
                }
            }
        }
    }
    total_list
}

/// Run f on every package's manifest, write the doc. Fail on first error
pub fn edit_each<'a, I, F, R>(iter: I, f: F) -> Result<Vec<R>, Box<dyn Error>>
where
    F: Fn(&'a Package, &mut Document) -> Result<R, Box<dyn Error>>,
    I: Iterator<Item = &'a Package>,
{
    let mut results = Vec::new();
    for pkg in iter {
        let manifest_path = pkg.manifest_path();
        let content = fs::read_to_string(manifest_path)?;
        let mut doc: Document = content.parse()?;
        results.push(f(pkg, &mut doc)?);
        fs::write(manifest_path, doc.to_string())?;
    }
    Ok(results)
}

/// Wrap each the different dependency as a mutable item
pub enum DependencyEntry<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

#[derive(Debug, PartialEq)]
/// The action (should be) taken on the dependency entry
pub enum DependencyAction {
    /// Ignored, we didn't touch
    Untouched,
    /// Entry was changed, needs to be saved
    Mutated,
    /// Remove this entry and save the manifest
    Remove,
}

/// Iterate through the dependency sections of root, find each
/// dependency entry, that is a subsection and hand it and its name
/// to f. Return the counter of how many times f returned true.
pub fn edit_each_dep<F>(root: &mut Table, f: F) -> u32
where
    F: Fn(String, Option<String>, DependencyEntry) -> DependencyAction,
{
    let mut counter = 0;
    let mut removed = Vec::new();
    for k in &["dependencies", "dev-dependencies", "build-dependencies"] {
        let keys = {
            if let Some(Item::Table(t)) = &root.get(k) {
                t.iter()
                    .filter_map(|(key, v)| {
                        if v.is_table() || v.is_inline_table() {
                            Some(key.to_owned())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                continue;
            }
        };
        let t = root.entry(k).as_table_mut().expect("Just checked. qed");

        for key in keys {
            let (name, action) = match t.entry(&key) {
                Item::Value(Value::InlineTable(info)) => {
                    let (name, alias) = {
                        if let Some(name) = info.get("package") {
                            // is there a rename
                            (name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned(),
                            Some(key.clone()))
                        } else {
                            (key.clone(), None)
                        }
                    };
                    (name.clone(), f(name, alias, DependencyEntry::Inline(info)))
                }
                Item::Table(info) => {
                    let (name, alias) = {
                        if let Some(name) = info.get("package") {
                            // is there a rename
                            (name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned(),
                            Some(key.clone()))
                        } else {
                            (key.clone(), None)
                        }
                    };

                    (name.clone(), f(name, alias, DependencyEntry::Table(info)))
                }
                _ => {
                    warn!("Unsupported dependency format");
                    (key, DependencyAction::Untouched)
                }
            };

            if action == DependencyAction::Remove {
                t.remove(&name);
                removed.push(name);
            }
            if action != DependencyAction::Untouched {
                counter += 1;
            }
        }
    }

    if !removed.is_empty() {
        if let Item::Table(features) = root.entry("features") {
            let keys = features
                .iter()
                .map(|(k, _v)| k.to_owned())
                .collect::<Vec<_>>();
            for feat in keys {
                if let Item::Value(Value::Array(deps)) = features.entry(&feat) {
                    let mut to_remove = Vec::new();
                    for (idx, dep) in deps.iter().enumerate() {
                        if let Value::String(s) = dep {
                            if let Some(s) = s.value().trim().split('/').next() {
                                if removed.contains(&s.to_owned()) {
                                    to_remove.push(idx);
                                }
                            }
                        }
                    }
                    if !to_remove.is_empty() {
                        // remove starting from the end:
                        to_remove.reverse();
                        for idx in to_remove {
                            deps.remove(idx);
                        }
                    }
                }
            }
        }
    }
    counter
}
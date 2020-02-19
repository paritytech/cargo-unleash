use cargo::{
    core::{
        Workspace,
        package::Package,
    },
};
use std::collections::HashMap;
use petgraph::Graph;
use log::{trace, warn};

/// Generate the packages we should be releasing
pub fn packages_to_release<'a, F>(ws: &Workspace<'a>, predicate: F) -> Result<Vec<Package>, String>
    where F: Fn(&Package) -> bool
{
    // based on the work of `cargo-publish-all`: https://gitlab.com/torkleyy/cargo-publish-all
    ws.config().shell().status("Resolving", "Dependency Tree").expect("Writing to Shell failed");

    let mut graph = Graph::<Package, (), _, _>::new();
    let mut map = HashMap::new();
    let mut ignored = HashMap::new();

    for member in ws.members() {
        if !predicate(&member) {
            let _ = ignored.insert(member.name(), member.clone());
        } else {
            let index = graph.add_node(member.clone());
            // Package names assumed to be unique
            if let Some(_) = map.insert(member.name(), index) {
                return Err(format!("ERR: {:} found more than once in the package tree", member.name()))
            }
        }
    }

    for member in ws.members() {
        let current_index = match map.get(&member.name()) {
            Some(i) => i,
            _ => continue // ignore entries we are not expected to publish
        };

        for dep in member.dependencies() {

            if let Some(dep_index) = map.get(&dep.package_name()) {
                graph.add_edge(*current_index, *dep_index, ());
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
                    if let Some(_) = ignored.get(&dep.package_name()) {
                        warn!("{} lock depends on {}, which is expected to not be published. This might fail.", member.name(), dep.package_name())
                    }
                }
            }
        }
    }

    let indices = petgraph::algo::toposort(&graph, None)
        .map_err(|c| format!("Cycle detected: {:}", graph.node_weight(c.node_id()).unwrap().name().to_string()))?;
    let packages = indices
        .into_iter()
        .map(|i| graph.node_weight(i).unwrap().clone())
        .rev()
        .collect();

    Ok(packages)
}
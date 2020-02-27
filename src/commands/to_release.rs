use cargo::{
    core::{
        Workspace,
        package::Package,
    },
};
use std::collections::{
    HashMap,
    HashSet,
};
use petgraph::Graph;
use log::{trace, warn};
use crate::util::fetch_many_cratesio_versions;

/// Generate the packages we should be releasing
pub fn packages_to_release<'a, F>(ws: &Workspace<'a>, predicate: F) -> Result<Vec<Package>, String>
    where F: Fn(&Package) -> bool
{
    // inspired by the work of `cargo-publish-all`: https://gitlab.com/torkleyy/cargo-publish-all
    ws.config().shell().status("Resolving", "Dependency Tree")
        .expect("Writing to Shell doesn't fail");

    let mut graph = Graph::<Package, (), _, _>::new();

    let (members, to_ignore): (Vec<_>, Vec<_>) = ws.members().partition(|m| predicate(&m));

    let ignored = to_ignore.into_iter().map(|m| m.name()).collect::<HashSet<_>>();

    ws.config().shell().status("Syncing", "Versions from crates.io")
        .expect("Writing to Shell doesn't fail");

    let published_versions = fetch_many_cratesio_versions(members
        .iter()
        .map(|m| m.name().to_string())
        .collect::<Vec<_>>()
    )?;
    let already_published = members.iter().filter_map(|member| {
        if let Some(versions) = published_versions.get(&member.name().to_string()) {
            for v in versions {
                if &v.version == member.version() {
                    return Some(member.name())
                }
            }
        }
        None
    }).collect::<HashSet<_>>();

    let map = members.into_iter().filter_map(|member| {
        if ignored.contains(&member.name()) || already_published.contains(&member.name()) {
            return None
        }
        Some((member.name(), graph.add_node(member.clone())))
    }).collect::<HashMap<_, _>>();

    for member in ws.members() {
        let current_index = match map.get(&member.name()) {
            Some(i) => i,
            _ => continue // ignore entries we are not expected to publish
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

    let indices = petgraph::algo::toposort(&graph, None)
        .map_err(|c| format!("Cycle detected: {:}", graph.node_weight(c.node_id()).unwrap().name().to_string()))?;
    let packages = indices
        .into_iter()
        .map(|i| graph.node_weight(i).unwrap().clone())
        .rev()
        .collect::<Vec<_>>();

    if packages.len() == 0 {
        return Err("No Packages matching criteria. Exiting".into());
    }

    Ok(packages)
}
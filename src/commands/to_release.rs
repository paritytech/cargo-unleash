use crate::util::members_deep;
use cargo::{
    core::{package::Package, Dependency, Source, SourceId, Workspace},
    sources::registry::RegistrySource,
};
use log::{trace, warn};
use petgraph::Graph;
use std::collections::{HashMap, HashSet};

/// Generate the packages we should be releasing
pub fn packages_to_release<F>(
    ws: &Workspace<'_>,
    predicate: F,
) -> Result<Vec<Package>, anyhow::Error>
where
    F: Fn(&Package) -> bool,
{
    // inspired by the work of `cargo-publish-all`: https://gitlab.com/torkleyy/cargo-publish-all
    ws.config()
        .shell()
        .status("Resolving", "Dependency Tree")
        .expect("Writing to Shell doesn't fail");

    let mut graph = Graph::<Package, (), _, _>::new();
    let members = members_deep(ws);

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

    // cannot use `toposort` for graphs that are cyclic in a undirected sense
    // but are not in a directed way
    // TODO check if this is a bug in the toposort impl
    let mut cycles = vec![];
    let mut toposorted_indices = vec![];
    let strongly_connected_sets = petgraph::algo::kosaraju_scc(&graph);
    for strongly_connected in strongly_connected_sets {
        match strongly_connected.len() {
            0 => unreachable!("Strongly connected components are at least size 1. qed"),
            1 => toposorted_indices.push(strongly_connected[0].clone()),
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

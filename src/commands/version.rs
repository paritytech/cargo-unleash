use crate::util::{
	edit_each, edit_each_dep, members_deep, DependencyAction, DependencyEntry, DependencySection,
};
use anyhow::Context;
use cargo::core::{package::Package, Workspace};
use log::trace;
use semver::{Version, VersionReq};
use std::collections::HashMap;
use toml_edit::{Entry, Item, Value};

fn check_for_update(
	name: String,
	wrap: DependencyEntry<'_>,
	updates: &HashMap<String, Version>,
	section: DependencySection,
	force_update: bool,
) -> DependencyAction {
	let new_version = if let Some(v) = updates.get(&name) {
		v
	} else {
		return DependencyAction::Untouched // we do not care about this entry
	};

	match wrap {
		DependencyEntry::Inline(info) => {
			if !info.contains_key("path") {
				return DependencyAction::Untouched // entry isn't local
			}

			trace!("We changed the version of {:} to {:}", name, new_version);
			// this has been changed.
			if let Some(v_req) = info.get_mut("version") {
				let r = v_req
					.as_str()
					.ok_or_else(|| anyhow::anyhow!("Version must be string"))
					.and_then(|s| VersionReq::parse(s).context("Parsing failed"))
					.expect("Cargo enforces us using semver versions. qed");
				if force_update || !r.matches(new_version) {
					trace!("Versions don't match anymore, updating.");
					*v_req = Value::from(format!("{:}", new_version)).decorated(" ", "");
					return DependencyAction::Mutated
				}
			} else if section == DependencySection::Dev {
				trace!("No version found on dev dependency, ignoring.");
				return DependencyAction::Untouched
			} else {
				// not yet present, we force set.
				trace!("No version found, setting.");
				// having a space here means we formatting it nicer inline
				info.get_or_insert(
					" version",
					Value::from(format!("{:}", new_version)).decorated(" ", " "),
				);
				return DependencyAction::Mutated
			}
		},
		DependencyEntry::Table(info) => {
			if !info.contains_key("path") {
				return DependencyAction::Untouched // entry isn't local
			}
			if let Some(new_version) = updates.get(&name) {
				trace!("We changed the version of {:} to {:}", name, new_version);
				// this has been changed.
				if let Some(v_req) = info.get("version") {
					let r = v_req
						.as_str()
						.ok_or_else(|| anyhow::anyhow!("Version must be string"))
						.and_then(|s| VersionReq::parse(s).context("Parsing failed"))
						.expect("Cargo enforces us using semver versions. qed");
					if !force_update && r.matches(new_version) {
						return DependencyAction::Untouched
					}
					trace!("Versions don't match anymore, updating.");
				} else if section == DependencySection::Dev {
					trace!("No version found on dev dependency {:}, ignoring.", name);
					return DependencyAction::Untouched
				} else {
					trace!("No version found, setting.");
				}
				info["version"] =
					Item::Value(Value::from(format!("{:}", new_version)).decorated(" ", ""));
				return DependencyAction::Mutated
			}
		},
	}
	DependencyAction::Untouched
}

/// For packages matching predicate set to mapper given version, if any. Update all members
/// dependencies if necessary.
pub fn set_version<M, P>(
	ws: &Workspace<'_>,
	predicate: P,
	mapper: M,
	force_update: bool,
) -> Result<(), anyhow::Error>
where
	P: Fn(&Package) -> bool,
	M: Fn(&Package) -> Option<Version>,
{
	let c = ws.config();

	let updates = edit_each(members_deep(ws).iter().filter(|p| predicate(p)), |p, doc| {
		Ok(mapper(p).map(|nv_version| {
			c.shell()
				.status("Bumping", format!("{:}: {:} -> {:}", p.name(), p.version(), nv_version))
				.expect("Writing to the shell would have failed before. qed");
			doc["package"]["version"] =
				Item::Value(Value::from(nv_version.to_string()).decorated(" ", ""));
			(p.name().as_str().to_owned(), nv_version)
		}))
	})?
	.into_iter()
	.flatten()
	.collect::<HashMap<_, _>>();

	c.shell().status("Updating", "Dependency tree")?;
	edit_each(members_deep(ws).iter(), |p, doc| {
		c.shell().status("Updating", p.name())?;
		let root = doc.as_table_mut();
		let mut updates_count = 0;
		updates_count += edit_each_dep(root, |name, _, wrap, section| {
			check_for_update(name, wrap, &updates, section, force_update)
		});

		if let Entry::Occupied(occupied) = root.entry("target") {
			if let Item::Table(table) = occupied.get() {
				let keys = table
					.iter()
					.filter_map(|(k, v)| if v.is_table() { Some(k.to_owned()) } else { None })
					.collect::<Vec<_>>();

				for k in keys {
					if let Some(Item::Table(root)) = root.get_mut(&k) {
						updates_count += edit_each_dep(root, |a, _, b, c| {
							check_for_update(a, b, &updates, c, force_update)
						});
					}
				}
			}
		}
		if updates_count == 0 {
			c.shell().status("Done", "No dependency updates")?;
		} else if updates_count == 1 {
			c.shell().status("Done", "One dependency updated")?;
		} else {
			c.shell().status("Done", format!("{} dependencies updated", updates_count))?;
		}

		Ok(())
	})?;

	Ok(())
}

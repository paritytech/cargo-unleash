use crate::util::{edit_each, edit_each_dep, members_deep, DependencyAction, DependencyEntry};
use cargo::core::{package::Package, Workspace};
use log::trace;
use std::collections::HashMap;
use toml_edit::{Item, Value};

fn check_for_update(
	name: String,
	wrap: DependencyEntry<'_>,
	updates: &HashMap<String, String>,
) -> DependencyAction {
	let new_name = if let Some(v) = updates.get(&name) {
		v
	} else {
		return DependencyAction::Untouched // we do not care about this entry
	};

	match wrap {
		DependencyEntry::Inline(info) => {
			if !info.contains_key("path") {
				return DependencyAction::Untouched // entry isn't local
			}

			trace!("We renamed {:} to {:}", name, new_name);
			info.get_or_insert(" package", Value::from(new_name.to_string()).decorated(" ", " "));

			DependencyAction::Mutated
		},
		DependencyEntry::Table(info) => {
			if !info.contains_key("path") {
				return DependencyAction::Untouched // entry isn't local
			}

			info["package"] = Item::Value(Value::from(new_name.to_string()).decorated(" ", ""));

			DependencyAction::Mutated
		},
	}
}

/// For packages matching predicate set to mapper given version, if any. Update all members
/// dependencies if necessary.
pub fn rename<M, P>(ws: &Workspace<'_>, predicate: P, mapper: M) -> Result<(), anyhow::Error>
where
	P: Fn(&Package) -> bool,
	M: Fn(&Package) -> Option<String>,
{
	let c = ws.config();

	let updates = edit_each(members_deep(ws).iter().filter(|p| predicate(p)), |p, doc| {
		Ok(mapper(p).map(|new_name| {
			c.shell()
				.status("Renaming", format!("{:} -> {:}", p.name(), new_name))
				.expect("Writing to the shell would have failed before. qed");
			doc["package"]["name"] =
				Item::Value(Value::from(new_name.to_string()).decorated(" ", ""));
			(p.name().as_str().to_owned(), new_name)
		}))
	})?
	.into_iter()
	.flatten()
	.collect::<HashMap<_, _>>();

	if updates.is_empty() {
		c.shell().status("Done", "No changed applied")?;
		return Ok(())
	}

	c.shell().status("Updating", "Dependency tree")?;
	edit_each(members_deep(ws).iter(), |p, doc| {
		c.shell().status("Updating", p.name())?;
		let root = doc.as_table_mut();
		let mut updates_count = 0;
		updates_count += edit_each_dep(root, |a, _, b, _| check_for_update(a, b, &updates));

		if let Some(Item::Table(table)) = root.get_mut("target") {
			let keys = table
				.iter()
				.filter_map(|(k, v)| if v.is_table() { Some(k.to_owned()) } else { None })
				.collect::<Vec<_>>();

			for k in keys {
				if let Some(Item::Table(root)) = table.get_mut(&k) {
					updates_count +=
						edit_each_dep(root, |a, _, b, _| check_for_update(a, b, &updates));
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

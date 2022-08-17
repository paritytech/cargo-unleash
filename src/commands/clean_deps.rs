use crate::util::{edit_each, edit_each_dep, members_deep, DependencyAction};
use cargo::core::{package::Package, Workspace};
// use log::trace;
use std::process::Command;

pub fn clean_up_unused_dependencies<P>(
	ws: &Workspace<'_>,
	predicate: P,
	check_only: bool,
) -> Result<(), anyhow::Error>
where
	P: Fn(&Package) -> bool,
{
	let c = ws.config();

	// inspired by https://gist.github.com/sinkuu/8083240257c485c9f928744b41bbac98
	let total = edit_each(members_deep(ws).iter().filter(|p| predicate(p)), |p, doc| {
		c.shell().status("Checking", p.name())?;
		let source_path = p.root();
		let root = doc.as_table_mut();
		Ok(edit_each_dep(root, |p_name, alias, _table, _| {
			let name = alias.unwrap_or(p_name);
			let found = Command::new("rg")
				.args(&["--type", "rust"])
				.arg("-qw")
				.arg(name.replace('-', "_"))
				.arg(&source_path)
				.status()
				.unwrap()
				.success();

			if !found {
				if check_only {
					c.shell().status("Not needed", name).expect("Writing to Shell works");
					DependencyAction::Untouched
				} else {
					c.shell().status("Removed", name).expect("Writing to Shell works");
					DependencyAction::Remove
				}
			} else {
				DependencyAction::Untouched
			}
		}))
	})
	.map(|v| v.iter().sum::<u32>());

	match total? {
		t if t > 0 && check_only => {
			anyhow::bail!("Aborting: {:} unused dependencies found. See shell output for more.", t)
		},
		_ => {},
	}
	Ok(())
}

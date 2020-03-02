use crate::util::{edit_each, edit_each_dep, DependencyAction};
use cargo::core::{package::Package, Workspace};
// use log::trace;
use std::{
    process::Command,
    error::Error
};
// use toml_edit::{decorated, Item, Value};

pub fn clean_up_unused_dependencies<'a, P>(
    ws: &Workspace<'a>,
    predicate: P,
    check_only: bool,
) -> Result<(), Box<dyn Error>>
where
    P: Fn(&Package) -> bool,
{
    let c = ws.config();
    
    // inspired by https://gist.github.com/sinkuu/8083240257c485c9f928744b41bbac98
    let total = edit_each(ws.members().filter(|p| predicate(p)), |p, doc| {
        c.shell().status("Checking", p.name())?;
        let source_path = p.root();
        let root = doc.as_table_mut();
        Ok(edit_each_dep(root, |p_name, alias, _table| {
            let name = alias.unwrap_or(p_name);
            let found = Command::new("rg")
                .args(&["--type", "rust"])
                .arg("-qw")
                .arg(name.replace("-", "_"))
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
    }).map(|v| v.iter().sum::<u32>());

    match total {
        Ok(t) if t > 0 && check_only => {
            Err(format!("Aborting: {:} unused dependencies found. See shell output for more.", t).into())
        },
        Ok(_) => Ok(()),
        Err(e) => Err(e)
    }
}

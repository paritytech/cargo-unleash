use crate::util::{edit_each, edit_each_dep};
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
) -> Result<(), Box<dyn Error>>
where
    P: Fn(&Package) -> bool,
{
    let c = ws.config();
    
    // inspired by https://gist.github.com/sinkuu/8083240257c485c9f928744b41bbac98
    edit_each(ws.members().filter(|p| predicate(p)), |p, doc| {
        c.shell().status("Checking", p.name())?;
        let source_path = p.root();
        let root = doc.as_table_mut();
        edit_each_dep(root, |p_name, alias, _table| {
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
                c.shell().status("Not needed", name).expect("Writing to Shell works");
            }

            !found
        });

        Ok(())
    }).map(|_| ())
}

mod de_dev_deps;
use crate::cli::{Command, Opt};
use flexi_logger::Logger;
use log::trace;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use toml_edit::{Document, Item, Value};

fn run_recursive<F>(manifest_path: PathBuf, f: F) -> Result<(), Box<dyn std::error::Error>>
where
    F: Fn(&mut Document, &Path) -> Result<(), Box<dyn std::error::Error>>,
{
    let content = fs::read_to_string(&manifest_path)?;
    let base_path = manifest_path
        .parent()
        .expect("Was abe to read the file, there must be a directory. qed");
    let mut doc: Document = content.parse()?;
    let _ = f(&mut doc, &manifest_path)?;
    trace!("reading members of {:?}", manifest_path);
    let members = {
        if let Some(Item::Table(workspace)) = doc.as_table().get("workspace") {
            if let Some(Item::Value(Value::Array(members))) = workspace.get("members") {
                members
                    .iter()
                    .filter_map(|m| m.as_str())
                    .collect::<Vec<_>>()
            } else {
                return Err(format!("Members not found in {:?}", &manifest_path).into());
            }
        } else {
            vec![]
        }
    };

    trace!("Members found: {:?}", members);

    for m in members {
        let filename = base_path.join(m).join("Cargo.toml");
        trace!("Running on {:?}", filename);
        let mut doc: Document = fs::read_to_string(&filename)?.parse()?;
        let _ = f(&mut doc, &filename)?;
    }

    Ok(())
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log).start();

    let manifest_path = {
        let mut path = args.manifest_path;
        if path.is_dir() {
            path = path.join("Cargo.toml")
        }
        path
    };

    trace!("Uising manifest {:?}", &manifest_path);
    match args.cmd {
        Command::DeDevDeps => {
            run_recursive(manifest_path, de_dev_deps::deactivate_dev_dependencies)
        }
    }
}

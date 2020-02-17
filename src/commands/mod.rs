mod de_dev_deps;
mod to_release;
use crate::cli::{Command, Opt};
use flexi_logger::Logger;
use log::trace;
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use cargo::{util::config::Config as CargoConfig, core::{package::Package, Workspace}};
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

fn _run(cmd: Command, root_manifest: PathBuf) -> Result<(), Box<dyn Error>> {
    match cmd {
        Command::DeDevDeps => {
            run_recursive(root_manifest, de_dev_deps::deactivate_dev_dependencies)
        }
        Command::ToRelease { skip, ignore_version_pre } => {
            let c = CargoConfig::default().expect("Couldn't create cargo config");
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let skipper = |p: &Package| {
                if let Some(false) = p.publish().as_ref().map(|v| v.is_empty()) {
                    trace!("Skipping {} because it shouldn't be published", p.name());
                    return true
                }
                if skip.contains(&p.name()) {
                    return true
                }
                if p.version().is_prerelease() {
                    for pre in &p.version().pre {
                        if ignore_version_pre.contains(&pre) {
                            return true
                        }
                    }
                }
                false
            };
            let packages = to_release::packages_to_release(ws, skipper)?;
            println!("{:?}", packages.iter().map(|p| p.name()).collect::<Vec<_>>());
            Ok(())
        }
    }
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log).start();
    let manifest_path = {
        let mut path = args.manifest_path;
        if path.is_dir() {
            path = path.join("Cargo.toml")
        }
        fs::canonicalize(path)?
    };

    trace!("Using manifest {:?}", &manifest_path);
    _run(args.cmd, manifest_path)
}

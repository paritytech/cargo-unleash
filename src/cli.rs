use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use cargo::core::InternedString;
use semver::Identifier;
use toml_edit::{Document, Item, Value};
use cargo::{
    util::config::Config as CargoConfig,
    core::{
        package::Package,
        Workspace,
    },
};
use flexi_logger::Logger;
use log::trace;

fn parse_identifiers(src: &str) -> Identifier {
    Identifier::AlphaNumeric(src.to_owned())
}
use crate::commands;

#[derive(StructOpt, Debug)]
pub enum Command {
    /// deactivate the development dependencies
    DeDevDeps,
    /// calculate the packages that should be released, in the order they should be released
    ToRelease {
        /// skip the packages named ...
        #[structopt(long, parse(from_str))]
        skip: Vec<InternedString>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev")]
        ignore_version_pre: Vec<Identifier>,
    }
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "carg-unleash",
    about = "Release the crates of this massiv monorepo"
)]
pub struct Opt {
    /// Output file, stdout if not present
    #[structopt(long, parse(from_os_str), default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,
    /// Specify the log levels
    #[structopt(long = "log-level", short = "l", default_value = "warn")]
    pub log: String,

    #[structopt(subcommand)]
    pub cmd: Command,
}


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

fn to_release<'a>(ws: &Workspace<'a>, skip: Vec<InternedString>, ignore_version_pre: Vec<Identifier>)
    -> Result<Vec<Package>, String>
{
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
    commands::packages_to_release(ws, skipper)
}

fn _run(cmd: Command, root_manifest: PathBuf) -> Result<(), Box<dyn Error>> {
    let c = CargoConfig::default().expect("Couldn't create cargo config");
    match cmd {
        Command::DeDevDeps => {
            run_recursive(root_manifest, commands::deactivate_dev_dependencies)
        }
        Command::ToRelease { skip, ignore_version_pre } => {
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = to_release(&ws, skip,  ignore_version_pre)?;
            println!("{:}", packages
                .iter()
                    .map(|p| format!("{} ({})", p.name(), p.version()))
                .collect::<Vec<String>>()
                .join(", ")
            );
            Ok(())
        }
    }
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log.clone()).start();
    trace!("Running with config {:?}", args);
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

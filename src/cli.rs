use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use semver::Identifier;
use toml_edit::{Document, Item, Value};
use cargo::{
    util::config::Config as CargoConfig,
    core::{
        package::Package,
        Verbosity, Workspace
    },
};
use flexi_logger::Logger;
use regex::Regex;
use log::trace;

fn parse_identifiers(src: &str) -> Identifier {
    Identifier::AlphaNumeric(src.to_owned())
}
fn parse_regex(src: &str) -> Result<Regex, String> {
    Regex::new(src)
        .map_err(|e| format!("Parsing Regex failed: {:}", e))
}
use crate::commands;


#[derive(StructOpt, Debug)]
pub enum Command {
    /// deactivate the development dependencies
    DeDevDeps,
    /// calculate the packages that should be released, in the order they should be released
    ToRelease {
        /// skip the package names matching ...
        #[structopt(long, parse(try_from_str = parse_regex))]
        skip: Vec<Regex>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev")]
        ignore_version_pre: Vec<Identifier>,
    },
    /// Unleash 'em dragons 
    Em {
        /// skip the package names matching ...
        #[structopt(long, parse(try_from_str = parse_regex))]
        skip: Vec<Regex>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev")]
        ignore_version_pre: Vec<Identifier>,
        /// dry run
        #[structopt(long="dry-run")]
        dry_run: bool,
        // the token to use for uploading
        #[structopt(long, env = "CRATES_TOKEN", hide_env_values = true)]
        token: Option<String>
    }
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "cargo-unleash",
    about = "Release the crates of this massiv monorepo"
)]
pub struct Opt {
    /// Output file, stdout if not present
    #[structopt(long, parse(from_os_str), default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,
    /// Specify the log levels
    #[structopt(long = "log-level", short = "l", default_value = "warn")]
    pub log: String,
    /// Show verbose cargo output
    #[structopt(long = "verbose", short = "v")]
    pub verbose: bool,

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

fn to_release<'a>(ws: &Workspace<'a>, skip: Vec<Regex>, ignore_version_pre: Vec<Identifier>)
    -> Result<Vec<Package>, String>
{
    let skipper = |p: &Package| {
        if let Some(false) = p.publish().as_ref().map(|v| v.is_empty()) {
            trace!("Skipping {} because it shouldn't be published", p.name());
            return true
        }
        let name = p.name();
        if skip.iter().find(|r| r.is_match(&name)).is_some() {
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

fn release<'a>(ws: Workspace<'a>, dry_run:bool, token: Option<String>, skip: Vec<Regex>, ignore_version_pre: Vec<Identifier>)
    -> Result<(), Box<dyn Error>>
{

    let packages = to_release(&ws, skip, ignore_version_pre)?;

    ws.config().shell().status("Releasing", &packages
        .iter()
        .map(|p| format!("{} ({})", p.name(), p.version()))
        .collect::<Vec<String>>()
        .join(", ")
    )?;
    commands::release(packages, ws, dry_run, token)
}

fn _run(args: Opt, root_manifest: PathBuf) -> Result<(), Box<dyn Error>> {
    let c = CargoConfig::default().expect("Couldn't create cargo config");
    c.shell().set_verbosity(if args.verbose { Verbosity::Verbose }  else { Verbosity::Normal });
    match args.cmd {
        Command::DeDevDeps => {
            c.shell().status("Preparing", "Disabling Dev Dependencies for all crates")?;
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
        Command::Em { dry_run, skip, token, ignore_version_pre } => {
            c.shell().status("Preparing", "Disabling Dev Dependencies for all crates")?;
            // we first disable dev-dependencies
            run_recursive(root_manifest.clone(), commands::deactivate_dev_dependencies)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            release(ws, dry_run, token, skip, ignore_version_pre)
        }
    }
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log.clone()).start();
    trace!("Running with config {:?}", args);
    let manifest_path = {
        let mut path = args.manifest_path.clone();
        if path.is_dir() {
            path = path.join("Cargo.toml")
        }
        fs::canonicalize(path)?
    };

    trace!("Using manifest {:?}", &manifest_path);
    _run(args, manifest_path)
}

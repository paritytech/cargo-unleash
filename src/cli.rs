use std::{
    error::Error,
    fs,
    path::PathBuf,
};
use structopt::StructOpt;
use semver::Identifier;
use cargo::{
    util::config::Config as CargoConfig,
    core::{
        package::Package,
        InternedString,
        Verbosity, Workspace
    },
};
use flexi_logger::Logger;
use log::trace;
use regex::Regex;

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
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
    },
    /// Check packages
    Check {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
    },
    /// Unleash 'em dragons 
    EmDragons {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
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
    /// Only use the specfic set of packages
    #[structopt(short, long, parse(from_str))]
    pub packages: Vec<InternedString>,
    /// skip the package names matching ...
    #[structopt(short, long, parse(try_from_str = parse_regex))]
    pub skip: Vec<Regex>,
    /// ignore version pre-releases, comma separated
    #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev")]
    pub ignore_version_pre: Vec<Identifier>,

    #[structopt(subcommand)]
    pub cmd: Command,
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log.clone()).start();
    let c = CargoConfig::default().expect("Couldn't create cargo config");
    c.shell().set_verbosity(
        if args.verbose {
            Verbosity::Verbose
        }  else {
            Verbosity::Normal
        }
    );

    let root_manifest = {
        let mut path = args.manifest_path.clone();
        if path.is_dir() {
            path = path.join("Cargo.toml")
        }
        fs::canonicalize(path)?
    };

    let (packages, skip, ignore_version_pre) = {
        if !args.packages.is_empty() {
            if !args.skip.is_empty() || !args.ignore_version_pre.is_empty() {
                return Err("-p/--packages is mutually exlusive to using -s/--skip and -i/--ignore-version-pre".into())
            }
        }
        (&args.packages, &args.skip, &args.ignore_version_pre)
    };

    let predicate = |p: &Package| {
        if let Some(false) = p.publish().as_ref().map(|v| v.is_empty()) {
            trace!("Skipping {} because it shouldn't be published", p.name());
            return false
        }
        let name = &p.name();
        if !packages.is_empty() {
            return packages.contains(name)
        }
        if skip.iter().find(|r| r.is_match(name)).is_some() {
            return false
        }
        if p.version().is_prerelease() {
            for pre in &p.version().pre {
                if ignore_version_pre.contains(&pre) {
                    return false
                }
            }
        }
        true
    };
    
    let maybe_patch = |shouldnt_patch| {
        if shouldnt_patch { return Ok(()); }
    
        c.shell().status("Preparing", "Disabling Dev Dependencies for all crates")?;
            
        let ws = Workspace::new(&root_manifest, &c)
            .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
        commands::deactivate_dev_dependencies(ws, predicate)
    };

    match args.cmd {
        Command::DeDevDeps => {
            maybe_patch(false)
        }
        Command::ToRelease { include_dev } => {
            maybe_patch(include_dev)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;

            println!("{:}", packages
                .iter()
                    .map(|p| format!("{} ({})", p.name(), p.version()))
                .collect::<Vec<String>>()
                .join(", ")
            );

            Ok(())
        }
        Command::Check { include_dev } => {
            maybe_patch(include_dev)?;
            
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;

            commands::check(&packages, &ws)
        }
        Command::EmDragons { dry_run, token, include_dev } => {
            maybe_patch(include_dev)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;

            let packages = commands::packages_to_release(&ws, predicate)?;

            commands::check(&packages, &ws)?;

            ws.config().shell().status("Releasing", &packages
                .iter()
                .map(|p| format!("{} ({})", p.name(), p.version()))
                .collect::<Vec<String>>()
                .join(", ")
            )?;

            commands::release(packages, ws, dry_run, token)
        }
    }
}

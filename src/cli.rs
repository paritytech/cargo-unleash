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
        Verbosity, Workspace
    },
};
use flexi_logger::Logger;
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
        /// skip the package names matching ...
        #[structopt(long, parse(try_from_str = parse_regex))]
        skip: Vec<Regex>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev")]
        ignore_version_pre: Vec<Identifier>,
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
    },
    /// Check packages
    Check {
        /// skip the package names matching ...
        #[structopt(long, parse(try_from_str = parse_regex))]
        skip: Vec<Regex>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers))]
        ignore_version_pre: Vec<Identifier>,
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
    },
    /// Unleash 'em dragons 
    Em {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
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
    
    let maybe_patch = |shouldnt_patch: bool| {
        if shouldnt_patch { return Ok(()); }
    
        c.shell().status("Preparing", "Disabling Dev Dependencies for all crates")?;
        commands::deactivate_dev_dependencies(root_manifest.clone())
    };

    match args.cmd {
        Command::DeDevDeps => {
            maybe_patch(false)
        }
        Command::ToRelease { skip, ignore_version_pre, include_dev } => {
            maybe_patch(include_dev)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, skip,  ignore_version_pre)?;

            println!("{:}", packages
                .iter()
                    .map(|p| format!("{} ({})", p.name(), p.version()))
                .collect::<Vec<String>>()
                .join(", ")
            );

            Ok(())
        }
        Command::Check { skip, ignore_version_pre, include_dev } => {
            maybe_patch(include_dev)?;
            
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, skip,  ignore_version_pre)?;

            commands::check(&packages, &ws)
        }
        Command::Em { dry_run, skip, token, ignore_version_pre, include_dev } => {
            maybe_patch(include_dev)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;

            let packages = commands::packages_to_release(&ws, skip, ignore_version_pre)?;

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

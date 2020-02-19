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
use log::trace;
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
pub struct PackageSelectOptions {
    /// Only use the specfic set of packages
    ///
    /// Apply only to the packages named as defined. This is mutuable exclusive with skip and ignore-version-pre.
    /// Default: []
    #[structopt(short, long, parse(from_str))]
    pub packages: Vec<InternedString>,
    /// skip the package names matching ...
    ///
    /// Provide one or many regular expression that, if the package name matches, means we skip that package.
    /// Mutuable exclusive with `--package`
    #[structopt(short, long, parse(try_from_str = parse_regex))]
    pub skip: Vec<Regex>,
    /// Ignore version pre-releases
    ///
    /// Skip if the SemVer pre-release field is any of the listed. Mutuable exclusive with `--package`
    #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers))]
    pub ignore_version_pre: Vec<Identifier>,
    /// Ignore whether `publish` is set.
    ///
    /// If nothing else is specified `publish = true` is assumed for every package. If publish
    /// is set to false or any registry, it is ignore by default. If you want to include it
    /// regardless, set this flag.
    #[structopt(long="ignore-publish")]
    ignore_publish: bool,
}

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Deactivate the `[dev-dependencies]`
    ///
    /// Goes through the workspace and removes the `[dev-dependencies]`-section from the package
    /// manifest for all packages matching.
    DeDevDeps {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
    },
    /// calculate the packages that should be released, in the order they should be released
    ToRelease {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
    },
    /// Check whether crates can be packaged
    Check {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
    },
    /// Unleash 'em dragons 
    EmDragons {
        /// Do not disable dev-dependencies
        #[structopt(long="include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
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
    #[structopt(short="m", long, parse(from_os_str), default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,
    /// Specify the log levels
    #[structopt(long = "log", short = "l", default_value = "warn")]
    pub log: String,
    /// Show verbose cargo output
    #[structopt(long = "verbose", short = "v")]
    pub verbose: bool,

    #[structopt(subcommand)]
    pub cmd: Command,
}

fn make_pkg_predicate(args: PackageSelectOptions) -> Result<Box<dyn Fn(&Package) -> bool>, String> {
    let PackageSelectOptions { packages, skip, ignore_version_pre, ignore_publish } = args;

    if !packages.is_empty() {
        if !skip.is_empty() || !ignore_version_pre.is_empty() {
            return Err("-p/--packages is mutually exlusive to using -s/--skip and -i/--ignore-version-pre".into())
        }
    }

    let publish = move |p: &Package| ignore_publish || p.publish().as_ref().map(|v| v.is_empty()).unwrap_or(true);

    if !packages.is_empty() {
        trace!("going for matching against {:?}", packages);
        return Ok(Box::new(move |p: &Package| publish(p) && packages.contains(&p.name())));
    }

    if !skip.is_empty() || !ignore_version_pre.is_empty() {
        return Ok(Box::new(move |p: &Package| {
            if !publish(p) { return false }
            let name = p.name();
            if skip.iter().find(|r| r.is_match(&name)).is_some() {
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
        }));
    }

    Ok(Box::new(publish))
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
    
    let maybe_patch = |shouldnt_patch, predicate| {
        if shouldnt_patch { return Ok(()); }
    
        c.shell().status("Preparing", "Disabling Dev Dependencies for all crates")?;
            
        let ws = Workspace::new(&root_manifest, &c)
            .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
        commands::deactivate_dev_dependencies(ws, predicate)
    };

    match args.cmd {
        Command::DeDevDeps { pkg_opts } => {
            maybe_patch(false,  &make_pkg_predicate(pkg_opts)?)
        }
        Command::ToRelease { include_dev, pkg_opts } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev, &predicate)?;

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
        Command::Check { include_dev, pkg_opts } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev, &predicate)?;
            
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;

            commands::check(&packages, &ws)
        }
        Command::EmDragons { dry_run, token, include_dev, pkg_opts } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev,  &predicate)?;

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

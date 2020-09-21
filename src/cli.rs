use cargo::{
    core::{package::Package, Verbosity, Workspace},
    util::{config::Config as CargoConfig, interning::InternedString},
};
use flexi_logger::Logger;
use log::trace;
use regex::Regex;
use semver::{Identifier, Version};
use std::{error::Error, fs, path::PathBuf, str::FromStr};
use structopt::clap::arg_enum;
use structopt::StructOpt;
use toml_edit::Value;

use crate::commands;

fn parse_identifiers(src: &str) -> Identifier {
    Identifier::AlphaNumeric(src.to_owned())
}
fn parse_regex(src: &str) -> Result<Regex, String> {
    Regex::new(src).map_err(|e| format!("Parsing Regex failed: {:}", e))
}

arg_enum! {
    #[derive(Debug, PartialEq)]
    pub enum GenerateReadmeMode {
        // Generate Readme only if it is missing.
        IfMissing,
        // Generate Readme & append to existing file.
        Append,
        // Generate Readme & overwrite existing file.
        Overwrite,
    }
}

#[derive(StructOpt, Debug)]
pub struct PackageSelectOptions {
    /// Only use the specfic set of packages
    ///
    /// Apply only to the packages named as defined. This is mutually exclusive with skip and ignore-version-pre.
    #[structopt(short, long, parse(from_str))]
    pub packages: Vec<InternedString>,
    /// Skip the package names matching ...
    ///
    /// Provide one or many regular expression that, if the package name matches, means we skip that package.
    /// Mutually exclusive with `--package`
    #[structopt(short, long, parse(try_from_str = parse_regex))]
    pub skip: Vec<Regex>,
    /// Ignore version pre-releases
    ///
    /// Skip if the SemVer pre-release field is any of the listed. Mutually exclusive with `--package`
    #[structopt(short, long, parse(from_str = parse_identifiers))]
    pub ignore_pre_version: Vec<Identifier>,
    /// Ignore whether `publish` is set.
    ///
    /// If nothing else is specified, `publish = true` is assumed for every package. If publish
    /// is set to false or any registry, it is ignored by default. If you want to include it
    /// regardless, set this flag.
    #[structopt(long)]
    ignore_publish: bool,
    /// Automatically detect the packages, which changed compared to the given git commit.
    ///
    /// Compares the current git `head` to the reference given, identifies which files changed
    /// and attempts to identify the packages and its dependents through that mechanism. You
    /// can use any `tag`, `branch` or `commit`, but you must be sure it is available
    /// (and up to date) locally.
    #[structopt(short = "c", long="changed-since")]
    pub changed_since: String,
}

#[derive(StructOpt, Debug)]
pub enum VersionCommand {
    /// Pick pre-releases and put them to release mode.
    Release {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Smart bumping of crates for the next breaking release, bumps minor for 0.x and major for major > 1
    BumpBreaking {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Increase the pre-release suffix, keep prefix, set to `.1` if no suffix is present
    BumpPre {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Increase the patch version, unset prerelease
    BumpPatch {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Increase the minor version, unset prerelease and patch
    BumpMinor {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Increase the major version, unset prerelease, minor and patch
    BumpMajor {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Hard set version to given string
    Set {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Set to a specific Version
        version: Version,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Set the pre-release to string
    SetPre {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// The string to set the pre-release to
        #[structopt(parse(from_str = parse_identifiers))]
        pre: Identifier,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
    /// Set the metadata to string
    SetBuild {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// The specific metadata to set to
        #[structopt(parse(from_str = parse_identifiers))]
        meta: Identifier,
        /// Force an update of dependencies
        ///
        /// Hard set to the new version, do not check whether the given one still matches
        #[structopt(long)]
        force_update: bool,
    },
}

#[derive(StructOpt, Debug)]
pub enum Command {
    /// Set a field in all manifests
    ///
    /// Go through all matching crates and set the field name to value.
    /// Add the field if it doesn't exists yet.
    Set {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// The root key table to look the key up in
        #[structopt(short, long, default_value = "package")]
        root_key: String,
        /// Name of the field
        name: String,
        /// Value to set it, too
        value: String,
    },
    /// Rename a package
    ///
    /// Update the internally used references to the package by adding an `package = ` entry
    /// to the dependencies.
    Rename {
        /// Name of the field
        old_name: String,
        /// Value to set it, too
        new_name: String,
    },
    /// Messing with versioning
    ///
    /// Change versions as requested, then update all package's dependencies
    /// to ensure they are still matching
    Version {
        #[structopt(subcommand)]
        cmd: VersionCommand,
    },
    /// Add owners for a lot of crates
    ///
    ///
    AddOwner {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Owner to add to the packages
        owner: String,
        /// the crates.io token to use for API access
        ///
        /// If this is nor the environment variable are set, this falls
        /// back to the default value provided in the user directory
        #[structopt(long, env = "CRATES_TOKEN", hide_env_values = true)]
        token: Option<String>,
    },
    /// Deactivate the `[dev-dependencies]`
    ///
    /// Go through the workspace and remove the `[dev-dependencies]`-section from the package
    /// manifest for all packages matching.
    DeDevDeps {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
    },
    /// Check the package(s) for unused dependencies
    ///
    CleanDeps {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Do only check if you'd clean up.
        ///
        /// Abort if you found unused dependencies
        #[structopt(long = "check")]
        check_only: bool,
    },
    /// Calculate the packages and the order in which to release
    ///
    /// Go through the members of the workspace and calculate the dependency tree. Halt early
    /// if any circles are found
    ToRelease {
        /// Do not disable dev-dependencies
        ///
        /// By default we disable dev-dependencies before the run.
        #[structopt(long = "include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Consider no package matching the criteria an error
        #[structopt(long)]
        empty_is_failure: bool,
    },
    /// Check whether crates can be packaged
    ///
    /// Package the selected packages, then check the packages can be build with
    /// the packages as dependencies as to be released.
    Check {
        /// Do not disable dev-dependencies
        ///
        /// By default we disable dev-dependencies before the run.
        #[structopt(long = "include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Actually build the package
        ///
        /// By default, this only runs `cargo check` against the package
        /// build. Set this flag to have it run an actual `build` instead.
        #[structopt(long)]
        build: bool,
        /// Generate & verify whether the Readme file has changed.
        ///
        /// When enabled, this will generate a Readme file from
        /// the crate's doc comments (using cargo-readme), and
        /// check whether the existing Readme (if any) matches.
        #[structopt(long)]
        check_readme: bool,
        /// Consider no package matching the criteria an error
        #[structopt(long)]
        empty_is_failure: bool,
    },
    /// Generate Readme files
    ///
    /// Generate Readme files for the selected packges, based
    /// on the crates' doc comments.
    #[cfg(feature = "gen-readme")]
    GenReadme {
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Generate readme file for package.
        ///
        /// Depending on the chosen option, this will generate a Readme
        /// file from the crate's doc comments (using cargo-readme).
        #[structopt(long)]
        #[structopt(
            possible_values = &GenerateReadmeMode::variants(),
            case_insensitive = true
        )]
        readme_mode: GenerateReadmeMode,
        /// Consider no package matching the criteria an error
        #[structopt(long)]
        empty_is_failure: bool,
    },
    /// Unleash 'em dragons
    ///
    /// Package all selected crates, check them and attempt to publish them.
    EmDragons {
        /// Do not disable dev-dependencies
        ///
        /// By default we disable dev-dependencies before the run.
        #[structopt(long = "include-dev-deps")]
        include_dev: bool,
        #[structopt(flatten)]
        pkg_opts: PackageSelectOptions,
        /// Actually build the package in check
        ///
        /// By default, this only runs `cargo check` against the package
        /// build. Set this flag to have it run an actual `build` instead.
        #[structopt(long)]
        build: bool,
        /// dry run
        #[structopt(long)]
        dry_run: bool,
        /// dry run
        #[structopt(long)]
        no_check: bool,
        /// Ensure we have the owner set as well
        #[structopt(long = "owner")]
        add_owner: Option<String>,
        /// the crates.io token to use for uploading
        ///
        /// If this is nor the environment variable are set, this falls
        /// back to the default value provided in the user directory
        #[structopt(long, env = "CRATES_TOKEN", hide_env_values = true)]
        token: Option<String>,
        /// Generate & verify whether the Readme file has changed.
        ///
        /// When enabled, this will generate a Readme file from
        /// the crate's doc comments (using cargo-readme), and
        /// check whether the existing Readme (if any) matches.
        #[structopt(long)]
        check_readme: bool,
        /// Consider no package matching the criteria an error
        #[structopt(long)]
        empty_is_failure: bool,
    },
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "cargo-unleash",
    about = "Release the crates of this massiv monorepo"
)]
pub struct Opt {
    /// The path to workspace manifest
    ///
    /// Can either be the folder if the file is named `Cargo.toml` or the path
    /// to the specific `.toml`-manifest to load as the cargo workspace.
    #[structopt(short, long, parse(from_os_str), default_value = "./")]
    pub manifest_path: PathBuf,
    /// Specify the log levels.
    #[structopt(short, long, default_value = "warn")]
    pub log: String,
    /// Show verbose cargo output
    #[structopt(short, long)]
    pub verbose: bool,

    #[structopt(subcommand)]
    pub cmd: Command,
}

fn make_pkg_predicate(args: PackageSelectOptions) -> Result<impl Fn(&Package) -> bool, String> {
    let PackageSelectOptions {
        packages,
        skip,
        ignore_pre_version,
        ignore_publish,
        changed_since,
    } = args;

    if !packages.is_empty() {
        if !skip.is_empty() || !ignore_pre_version.is_empty() {
            return Err(
                "-p/--packages is mutually exlusive to using -s/--skip and -i/--ignore-version-pre"
                    .into(),
            );
        }
        if changed_since.len() != 0 {
            return Err(
                "-p/--packages is mutually exlusive to using -c/--changed-since"
                    .into(),
            );
        }
    }

    if changed_since.len() != 0 {
        if !skip.is_empty() || !ignore_pre_version.is_empty() {
            return Err(
                "-c/--changed-since is mutually exlusive to using -s/--skip and -i/--ignore-version-pre"
                    .into(),
            );
        }
    }


    let publish = move |p: &Package| {
        // If publish is set to false or any registry, it is ignored by default
        // unless overriden.
        let value = ignore_publish || p.publish().is_none();

        trace!("{:}.publish={}", p.name(), value);
        value
    };

    Ok(move |p: &Package| {
        if !publish(p) {
            return false;
        }

        if !packages.is_empty() {
            trace!("going for matching against {:?}", packages);
            return packages.contains(&p.name());
        }

        if !skip.is_empty() || !ignore_pre_version.is_empty() {
            let name = p.name();
            if skip.iter().any(|r| r.is_match(&name)) {
                return false;
            }
            if p.version().is_prerelease() {
                for pre in &p.version().pre {
                    if ignore_pre_version.contains(&pre) {
                        return false;
                    }
                }
            }
        }

        true
    })
}

fn verify_readme_feature() -> Result<(), String> {
    if cfg!(feature = "gen-readme") {
        Ok(())
    } else {
        Err("Readme related functionalities not available. Please re-install with gen-readme feature.".to_owned())
    }
}

pub fn run(args: Opt) -> Result<(), Box<dyn Error>> {
    let _ = Logger::with_str(args.log.clone()).start();
    let mut c = CargoConfig::default().expect("Couldn't create cargo config");
    c.values()?;
    c.load_credentials()?;

    let get_token = |t| -> Result<Option<String>, Box<dyn Error>> {
        Ok(match t {
            None => c.get_string("registry.token")?.map(|x| x.val),
            _ => t,
        })
    };

    c.shell().set_verbosity(if args.verbose {
        Verbosity::Verbose
    } else {
        Verbosity::Normal
    });

    let root_manifest = {
        let mut path = args.manifest_path.clone();
        if path.is_dir() {
            path = path.join("Cargo.toml")
        }
        fs::canonicalize(path)?
    };

    let maybe_patch = |shouldnt_patch, predicate: &dyn Fn(&Package) -> bool| {
        if shouldnt_patch {
            return Ok(());
        }

        c.shell()
            .status("Preparing", "Disabling Dev Dependencies")?;

        let ws = Workspace::new(&root_manifest, &c)
            .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
        commands::deactivate_dev_dependencies(
            ws.members()
                .filter(|p| predicate(p) && c.shell().status("Patching", p.name()).is_ok()),
        )
    };

    match args.cmd {
        Command::CleanDeps {
            pkg_opts,
            check_only,
        } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            commands::clean_up_unused_dependencies(&ws, predicate, check_only)
        }
        Command::AddOwner {
            owner,
            token,
            pkg_opts,
        } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let t = get_token(token)?;
            for pkg in ws.members().filter(|p| predicate(p)) {
                commands::add_owner(ws.config(), &pkg, owner.clone(), t.clone())?;
            }
            Ok(())
        }
        Command::Set {
            root_key,
            name,
            value,
            pkg_opts,
        } => {
            if name == "name" {
                return Err("To change the name please use the rename command!".into());
            }
            let predicate = make_pkg_predicate(pkg_opts)?;
            let type_value = {
                if let Ok(v) = bool::from_str(&value) {
                    Value::from(v)
                } else if let Ok(v) = i64::from_str(&value) {
                    Value::from(v)
                } else {
                    Value::from(value)
                }
            };

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            commands::set_field(
                ws.members()
                    .filter(|p| predicate(p) && c.shell().status("Setting on", p.name()).is_ok()),
                root_key,
                name,
                type_value,
            )
        }
        Command::Rename { old_name, new_name } => {
            let predicate = |p: &Package| p.name().to_string().trim() == old_name;
            let renamer = |_p: &Package| Some(new_name.clone());

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            commands::rename(&ws, predicate, renamer)
        }
        Command::Version { cmd } => {
            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            match cmd {
                VersionCommand::Set {
                    pkg_opts,
                    force_update,
                    version,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |_| Some(version.clone()),
                        force_update,
                    )
                }
                VersionCommand::BumpPre {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            if v.pre.is_empty() {
                                v.pre = vec![Identifier::Numeric(1)]
                            } else {
                                match v.pre.pop() {
                                    Some(Identifier::Numeric(num)) => {
                                        v.pre.push(Identifier::Numeric(num + 1))
                                    }
                                    Some(Identifier::AlphaNumeric(pre)) => {
                                        v.pre.push(Identifier::AlphaNumeric(pre));
                                        v.pre.push(Identifier::Numeric(1));
                                    }
                                    _ => unreachable!("There is a last item"),
                                }
                            }
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::BumpPatch {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = Vec::new();
                            v.increment_patch();
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::BumpMinor {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = Vec::new();
                            v.increment_minor();
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::BumpMajor {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = Vec::new();
                            v.increment_major();
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::BumpBreaking {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = Vec::new();
                            if v.major != 0 {
                                v.increment_major();
                            } else if v.minor != 0 {
                                v.increment_minor();
                            } else {
                                // 0.0.x means each patch is breaking, see:
                                // https://doc.rust-lang.org/cargo/reference/semver.html#change-categories

                                v.patch += 1;
                                // no helper, have to reset the metadata ourselves
                                v.build = Vec::new();
                                v.pre = Vec::new();
                            }
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::SetPre {
                    pre,
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = vec![pre.clone()];
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::SetBuild {
                    meta,
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.build = vec![meta.clone()];
                            Some(v)
                        },
                        force_update,
                    )
                }
                VersionCommand::Release {
                    pkg_opts,
                    force_update,
                } => {
                    let predicate = make_pkg_predicate(pkg_opts)?;
                    commands::set_version(
                        &ws,
                        |p| predicate(p),
                        |p| {
                            let mut v = p.version().clone();
                            v.pre = vec![];
                            v.build = vec![];
                            Some(v)
                        },
                        force_update,
                    )
                }
            }
        }
        Command::DeDevDeps { pkg_opts } => maybe_patch(false, &make_pkg_predicate(pkg_opts)?),
        Command::ToRelease {
            include_dev,
            pkg_opts,
            empty_is_failure,
        } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev, &predicate)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;
            if packages.is_empty() {
                if empty_is_failure {
                    return Err("No Packages matching criteria. Exiting".into());
                } else {
                    println!("No packages selected. All good. Exiting.");
                    return Ok(());
                }
            }
            println!(
                "{:}",
                packages
                    .iter()
                    .map(|p| format!("{} ({})", p.name(), p.version()))
                    .collect::<Vec<String>>()
                    .join(", ")
            );

            Ok(())
        }
        Command::Check {
            include_dev,
            build,
            pkg_opts,
            check_readme,
            empty_is_failure,
        } => {
            if check_readme {
                verify_readme_feature()?;
            }

            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev, &predicate)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;
            if packages.is_empty() {
                if empty_is_failure {
                    return Err("No Packages matching criteria. Exiting".into());
                } else {
                    println!("No packages selected. All good. Exiting.");
                    return Ok(());
                }
            }

            commands::check(&packages, &ws, build, check_readme)
        }
        #[cfg(feature = "gen-readme")]
        Command::GenReadme {
            pkg_opts,
            readme_mode,
            empty_is_failure,
        } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(false, &predicate)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;
            let packages = commands::packages_to_release(&ws, predicate)?;
            if packages.is_empty() {
                if empty_is_failure {
                    return Err("No Packages matching criteria. Exiting".into());
                } else {
                    println!("No packages selected. All good. Exiting.");
                    return Ok(());
                }
            }

            commands::gen_all_readme(packages, &ws, readme_mode)
        }
        Command::EmDragons {
            dry_run,
            no_check,
            token,
            include_dev,
            add_owner,
            build,
            pkg_opts,
            check_readme,
            empty_is_failure,
        } => {
            let predicate = make_pkg_predicate(pkg_opts)?;
            maybe_patch(include_dev, &predicate)?;

            let ws = Workspace::new(&root_manifest, &c)
                .map_err(|e| format!("Reading workspace {:?} failed: {:}", root_manifest, e))?;

            let packages = commands::packages_to_release(&ws, predicate)?;
            if packages.is_empty() {
                if empty_is_failure {
                    return Err("No Packages matching criteria. Exiting".into());
                } else {
                    println!("No packages selected. All good. Exiting.");
                    return Ok(());
                }
            }

            if !no_check {
                if check_readme {
                    verify_readme_feature()?;
                }

                commands::check(&packages, &ws, build, check_readme)?;
            }

            ws.config().shell().status(
                "Releasing",
                &packages
                    .iter()
                    .map(|p| format!("{} ({})", p.name(), p.version()))
                    .collect::<Vec<String>>()
                    .join(", "),
            )?;

            commands::release(packages, ws, dry_run, get_token(token)?, add_owner)
        }
    }
}

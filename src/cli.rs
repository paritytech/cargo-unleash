use anyhow::Context;
use cargo::{
	core::{package::Package, Verbosity, Workspace},
	util::{config::Config as CargoConfig, interning::InternedString},
};
use flexi_logger::Logger;
use log::trace;
use regex::Regex;
use semver::{BuildMetadata, Prerelease, Version};
use std::{fs, path::PathBuf, str::FromStr};
use structopt::{
	clap::{arg_enum, AppSettings::*},
	StructOpt,
};
use toml_edit::Value;

use crate::{commands, util};

fn parse_regex(src: &str) -> Result<Regex, anyhow::Error> {
	Regex::new(src).context("Parsing Regex failed")
}

arg_enum! {
	#[derive(Debug, PartialEq, Eq)]
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
#[structopt(setting(ColorAuto), setting(ColoredHelp))]
pub struct PackageSelectOptions {
	/// Only use the specfic set of packages
	///
	/// Apply only to the packages named as defined. This is mutually exclusive with skip and
	/// ignore-version-pre.
	#[structopt(short, long, parse(from_str))]
	pub packages: Vec<InternedString>,
	/// Skip the package names matching ...
	///
	/// Provide one or many regular expression that, if the package name matches, means we skip
	/// that package. Mutually exclusive with `--package`
	#[structopt(short, long, parse(try_from_str = parse_regex))]
	pub skip: Vec<Regex>,
	/// Ignore version pre-releases
	///
	/// Skip if the SemVer pre-release field is any of the listed. Mutually exclusive with
	/// `--package`
	#[structopt(short, long)]
	pub ignore_pre_version: Vec<String>,
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
	#[structopt(short = "c", long = "changed-since")]
	pub changed_since: Option<String>,
	/// Even if not selected by default, also include depedencies with a pre (cascading)
	#[structopt(long)]
	pub include_pre_deps: bool,
}

#[derive(StructOpt, Debug)]
#[structopt(setting(ColorAuto), setting(ColoredHelp))]
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
	/// Smart bumping of crates for the next breaking release, bumps minor for 0.x and major for
	/// major > 1
	BumpBreaking {
		#[structopt(flatten)]
		pkg_opts: PackageSelectOptions,
		/// Force an update of dependencies
		///
		/// Hard set to the new version, do not check whether the given one still matches
		#[structopt(long)]
		force_update: bool,
	},
	/// Smart bumping of crates for the next breaking release and add a `-dev`-pre-release-tag
	BumpToDev {
		#[structopt(flatten)]
		pkg_opts: PackageSelectOptions,
		/// Force an update of dependencies
		///
		/// Hard set to the new version, do not check whether the given one still matches
		#[structopt(long)]
		force_update: bool,
		/// Use this identifier instead of `dev`  for the pre-release
		#[structopt()]
		pre_tag: Option<String>,
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
		#[structopt()]
		pre: String,
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
		#[structopt()]
		meta: String,
		/// Force an update of dependencies
		///
		/// Hard set to the new version, do not check whether the given one still matches
		#[structopt(long)]
		force_update: bool,
	},
}

#[derive(StructOpt, Debug)]
#[structopt(setting(ColorAuto), setting(ColoredHelp))]
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

		/// Write a graphviz dot of all crates to be release and their depedency relation
		/// to the given path.
		#[structopt(long = "dot-graph")]
		dot_graph: Option<PathBuf>,
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

		/// Write a graphviz dot file to the given destination
		#[structopt(long = "dot-graph")]
		dot_graph: Option<PathBuf>,
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

		/// Write a graphviz dot file to the given destination
		#[structopt(long = "dot-graph")]
		dot_graph: Option<PathBuf>,
	},
}

#[derive(Debug, StructOpt)]
#[structopt(name = "cargo-unleash", about = "Release the crates of this massiv monorepo")]
#[structopt(setting(ColorAuto), setting(ColoredHelp))]
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

fn make_pkg_predicate(
	ws: &Workspace<'_>,
	args: PackageSelectOptions,
) -> Result<impl Fn(&Package) -> bool, anyhow::Error> {
	let PackageSelectOptions {
		packages,
		skip,
		ignore_pre_version,
		ignore_publish,
		changed_since,
		include_pre_deps,
	} = args;

	if !packages.is_empty() {
		if !skip.is_empty() || !ignore_pre_version.is_empty() {
			anyhow::bail!(
				"-p/--packages is mutually exlusive to using -s/--skip and -i/--ignore-version-pre"
			);
		}
		if changed_since.is_some() {
			anyhow::bail!("-p/--packages is mutually exlusive to using -c/--changed-since");
		}
	}

	let publish = move |p: &Package| {
		// If publish is set to false or any registry, it is ignored by default
		// unless overriden.
		let value = ignore_publish || p.publish().is_none();

		trace!("{:}.publish={}", p.name(), value);
		value
	};
	let check_version = move |p: &Package| return include_pre_deps && !p.version().pre.is_empty();

	let changed = if let Some(changed_since) = &changed_since {
		if !skip.is_empty() || !ignore_pre_version.is_empty() {
			anyhow::bail!("-c/--changed-since is mutually exlusive to using -s/--skip and -i/--ignore-version-pre",);
		}
		Some(util::changed_packages(ws, changed_since)?)
	} else {
		None
	};

	Ok(move |p: &Package| {
		if !publish(p) {
			return false
		}

		if let Some(changed) = &changed {
			return changed.contains(p) || check_version(p)
		}

		if !packages.is_empty() {
			trace!("going for matching against {:?}", packages);
			return packages.contains(&p.name()) || check_version(p)
		}

		if !skip.is_empty() || !ignore_pre_version.is_empty() {
			let name = p.name();
			if skip.iter().any(|r| r.is_match(&name)) {
				return false
			}
			if !p.version().pre.is_empty() &&
				ignore_pre_version.contains(&p.version().pre.as_str().to_owned())
			{
				return false
			}
		}

		true
	})
}

fn verify_readme_feature() -> Result<(), anyhow::Error> {
	if cfg!(feature = "gen-readme") {
		Ok(())
	} else {
		anyhow::bail!("Readme related functionalities not available. Please re-install with gen-readme feature.")
	}
}

pub fn run(args: Opt) -> Result<(), anyhow::Error> {
	let _ = Logger::try_with_str(args.log.clone())?.start()?;
	let mut c = CargoConfig::default().expect("Couldn't create cargo config");
	c.values()?;
	c.load_credentials()?;

	let get_token = |t| -> Result<Option<String>, anyhow::Error> {
		Ok(match t {
			None => c.get_string("registry.token")?.map(|x| x.val),
			_ => t,
		})
	};

	c.shell()
		.set_verbosity(if args.verbose { Verbosity::Verbose } else { Verbosity::Normal });

	let root_manifest = {
		let mut path = args.manifest_path.clone();
		if path.is_dir() {
			path = path.join("Cargo.toml")
		}
		fs::canonicalize(path)?
	};

	let ws = Workspace::new(&root_manifest, &c).context("Reading workspace failed")?;

	let maybe_patch =
		|ws, shouldnt_patch, predicate: &dyn Fn(&Package) -> bool| -> anyhow::Result<Workspace> {
			if shouldnt_patch {
				return Ok(ws)
			}

			c.shell().status("Preparing", "Disabling Dev Dependencies")?;

			commands::deactivate_dev_dependencies(
				ws.members()
					.filter(|p| predicate(p) && c.shell().status("Patching", p.name()).is_ok()),
			)?;
			// assure to re-read the workspace, otherwise `fn to_release` will still find cycles
			// (rightfully so!)
			Workspace::new(&root_manifest, &c).context("Reading workspace failed")
		};

	match args.cmd {
		Command::CleanDeps { pkg_opts, check_only } => {
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			commands::clean_up_unused_dependencies(&ws, predicate, check_only)
		},
		Command::AddOwner { owner, token, pkg_opts } => {
			let t = get_token(token)?;
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;

			for pkg in ws.members().filter(|p| predicate(p)) {
				commands::add_owner(ws.config(), pkg, owner.clone(), t.clone())?;
			}
			Ok(())
		},
		Command::Set { root_key, name, value, pkg_opts } => {
			if name == "name" {
				anyhow::bail!("To change the name please use the rename command!");
			}
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let type_value = {
				if let Ok(v) = bool::from_str(&value) {
					Value::from(v)
				} else if let Ok(v) = i64::from_str(&value) {
					Value::from(v)
				} else {
					Value::from(value)
				}
			};

			commands::set_field(
				ws.members()
					.filter(|p| predicate(p) && c.shell().status("Setting on", p.name()).is_ok()),
				root_key,
				name,
				type_value,
			)
		},
		Command::Rename { old_name, new_name } => {
			let predicate = |p: &Package| p.name().to_string().trim() == old_name;
			let renamer = |_p: &Package| Some(new_name.clone());

			commands::rename(&ws, predicate, renamer)
		},
		Command::Version { cmd } => {
			match cmd {
				VersionCommand::Set { pkg_opts, force_update, version } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|_| Some(version.clone()),
						force_update,
					)
				},
				VersionCommand::BumpPre { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							if v.pre.is_empty() {
								v.pre = Prerelease::new("1").expect("Static will work");
							} else if let Ok(num) = v.pre.as_str().parse::<u32>() {
								v.pre = Prerelease::new(&format!("{}", num + 1))
									.expect("Knwon to work");
							} else {
								let mut items = v
									.pre
									.as_str()
									.split('.')
									.map(|s| s.to_string())
									.collect::<Vec<_>>();
								if let Some(num) = items.last().and_then(|u| u.parse::<u32>().ok())
								{
									let _ = items.pop();
									items.push(format!("{}", num + 1));
								} else {
									items.push("1".to_owned());
								}
								if let Ok(pre) = Prerelease::new(&items.join(".")) {
									v.pre = pre;
								} else {
									return None
								}
							}
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::BumpPatch { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre = Prerelease::EMPTY;
							v.patch += 1;
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::BumpMinor { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre = Prerelease::EMPTY;
							v.minor += 1;
							v.patch = 0;
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::BumpMajor { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre = Prerelease::EMPTY;
							v.major += 1;
							v.minor = 0;
							v.patch = 0;
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::BumpBreaking { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre = Prerelease::EMPTY;
							if v.major != 0 {
								v.major += 1;
								v.minor = 0;
								v.patch = 0;
							} else if v.minor != 0 {
								v.minor += 1;
								v.patch = 0;
							} else {
								// 0.0.x means each patch is breaking, see:
								// https://doc.rust-lang.org/cargo/reference/semver.html#change-categories

								v.patch += 1;
								// no helper, have to reset the metadata ourselves
								v.build = BuildMetadata::EMPTY;
							}
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::BumpToDev { pkg_opts, force_update, pre_tag } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					let pre_val = pre_tag.unwrap_or_else(|| "dev".to_owned());
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							if v.major != 0 {
								v.major += 1;
								v.minor = 0;
								v.patch = 0
							} else if v.minor != 0 {
								v.minor += 1;
								v.patch = 0;
							} else {
								// 0.0.x means each patch is breaking, see:
								// https://doc.rust-lang.org/cargo/reference/semver.html#change-categories

								v.patch += 1;
								// no helper, have to reset the metadata ourselves
								v.build = BuildMetadata::EMPTY;
							}
							// force the pre
							v.pre = Prerelease::new(&pre_val.clone())
								.expect("Static or expected to work");
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::SetPre { pre, pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre =
								Prerelease::new(&pre.clone()).expect("Static or expected to work");
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::SetBuild { meta, pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.build = BuildMetadata::new(&meta.clone())
								.expect("The meta you provided couldn't be parsed");
							Some(v)
						},
						force_update,
					)
				},
				VersionCommand::Release { pkg_opts, force_update } => {
					let predicate = make_pkg_predicate(&ws, pkg_opts)?;
					commands::set_version(
						&ws,
						|p| predicate(p),
						|p| {
							let mut v = p.version().clone();
							v.pre = Prerelease::EMPTY;
							v.build = BuildMetadata::EMPTY;
							Some(v)
						},
						force_update,
					)
				},
			}
		},
		Command::DeDevDeps { pkg_opts } => {
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let _ = maybe_patch(ws, false, &predicate)?;
			Ok(())
		},
		Command::ToRelease { include_dev, pkg_opts, empty_is_failure, dot_graph } => {
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let ws = maybe_patch(ws, include_dev, &predicate)?;

			let packages = commands::packages_to_release(&ws, predicate, dot_graph)?;
			if packages.is_empty() {
				if empty_is_failure {
					anyhow::bail!("No Packages matching criteria. Exiting");
				} else {
					println!("No packages selected. All good. Exiting.");
					return Ok(())
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
		},
		Command::Check {
			include_dev,
			build,
			pkg_opts,
			check_readme,
			empty_is_failure,
			dot_graph,
		} => {
			if check_readme {
				verify_readme_feature()?;
			}

			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let ws = maybe_patch(ws, include_dev, &predicate)?;

			let packages = commands::packages_to_release(&ws, predicate, dot_graph)?;
			if packages.is_empty() {
				if empty_is_failure {
					anyhow::bail!("No Packages matching criteria. Exiting");
				} else {
					println!("No packages selected. All good. Exiting.");
					return Ok(())
				}
			}

			commands::check(&packages, &ws, build, check_readme)
		},
		#[cfg(feature = "gen-readme")]
		Command::GenReadme { pkg_opts, readme_mode, empty_is_failure } => {
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let ws = maybe_patch(ws, false, &predicate)?;

			let packages = commands::packages_to_release(&ws, predicate, None)?;
			if packages.is_empty() {
				if empty_is_failure {
					anyhow::bail!("No Packages matching criteria. Exiting");
				} else {
					println!("No packages selected. All good. Exiting.");
					return Ok(())
				}
			}

			commands::gen_all_readme(packages, &ws, readme_mode)
		},
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
			dot_graph,
		} => {
			let predicate = make_pkg_predicate(&ws, pkg_opts)?;
			let ws = maybe_patch(ws, include_dev, &predicate)?;

			let packages = commands::packages_to_release(&ws, predicate, dot_graph)?;
			if packages.is_empty() {
				if empty_is_failure {
					anyhow::bail!("No Packages matching criteria. Exiting");
				} else {
					println!("No packages selected. All good. Exiting.");
					return Ok(())
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
		},
	}
}

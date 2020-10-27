#[cfg(feature = "gen-readme")]
use crate::commands::readme;

use crate::util::{edit_each_dep, DependencyAction, DependencyEntry};
use cargo::{
    core::{
        compiler::{BuildConfig, CompileMode, DefaultExecutor, Executor},
        manifest::ManifestMetadata,
        package::Package,
        Feature, SourceId, Workspace,
    },
    ops::{self, package, PackageOpts},
    sources::PathSource,
    util::{paths, FileLock},
};
use flate2::read::GzDecoder;
use log::error;
use semver::VersionReq;
use std::{
    collections::HashMap,
    error::Error,
    fs::{read_to_string, write},
    sync::Arc,
};
use tar::Archive;
use toml_edit::{decorated, Document, Item, Value};

fn inject_replacement(
    pkg: &Package,
    replace: &HashMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let manifest = pkg.manifest_path();

    let document = read_to_string(manifest)?;
    let mut document = document.parse::<Document>()?;
    let root = document.as_table_mut();

    edit_each_dep(root, |name, _, entry| {
        if let Some(p) = replace.get(&name) {
            let path = decorated(Value::from(p.clone()), " ", " ");
            match entry {
                DependencyEntry::Inline(info) => {
                    info.get_or_insert("path", path);
                }
                DependencyEntry::Table(info) => {
                    info["path"] = Item::Value(path);
                }
            }
            DependencyAction::Mutated
        } else {
            DependencyAction::Untouched
        }
    });
    write(manifest, document.to_string().as_bytes())
        .map_err(|e| format!("Could not write local manifest: {}", e).into())
}

fn run_check(
    ws: &Workspace<'_>,
    tar: &FileLock,
    opts: &PackageOpts<'_>,
    build_mode: CompileMode,
    replace: &HashMap<String, String>,
) -> Result<(), Box<dyn Error>> {
    let config = ws.config();
    let pkg = ws.current()?;

    let f = GzDecoder::new(tar.file());
    let dst = tar
        .parent()
        .join(&format!("{}-{}", pkg.name(), pkg.version()));
    if dst.exists() {
        paths::remove_dir_all(&dst)?;
    }
    let mut archive = Archive::new(f);
    // We don't need to set the Modified Time, as it's not relevant to verification
    // and it errors on filesystems that don't support setting a modified timestamp
    archive.set_preserve_mtime(false);
    archive.unpack(dst.parent().unwrap())?;

    // Manufacture an ephemeral workspace to ensure that even if the top-level
    // package has a workspace we can still build our new crate.
    let (src, new_pkg) = {
        let id = SourceId::for_path(&dst)?;
        let mut src = PathSource::new(&dst, id, ws.config());
        let new_pkg = src.root_package()?;

        // inject our local builds
        inject_replacement(&new_pkg, replace)?;

        // parse the manifest again
        let mut src = PathSource::new(&dst, id, ws.config());
        let new_pkg = src.root_package()?;
        (src, new_pkg)
    };

    let pkg_fingerprint = src.last_modified_file(&new_pkg)?;
    let ws = Workspace::ephemeral(new_pkg, config, None, true)?;

    let rustc_args = if pkg
        .manifest()
        .features()
        .require(Feature::public_dependency())
        .is_ok()
    {
        // FIXME: Turn this on at some point in the future
        //Some(vec!["-D exported_private_dependencies".to_string()])
        Some(vec![])
    } else {
        None
    };

    let exec: Arc<dyn Executor> = Arc::new(DefaultExecutor);
    ops::compile_with_exec(
        &ws,
        &ops::CompileOptions {
            build_config: BuildConfig::new(config, opts.jobs, &opts.targets, build_mode)?,
            features: opts.features.clone(),
            no_default_features: opts.no_default_features,
            all_features: opts.all_features,
            spec: ops::Packages::Packages(Vec::new()),
            filter: ops::CompileFilter::Default {
                required_features_filterable: true,
            },
            target_rustdoc_args: None,
            target_rustc_args: rustc_args,
            local_rustdoc_args: None,
            rustdoc_document_private_items: false,
        },
        &exec,
    )?;

    // Check that `build.rs` didn't modify any files in the `src` directory.
    let ws_fingerprint = src.last_modified_file(ws.current()?)?;
    if pkg_fingerprint != ws_fingerprint {
        let (_, path) = ws_fingerprint;
        return Err(format!(
            "Source directory was modified by build.rs during cargo publish. \
             Build scripts should not modify anything outside of OUT_DIR.\n\
             {:?}\n\n\
             To proceed despite this, pass the `--no-verify` flag.",
            path
        )
        .into());
    }

    Ok(())
}

fn check_dependencies(package: &Package) -> Result<(), String> {
    let git_deps = package
        .dependencies()
        .iter()
        .filter(|d| d.source_id().is_git() && d.version_req() == &VersionReq::any())
        .map(|d| format!("{:}", d.package_name()))
        .collect::<Vec<_>>();
    if git_deps.len() > 0 {
        Err(git_deps.join(", "))
    } else {
        Ok(())
    }
}

// ensure metadata is set
// https://doc.rust-lang.org/cargo/reference/publishing.html#before-publishing-a-new-crate
fn check_metadata(metadata: &ManifestMetadata) -> Result<(), String> {
    let mut bad_fields = Vec::new();
    if metadata.authors.is_empty() {
        bad_fields.push("authors is empty")
    }
    match metadata.description.as_deref() {
        Some("") => bad_fields.push("description is empty"),
        None => bad_fields.push("description is missing"),
        _ => {}
    }
    match metadata.repository.as_deref() {
        Some("") => bad_fields.push("repository is empty"),
        None => bad_fields.push("repository is missing"),
        _ => {}
    }
    match (metadata.license.as_ref(), metadata.license_file.as_ref()) {
        (Some(ref s), None) | (None, Some(ref s)) if s.len() > 0 => {}
        (Some(_), Some(_)) => bad_fields.push("You can't have license AND license_file"),
        _ => bad_fields.push("Neither license nor license_file is provided"),
    }

    if bad_fields.is_empty() {
        Ok(())
    } else {
        Err(bad_fields.join("; "))
    }
}

#[cfg(feature = "gen-readme")]
fn check_readme<'a>(ws: &Workspace<'a>, pkg: &Package) -> Result<(), String> {
    let pkg_path = pkg.manifest_path().parent().expect("Folder exists");
    readme::check_pkg_readme(ws, pkg_path, pkg.manifest())
}

#[cfg(not(feature = "gen-readme"))]
fn check_readme<'a>(_ws: &Workspace<'a>, _pkg: &Package) -> Result<(), String> {
    unreachable!()
}

pub fn check<'a>(
    packages: &[Package],
    ws: &Workspace<'a>,
    build: bool,
    check_readme: bool,
) -> Result<(), Box<dyn Error>> {
    let c = ws.config();
    let replaces = packages
        .iter()
        .map(|pkg| {
            (
                pkg.name().as_str().to_owned(),
                pkg.manifest_path()
                    .parent()
                    .expect("Folder exists")
                    .to_str()
                    .expect("Is stringifiable")
                    .to_owned(),
            )
        })
        .collect::<HashMap<_, _>>();

    // FIXME: make build config configurable
    //        https://github.com/paritytech/cargo-unleash/issues/20
    let opts = PackageOpts {
        config: c,
        verify: false,
        check_metadata: true,
        list: false,
        allow_dirty: true,
        all_features: false,
        no_default_features: false,
        jobs: None,
        targets: Default::default(),
        features: Default::default(),
    };

    c.shell().status("Checking", "Metadata & Dependencies")?;

    let errors = packages.iter().fold(Vec::new(), |mut res, pkg| {
        if let Err(e) = check_metadata(pkg.manifest().metadata()) {
            res.push(format!("{:}: Bad metadata: {:}", pkg.name(), e));
        }
        if let Err(e) = check_dependencies(pkg) {
            res.push(format!(
                "{:}: has dependencies defined as git without a version: {:}",
                pkg.name(),
                e
            ));
        }
        res
    });

    errors.iter().for_each(|s| error!("{:#?}", s));
    if errors.len() > 0 {
        return Err(format!(
            "Soft checkes failed with {} errors (see above)",
            errors.len()
        )
        .into());
    }

    if check_readme {
        c.shell().status("Checking", "Readme files")?;
        let errors = packages.iter().fold(Vec::new(), |mut res, pkg| {
            if let Err(e) = self::check_readme(&ws, &pkg) {
                res.push(format!(
                    "{:}: Checking Readme file failed with: {:}",
                    pkg.name(),
                    e
                ));
            }
            res
        });

        errors.iter().for_each(|s| error!("{:#?}", s));
        if errors.len() > 0 {
            return Err(format!(
                "{} readme file(s) need to be updated (see above).",
                errors.len()
            )
            .into());
        }
    }

    let builds = packages.iter().map(|pkg| {
        check_metadata(pkg.manifest().metadata())
            .map_err(|e| format!("{:}: Bad metadata: {:}", pkg.name(), e))?;

        let pkg_ws = Workspace::ephemeral(pkg.clone(), c, Some(ws.target_dir()), true)
            .map_err(|e| format!("{:}", e))?;
        c.shell()
            .status("Packing", &pkg)
            .map_err(|e| format!("{:}", e))?;
        match package(&pkg_ws, &opts) {
            Ok(Some(rw_lock)) => Ok((pkg_ws, rw_lock)),
            Ok(None) => Err(format!("Failure packing {:}", pkg.name())),
            Err(e) => Err(format!("Failure packing {:}: {}", pkg.name(), e)),
        }
    });

    let (errors, successes): (Vec<_>, Vec<_>) = builds.partition(Result::is_err);

    for e in errors.iter().filter_map(|res| res.as_ref().err()) {
        error!("{:#?}", e);
    }
    if errors.len() > 0 {
        return Err(format!("Packing failed with {} errors (see above)", errors.len()).into());
    };

    let build_mode = if build {
        CompileMode::Build
    } else {
        CompileMode::Check { test: false }
    };

    c.shell().status("Checking", "Packages")?;
    for (pkg_ws, rw_lock) in successes.iter().filter_map(|e| e.as_ref().ok()) {
        c.shell().status(
            "Verfying",
            pkg_ws
                .current()
                .expect("We've build localised workspaces. qed"),
        )?;
        run_check(&pkg_ws, &rw_lock, &opts, build_mode, &replaces)?;
    }
    Ok(())
}

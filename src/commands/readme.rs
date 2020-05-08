use crate::cli::GenerateReadmeMode;
use cargo::core::{Manifest, Package, Workspace};
use sha1::Sha1;
use std::{
    error::Error,
    fmt::Display,
    fs::{self, File},
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum CheckReadmeResult {
    Skipped,
    Missing,
    UpdateNeeded,
    UpToDate,
}

impl Display for CheckReadmeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Skipped => "Skipped",
                Self::Missing => "Missing",
                Self::UpdateNeeded => "Updated needed",
                Self::UpToDate => "Up-to-date",
            }
        )
    }
}

pub fn check_pkg_readme<'a>(
    ws: &Workspace<'a>,
    pkg_path: &Path,
    pkg_manifest: &Manifest,
) -> Result<(), String> {
    let c = ws.config();

    let mut pkg_source = find_entrypoint(pkg_path, pkg_manifest)?;
    let readme_path = pkg_path.join("README.md");

    c.shell()
        .status("Checking", format!("Readme for {}", &pkg_manifest.name()))
        .map_err(|e| format!("{:}", e))?;

    let pkg_readme = fs::read_to_string(readme_path.clone());
    match pkg_readme {
        Ok(pkg_readme) => {
            // Try to find readme template
            let template_path = find_readme_template(&ws.root(), &pkg_path)?;

            let new_readme = generate_readme(&pkg_path, &mut pkg_source, template_path)?;
            if Sha1::from(pkg_readme) == Sha1::from(new_readme) {
                Ok(())
            } else {
                Err(CheckReadmeResult::UpdateNeeded.to_string())
            }
        }
        Err(_err) => Err(CheckReadmeResult::Missing.to_string()),
    }
}

pub fn gen_all_readme<'a>(
    packages: &Vec<Package>,
    ws: &Workspace<'a>,
    readme_mode: GenerateReadmeMode,
) -> Result<(), Box<dyn Error>> {
    let c = ws.config();

    c.shell().status("Generating", "Readme files")?;
    for pkg in packages.iter() {
        let pkg_path = pkg.manifest_path().parent().expect("Folder exists");
        gen_pkg_readme(ws, &pkg_path, &pkg.manifest(), &readme_mode)
            .map_err(|e| format!("Failure generating Readme for {:}: {}", pkg.name(), e))?
    }

    Ok(())
}

pub fn gen_pkg_readme<'a>(
    ws: &Workspace<'a>,
    pkg_path: &Path,
    pkg_manifest: &Manifest,
    mode: &GenerateReadmeMode,
) -> Result<(), String> {
    let c = ws.config();
    let root_path = ws.root();

    let mut pkg_source = find_entrypoint(pkg_path, pkg_manifest)?;
    let readme_path = pkg_path.join("README.md");

    let pkg_readme = fs::read_to_string(readme_path.clone());
    match (mode, pkg_readme) {
        (GenerateReadmeMode::IfMissing, Ok(_existing_readme)) => {
            c.shell()
                .status(
                    "Skipping",
                    format!("{}: Readme already exists.", &pkg_manifest.name()),
                )
                .map_err(|e| format!("{:}", e))?;

            Ok(())
        }
        (mode, existing_res) => {
            let template_path = find_readme_template(&ws.root(), &pkg_path)?;
            c.shell()
                .status(
                    "Generating",
                    format!(
                        "Readme for {} (template: {:?})",
                        &pkg_manifest.name(),
                        match &template_path {
                            Some(p) => p.strip_prefix(&root_path).unwrap().to_str().unwrap(),
                            None => "none found",
                        }
                    ),
                )
                .map_err(|e| format!("{:}", e))?;
            let new_readme = &mut generate_readme(&pkg_path, &mut pkg_source, template_path)?;
            if mode == &GenerateReadmeMode::Append && existing_res.is_ok() {
                *new_readme = format!("{}\n{}", existing_res.unwrap(), new_readme);
            }
            fs::write(readme_path, new_readme.as_bytes()).map_err(|e| format!("{:}", e))
        }
    }
}

fn generate_readme<'a>(
    pkg_path: &Path,
    pkg_source: &mut File,
    template_path: Option<PathBuf>,
) -> Result<String, String> {
    let mut template = template_path
        .map(|p| fs::File::open(&p).expect(&format!("Could not read template at {}", p.display())));

    cargo_readme::generate_readme(
        pkg_path,
        pkg_source,
        template.as_mut(),
        false,
        false,
        true,
        false,
    )
}

/// Find the default entrypoiny to read the doc comments from
///
/// Try to read entrypoint in the following order:
/// - src/lib.rs
/// - src/main.rs
/// - file defined in the `[lib]` section of Cargo.toml
/// - file defined in the `[[bin]]` section of Cargo.toml, if there is only one
///   - if there is more than one `[[bin]]`, an error is returned
fn find_entrypoint(current_dir: &Path, manifest: &Manifest) -> Result<File, String> {
    let entrypoint = find_entrypoint_internal(current_dir, &manifest)?;
    File::open(current_dir.join(entrypoint)).map_err(|e| format!("{}", e))
}

/// Find the default entrypoiny to read the doc comments from
///
/// Try to read entrypoint in the following order:
/// - src/lib.rs
/// - src/main.rs
/// - file defined in the `[lib]` section of Cargo.toml
/// - file defined in the `[[bin]]` section of Cargo.toml, if there is only one
///   - if there is more than one `[[bin]]`, an error is returned
fn find_entrypoint_internal(current_dir: &Path, _manifest: &Manifest) -> Result<PathBuf, String> {
    // try lib.rs
    let lib_rs = current_dir.join("src/lib.rs");
    if lib_rs.exists() {
        return Ok(lib_rs);
    }

    // try main.rs
    let main_rs = current_dir.join("src/main.rs");
    if main_rs.exists() {
        return Ok(main_rs);
    }

    // try lib defined in `Cargo.toml`
    // if let Some(ManifestLib {
    //     path: ref lib,
    //     doc: true,
    // }) = manifest.lib
    // {
    //     return Ok(lib.to_path_buf());
    // }

    // try bin defined in `Cargo.toml`
    // if manifest.bin.len() > 0 {
    //     let mut bin_list: Vec<_> = manifest
    //         .bin
    //         .iter()
    //         .filter(|b| b.doc == true)
    //         .map(|b| b.path.clone())
    //         .collect();

    //     if bin_list.len() > 1 {
    //         let paths = bin_list
    //             .iter()
    //             .map(|p| p.to_string_lossy())
    //             .collect::<Vec<_>>()
    //             .join(", ");
    //         return Err(format!("Multiple binaries found, choose one: [{}]", paths));
    //     }

    //     if let Some(bin) = bin_list.pop() {
    //         return Ok(bin);
    //     }
    // }

    // if no entrypoint is found, return an error
    Err("No entrypoint found".to_owned())
}

/// Find the template file to be used to generate README files.
///
/// Start from the package's folder & go up until a template is found
/// (or none).
fn find_readme_template<'a>(
    root_path: &'a Path,
    pkg_path: &'a Path,
) -> Result<Option<PathBuf>, String> {
    let mut cur_path = pkg_path;
    let mut tpl_path = cur_path.join("README.tpl");
    while !tpl_path.exists() && cur_path >= root_path {
        cur_path = cur_path.parent().unwrap();
        tpl_path = cur_path.join("README.tpl");
    }
    Ok(if tpl_path.exists() {
        Some(tpl_path)
    } else {
        None
    })
}

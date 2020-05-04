use cargo::core::Manifest;
use cargo_readme::generate_readme;
use sha1::{Digest, Sha1};
use std::{
    fmt::Display,
    fs::{self, File},
    path::{Path, PathBuf},
};

pub enum GenerateReadmeMode {
    // Do not generate README, skip operation
    Skip,
    // Generate README for check purpose only,
    // files are not written to disk.
    CheckOnly,
    // Generate README files and write them on disk.
    // GenerateIfMissing,
    // GenerateAppend,
    // GenerateOverwrite,
}

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
                _ => unreachable!(),
            }
        )
    }
}

pub fn check_readme(
    pkg_path: &Path,
    pkg_manifest: &Manifest,
    mode: GenerateReadmeMode,
) -> Result<(), String> {
    match mode {
        GenerateReadmeMode::Skip => Ok(()),
        CheckOnly => {
            let mut pkg_source = find_entrypoint(pkg_path, pkg_manifest)?;
            let readme_path = pkg_path.join("README.md");
            match fs::read_to_string(readme_path) {
                Ok(pkg_readme) => {
                    let readme =
                        generate_readme(pkg_path, &mut pkg_source, None, true, false, true, true)?;
                    let pkg_readme_hash = Sha1::from(pkg_readme);
                    let gen_readme_hash = Sha1::from(readme);
                    if pkg_readme_hash == gen_readme_hash {
                        Ok(())
                    } else {
                        Err(CheckReadmeResult::UpdateNeeded.to_string())
                    }
                }
                Err(err) => Err(CheckReadmeResult::Missing.to_string()),
            }
        }
        _ => unreachable!(),
    }
    //TODO: should delete README when the entire operation is finished ?
    // fs::write(readme_path, readme.as_bytes()).map_err(|e| format!("{:}", e))
}

/// Find the default entrypoiny to read the doc comments from
///
/// Try to read entrypoint in the following order:
/// - src/lib.rs
/// - src/main.rs
/// - file defined in the `[lib]` section of Cargo.toml
/// - file defined in the `[[bin]]` section of Cargo.toml, if there is only one
///   - if there is more than one `[[bin]]`, an error is returned
pub fn find_entrypoint(current_dir: &Path, manifest: &Manifest) -> Result<File, String> {
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
pub fn find_entrypoint_internal(
    current_dir: &Path,
    manifest: &Manifest,
) -> Result<PathBuf, String> {
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

    // // try bin defined in `Cargo.toml`
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

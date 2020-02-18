use log::trace;
use std::{error::Error, fs, path::{Path, PathBuf}};
use toml_edit::{Document, Item, Value};

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies(root_manifest: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    run_recursive(root_manifest, de_dev_deps)
}
    
fn de_dev_deps<'a>(
    doc: &mut Document,
    target: &'a Path,
) -> Result<(), Box<dyn Error>> {
    trace!("Removing dev-dependencies on {:?}", target);

    doc.as_table_mut().remove("dev-dependencies");
    fs::write(target, doc.to_string())?;
    Ok(())
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
        let mut doc: Document = fs::read_to_string(&filename)?.parse()?;
        let _ = f(&mut doc, &filename)?;
    }

    Ok(())
}
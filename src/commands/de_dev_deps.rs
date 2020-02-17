use log::trace;
use std::{error::Error, fs, path::Path};
use toml_edit::Document;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies<'a>(
    doc: &mut Document,
    target: &'a Path,
) -> Result<(), Box<dyn Error>> {
    trace!("Removing dev-dependencies on {:?}", target);

    doc.as_table_mut().remove("dev-dependencies");
    fs::write(target, doc.to_string())?;
    Ok(())
}

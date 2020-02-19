use std::{error::Error, fs};
use cargo::core::{
    package::Package,
    Workspace,
};
use toml_edit::Document;

/// Deactivate the Dev Dependencies Section of the given toml
pub fn deactivate_dev_dependencies<'a, F>(ws: Workspace<'a>, predicate: F) -> Result<(), Box<dyn Error>>
where F: Fn(&Package) -> bool
{
    for pkg in ws.members().filter(|p|predicate(p)) {
        let manifest_path = pkg.manifest_path();
        let content = fs::read_to_string(manifest_path)?;
        let mut doc: Document = content.parse()?;
        doc.as_table_mut().remove("dev-dependencies");
        fs::write(manifest_path, doc.to_string())?;
    }
    Ok(())
}
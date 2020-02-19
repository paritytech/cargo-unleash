use std::{error::Error, fs};
use cargo::core::{
    package::Package,
};
use toml_edit::Document;

/// Run f on every package's manifest, write the doc. Fail on first error
pub fn edit_each<'a, I, F>(iter: I, f: F) -> Result<(), Box<dyn Error>> 
where
    F: Fn(&'a Package, &mut Document) -> Result<(), Box<dyn Error>>,
    I: Iterator<Item=&'a Package>
{
    for pkg in iter {
        let manifest_path = pkg.manifest_path();
        let content = fs::read_to_string(manifest_path)?;
        let mut doc: Document = content.parse()?;
        f(pkg, &mut doc)?;
        fs::write(manifest_path, doc.to_string())?;
    }
    Ok(())
}
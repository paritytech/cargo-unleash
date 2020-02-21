use std::{error::Error, fs};
use cargo::core::{
    package::Package,
};
use toml_edit::{Document, Table, InlineTable, Item, Value};
use log::trace;

/// Run f on every package's manifest, write the doc. Fail on first error
pub fn edit_each<'a, I, F, R>(iter: I, f: F) -> Result<Vec<R>, Box<dyn Error>> 
where
    F: Fn(&'a Package, &mut Document) -> Result<R, Box<dyn Error>>,
    I: Iterator<Item=&'a Package>
{
    let mut results = Vec::new();
    for pkg in iter {
        let manifest_path = pkg.manifest_path();
        let content = fs::read_to_string(manifest_path)?;
        let mut doc: Document = content.parse()?;
        results.push(f(pkg, &mut doc)?);
        fs::write(manifest_path, doc.to_string())?;
    }
    Ok(results)
}

/// Wrap each the different dependency as a mutable item
pub enum DependencyEntry<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

/// Iterate through the dependency sections of root, find each
/// dependency entry, that is a subsection and hand it and its name
/// to f. Return the counter of how many times f returned true.
pub fn edit_each_dep<'a, F>(root: &'a mut Table, f: F) -> u32
    where F: Fn(String, DependencyEntry) -> bool
{
    let mut counter = 0;
    for k in vec!["dependencies", "dev-dependencies", "build-dependencies"] {
        let keys = {
            if let Some(Item::Table(t)) = &root.get(k) {
                t.iter().filter_map(|(key, v)| {
                    if v.is_table() || v.is_inline_table() { Some(key.to_owned()) } else { None }
                }).collect::<Vec<_>>()
            } else {
                continue
            }
        };
        let t = root.entry(k).as_table_mut().expect("Just checked. qed");

        for key in keys {
            match t.entry(&key) {
                Item::Value(Value::InlineTable(info)) => {
                    let name = {
                        if let Some(name) = info.get("package").clone() { // is there a rename
                            name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned()
                        } else {
                            key
                        }
                    };
                    if f(name, DependencyEntry::Inline(info)) {
                        counter += 1;
                    }

                }, 
                Item::Table(info) => {
                    let name = {
                        if let Some(name) = info.get("package").clone() { // is there a rename
                            name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned()
                        } else {
                            key
                        }
                    };
                    
                    if f(name, DependencyEntry::Table(info)) {
                        counter += 1;
                    }
                }
                _ => {
                    trace!("Unsupported dependency format")
                }

            }
        }
    };
    counter
}
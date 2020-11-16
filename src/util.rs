use cargo::{
    core::{package::Package, Workspace},
    sources::PathSource,
};
use log::warn;
use std::{error::Error, fs};
use toml_edit::{Document, InlineTable, Item, Table, Value};

pub fn members_deep<'a>(ws: &'a Workspace) -> Vec<Package> {
    let mut total_list = Vec::new();
    for m in ws.members() {
        total_list.push(m.clone());
        for dep in m.dependencies() {
            let source = dep.source_id();
            if source.is_path() {
                let dst = source
                    .url()
                    .to_file_path()
                    .expect("It was just checked before. qed");
                let mut src = PathSource::new(&dst, source, ws.config());
                let pkg = src.root_package().expect("Path must have a package");
                if !ws.is_member(&pkg) {
                    total_list.push(pkg);
                }
            }
        }
    }
    total_list
}

/// Run f on every package's manifest, write the doc. Fail on first error
pub fn edit_each<'a, I, F, R>(iter: I, f: F) -> Result<Vec<R>, Box<dyn Error>>
where
    F: Fn(&'a Package, &mut Document) -> Result<R, Box<dyn Error>>,
    I: Iterator<Item = &'a Package>,
{
    let mut results = Vec::new();
    for pkg in iter {
        let manifest_path = pkg.manifest_path();
        let content = fs::read_to_string(manifest_path)?;
        let mut doc: Document = content
            .parse()
            .map_err(|e| format!("Parsing {:?} failed: {:}", manifest_path, e))?;
        let res = f(pkg, &mut doc)?;
        let new_doc = doc.to_string();
        if content != new_doc {
            fs::write(format!("{:?}.bak", manifest_path), content)?;
            fs::write(manifest_path, new_doc)?;
            results.push(res);
        }
    }
    Ok(results)
}


/// Deactivate the Dev Dependencies Section of the given toml
pub fn with_deactivated_dev_dependencies<'a, I, F, A>(iter: I, fun: F)
    -> Result<A, Box<dyn Error>>
where
    I: Iterator<Item = &'a Package>,
    F: Fn() -> Result<A, Box<dyn Error>>,
{

    let edited = edit_each(iter, |p, doc| {
        doc.as_table_mut().remove("dev-dependencies");
        Ok(p.manifest_path())
    })?;
    let res =  fun();
    if res.is_err(){
        // revert dev-deps
        for path in edited {
            fs::rename(format!("{:?}.bak", path), path)?;
        }
    }
    res
}

/// Wrap each the different dependency as a mutable item
pub enum DependencyEntry<'a> {
    Table(&'a mut Table),
    Inline(&'a mut InlineTable),
}

#[derive(Debug, PartialEq)]
/// The action (should be) taken on the dependency entry
pub enum DependencyAction {
    /// Ignored, we didn't touch
    Untouched,
    /// Entry was changed, needs to be saved
    Mutated,
    /// Remove this entry and save the manifest
    Remove,
}

/// Iterate through the dependency sections of root, find each
/// dependency entry, that is a subsection and hand it and its name
/// to f. Return the counter of how many times f returned true.
pub fn edit_each_dep<F>(root: &mut Table, f: F) -> u32
where
    F: Fn(String, Option<String>, DependencyEntry) -> DependencyAction,
{
    let mut counter = 0;
    let mut removed = Vec::new();
    for k in &["dependencies", "dev-dependencies", "build-dependencies"] {
        let keys = {
            if let Some(Item::Table(t)) = &root.get(k) {
                t.iter()
                    .filter_map(|(key, v)| {
                        if v.is_table() || v.is_inline_table() {
                            Some(key.to_owned())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            } else {
                continue;
            }
        };
        let t = root.entry(k).as_table_mut().expect("Just checked. qed");

        for key in keys {
            let (name, action) = match t.entry(&key) {
                Item::Value(Value::InlineTable(info)) => {
                    let (name, alias) = {
                        if let Some(name) = info.get("package") {
                            // is there a rename
                            (name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned(),
                            Some(key.clone()))
                        } else {
                            (key.clone(), None)
                        }
                    };
                    (name.clone(), f(name, alias, DependencyEntry::Inline(info)))
                }
                Item::Table(info) => {
                    let (name, alias) = {
                        if let Some(name) = info.get("package") {
                            // is there a rename
                            (name
                                .as_str()
                                .expect("Package is always a string, or cargo would have failed before. qed")
                                .to_owned(),
                            Some(key.clone()))
                        } else {
                            (key.clone(), None)
                        }
                    };

                    (name.clone(), f(name, alias, DependencyEntry::Table(info)))
                }
                _ => {
                    warn!("Unsupported dependency format");
                    (key, DependencyAction::Untouched)
                }
            };

            if action == DependencyAction::Remove {
                t.remove(&name);
                removed.push(name);
            }
            if action != DependencyAction::Untouched {
                counter += 1;
            }
        }
    }

    if !removed.is_empty() {
        if let Item::Table(features) = root.entry("features") {
            let keys = features
                .iter()
                .map(|(k, _v)| k.to_owned())
                .collect::<Vec<_>>();
            for feat in keys {
                if let Item::Value(Value::Array(deps)) = features.entry(&feat) {
                    let mut to_remove = Vec::new();
                    for (idx, dep) in deps.iter().enumerate() {
                        if let Value::String(s) = dep {
                            if let Some(s) = s.value().trim().split('/').next() {
                                if removed.contains(&s.to_owned()) {
                                    to_remove.push(idx);
                                }
                            }
                        }
                    }
                    if !to_remove.is_empty() {
                        // remove starting from the end:
                        to_remove.reverse();
                        for idx in to_remove {
                            deps.remove(idx);
                        }
                    }
                }
            }
        }
    }
    counter
}
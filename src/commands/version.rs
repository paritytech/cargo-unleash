use std::{
    collections::HashMap,
    error::Error
};
use cargo::core::{
    package::Package, Workspace
};
use toml_edit::{Item, Value, Table, decorated};
use semver::{Version, VersionReq};
use crate::util::edit_each;
use log::trace;

fn updated_deps<'a>(
    root: &'a mut Table,
    updates: &'a HashMap<String, Version>
) -> u32 {
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

                    if !info.contains_key("path") {
                        continue // entry isn't local
                    }

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

                    if let Some(new_version) = updates.get(&name) {
                        trace!("We changed the version of {:} to {:}", name, new_version);
                        // this has been changed.
                        if let Some(v_req) = info.get_mut("version") {
                            let r = v_req
                                .as_str()
                                .ok_or("Version must be string".to_owned())
                                .and_then(|s| VersionReq::parse(s).map_err(|e| format!("Parsing failed {:}", e)))
                                .expect("Cargo enforces us using semver versions. qed");
                            if !r.matches(new_version) {
                                trace!("Versions don't match anymore, updating.");
                                counter += 1;
                                *v_req = Value::from(format!("{:}", new_version));
                            }
                        } else {
                            // not yet present, we force set.
                            trace!("No version found, setting.");
                            counter += 1;
                            info.get_or_insert("version", Value::from(format!("{:}", new_version)));
                        }
                    }
                }, 
                Item::Table(info) => {
                    if !info.contains_key("path") {
                        continue // entry isn't local
                    }

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

                    if let Some(new_version) = updates.get(&name) {
                        trace!("We changed the version of {:} to {:}", name, new_version);
                        // this has been changed.
                        if let Some(v_req) = info.get("version") {
                            let r = v_req
                                .as_str()
                                .ok_or("Version must be string".to_owned())
                                .and_then(|s| VersionReq::parse(s).map_err(|e| format!("Parsing failed {:}", e)))
                                .expect("Cargo enforces us using semver versions. qed");
                            if r.matches(new_version) {
                                continue
                            }
                            trace!("Versions don't match anymore, updating.");
                        } else {
                            trace!("No version found, setting.");
                        }

                        counter += 1;
                        info["version"] = Item::Value(Value::from(format!("{:}", new_version)));
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

/// For packages matching predicate set to mapper given version, if any. Update all members dependencies
/// if necessary.
pub fn set_version<'a, M, P>(ws: &Workspace<'a>, predicate: P, mapper: M) -> Result<(), Box<dyn Error>>
where
    P: Fn(&Package) -> bool,
    M: Fn(&Package) -> Option<Version>,
{
    let c = ws.config();

    let mut updates = HashMap::new();
    
    for s in edit_each(ws.members().filter(|p| predicate(p)),
        |p, doc| Ok(mapper(p).map(|nv_version| {
            c.shell()
                .status("Bumping", format!("{:}: {:} -> {:}", p.name(), p.version(), nv_version))
                .expect("Writing to the shell would have failed before. qed");
            doc["package"]["version"] = Item::Value(Value::from(nv_version.to_string()));
            (p.name().as_str().to_owned(), nv_version.clone())
        }))
    )? {
        if let Some((name, version)) = s {
            updates.insert(name, version);
        }
    };

    c.shell().status("Updating", "Dependency tree")?;
    edit_each(ws.members(), |p, doc| {
        c.shell().status("Updating", p.name())?;
        let root = doc.as_table_mut();
        let mut updates_count = 0;
        updates_count += updated_deps(root, &updates);
        
        if let Item::Table(t) = root.entry("target") {
            let keys = t.iter().filter_map(|(k, v)| {
                if v.is_table() {
                    Some(k.to_owned())
                } else {
                    None
                }
            }).collect::<Vec<_>>();
            
            for k in keys {
                if let Item::Table(root) = t.entry(&k) {
                    updates_count += updated_deps(root, &updates);
                }
            };
        }
        if updates_count == 0 {
            c.shell().status("Done", "No dependency updates")?;
            
        } else if updates_count == 1 {
            c.shell().status("Done", "One dependency updated")?;
        } else {
            c.shell().status("Done", format!("{} dependencies updated", updates_count))?;
        }



        Ok(())
    })?;

    Ok(())
}
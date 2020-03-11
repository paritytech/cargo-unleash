use cargo::{
    core::{
        Workspace,
        package::Package,
    },
    sources::PathSource,
};
use log::warn;
use serde::Deserialize;
use std::{
    collections::HashMap,
    error::Error,
    fs,
    time::Duration
};
use tokio::runtime::Runtime;
use toml_edit::{Document, InlineTable, Item, Table, Value};
use futures::future::FutureExt;

pub fn members_deep<'a>(ws: &'a Workspace) -> Vec<Package> {
    let mut total_list = Vec::new();
    for m in ws.members() {
        total_list.push(m.clone());
        for dep in m.dependencies() {
            let source = dep.source_id().clone();
            if source.is_path() {
                let dst = source.url().to_file_path().expect("It was just checked before. qed");
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

#[derive(Debug, PartialEq)]
/// The action (should be) taken on the dependency entry
pub enum DependencyAction {
    /// Ignored, we didn't touch
    Untouched,
    /// Entry was changed, needs to be saved
    Mutated,
    /// Remove this entry and save the manifest
    Remove
}

/// Iterate through the dependency sections of root, find each
/// dependency entry, that is a subsection and hand it and its name
/// to f. Return the counter of how many times f returned true.
pub fn edit_each_dep<'a, F>(root: &'a mut Table, f: F) -> u32
where
    F: Fn(String, Option<String>, DependencyEntry) -> DependencyAction,
{
    let mut counter = 0;
    let mut removed = Vec::new();
    for k in vec!["dependencies", "dev-dependencies", "build-dependencies"] {
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
                        if let Some(name) = info.get("package").clone() {
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
                        if let Some(name) = info.get("package").clone() {
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
            let keys = features.iter().map(|(k, _v)| k.to_owned()).collect::<Vec<_>>();
            for feat in keys {
                if let Item::Value(Value::Array(deps)) = features.entry(&feat) {
                    let mut to_remove = Vec::new();
                    for (idx, dep) in deps.iter().enumerate() {
                        if let Value::String(s) = dep {
                            if let Some(s) = s.value().trim().split("/").next() {
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

#[derive(Deserialize)]
pub struct Versions {
    pub versions: Vec<Version>,
}

#[derive(Deserialize)]
pub struct Version {
    #[serde(rename = "num")]
    pub version: semver::Version,
    pub yanked: bool,
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

pub fn fetch_many_cratesio_versions(
    crates: Vec<String>,
) -> Result<HashMap<String, Vec<Version>>, String> {
    let mut rt = Runtime::new().map_err(|e| e.to_string())?;
    rt.block_on(fetch_cratesio_versions(crates))
}

async fn fetch(client: reqwest::Client, name: String) -> Result<Vec<Version>, String> {
    let url = format!(
        "https://crates.io/api/v1/crates/{crate_name}/versions",
        crate_name = name
    );
    match client.get(&url).send().await {
        Ok(response) => {
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(vec![]);
            }
            Ok(response
                .json::<Versions>()
                .await
                .map_err(|e| e.to_string())?
                .versions)
        }
        Err(e) => {
            if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                return Ok(vec![]);
            }
            Err(e.to_string())
        }
    }
}

async fn fetch_cratesio_versions(
    crates: Vec<String>,
) -> Result<HashMap<String, Vec<Version>>, String> {
    let timeout = Duration::from_secs(10);
    let client = reqwest::ClientBuilder::new()
        .connect_timeout(timeout)
        .user_agent(APP_USER_AGENT)
        .build()
        .map_err(|e| e.to_string())?;

    let fts = crates.into_iter().map(move |name| {
        let c = client.clone();
        let n = name.to_string();
        fetch(c, n.clone()).map(move |r| (n, r))
    });

    let (success, failures): (Vec<_>, Vec<_>) = futures::future::join_all(fts)
        .await
        .into_iter()
        .partition(|(_, r)| r.is_ok());

    if failures.len() > 0 {
        return Err(format!(
            "Failure fetching crates versions: {:}",
            failures
                .into_iter()
                .map(|(n, e)| format!("{:}: {:}", n, e.err().expect("we partioned based on error")))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(success
        .into_iter()
        .map(move |(name, r)| (name.clone(), r.expect("We partioned based on error")))
        .collect::<HashMap<String, _>>())
}

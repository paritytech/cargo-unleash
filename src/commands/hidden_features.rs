use cargo::util::config::Config;
use lazy_static::lazy_static;
use regex::Regex;
use std::cmp::{Eq, PartialEq};
use std::collections::{HashMap, HashSet};
use std::fs::read_to_string;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Extracts the features from a given string and collects them into a Vector.
/// e.g `"#[cfg(features = "foo", features= "bar")]"` -> `vec!["foo", "bar"]`
fn extract_feature_names(line: &str) -> Option<Vec<&str>> {
    // Using lazy_static here to avoid having to compile this regex everytime.
    lazy_static! {
        static ref RE: Regex =
            Regex::new(r#"feature\s*=\s*"(?P<feature>((\w*)-*)*)""#).expect("Invalid regex");
    }
    Some(
        RE.captures_iter(line)
            // For each match, extract the "feature" group which we just captured.
            .map(|c| match c.name("feature") {
                Some(val) => val.as_str(),
                None => unreachable!(), // capture has "feature" in it, so this can't be reached.
            })
            .collect(),
    )
}

/// Struct that represents a feature.
#[derive(Debug, Clone)]
pub enum Feature {
    // A feature that is used inside the code. The path and line_number are kept
    // so that we can produce a clickable link when we print it.
    UsedFeature {
        name: String,
        path: PathBuf,
        line_number: u64,
    },
    // A feature that is exposed by a Cargo.toml.
    ExposedFeature {
        name: String,
    },
}

// We implement Hash for Feature because we want two objects to be identical if they share the same name.
// This avoids having tons of different lines written out if they all share the same feature in the same crate.
impl Hash for Feature {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.name().hash(state);
    }
}

impl PartialEq for Feature {
    fn eq(&self, other: &Self) -> bool {
        self.name() == other.name()
    }
}

impl Eq for Feature {}

impl Feature {
    /// Returns the path to the file, if it exists.
    fn path(&self) -> Option<&Path> {
        match self {
            Self::UsedFeature { path, .. } => Some(path),
            Self::ExposedFeature { .. } => None,
        }
    }

    /// Returns a clinkable link to the feature inside the code.
    fn clickable_path(&self) -> Option<String> {
        match self {
            Self::UsedFeature {
                path, line_number, ..
            } => {
                let clickable_path = format!("{}:{}", path.display(), line_number);
                Some(clickable_path)
            }
            Self::ExposedFeature { .. } => None,
        }
    }

    /// Returns the name of the feature.
    fn name(&self) -> &str {
        match self {
            Self::UsedFeature { name, .. } | Self::ExposedFeature { name } => name,
        }
    }
}

/// A representation of a Crate.
#[derive(Debug)]
struct CrateInfo {
    // Path to the Cargo.toml file.
    path: PathBuf,
    // Set of all exposed features (represented as Strings).
    exposed_features: HashSet<Feature>,
    // Set of all used features, represented as Strings.
    used_features: HashSet<Feature>,
    // Set that represents the difference between the used features and the exposed features.
    hidden_features: HashSet<Feature>,
}

impl CrateInfo {
    /// Creates a new `CrateInfo` object, given a `Path` to its Cargo.toml file.
    fn new(path: &Path) -> Self {
        let path = path.to_path_buf();
        let exposed_features = HashSet::new();
        let used_features = HashSet::new();
        let hidden_features = HashSet::new();
        Self {
            path,
            exposed_features,
            used_features,
            hidden_features,
        }
    }

    /// Adds some features to the set of used features.
    fn add_used_features(&mut self, new_features: &HashSet<Feature>) {
        self.used_features = self.used_features.union(new_features).cloned().collect();
    }
}

/// Helper function to determine whether an entry is hidden (starts with '.').
fn is_hidden(entry: &DirEntry) -> bool {
    if entry.depth() == 0 {
        return false;
    }
    entry
        .file_name()
        .to_str()
        .map_or(false, |s| s.starts_with('.'))
}
/// A mapping from `PathBuf` to `CrateInfo`. Only crates which USE features in their code will be added.
#[derive(Debug)]
pub struct HiddenFeaturesFinder<'a> {
    mapping: HashMap<PathBuf, CrateInfo>,

    // Set of paths to be ignored.
    ignored_paths: HashSet<PathBuf>,

    // Set of features to be ignored.
    ignored_features: HashSet<String>,

    // Shell reference for easier printing.
    config: Option<&'a Config>,
}

impl<'a> HiddenFeaturesFinder<'a> {
    pub fn new(
        ignored_paths: HashSet<PathBuf>,
        ignored_features: HashSet<String>,
        config: Option<&'a Config>,
    ) -> Self {
        Self {
            mapping: HashMap::new(),
            ignored_paths,
            ignored_features,
            config,
        }
    }

    /// Finds the Cargo.toml file associated to a path.
    pub fn find_associated_cargo(&self, mut path: &Path) -> Option<PathBuf> {
        loop {
            // Create a potential candidate by appending Cargo.toml
            let candidate = path.join("Cargo.toml");
            // Check whether this file exists.
            if self.mapping.contains_key(&candidate) || candidate.exists() {
                // It exists, so we've found the Cargo.toml that corresponds to the initial path.
                return Some(candidate);
            } else {
                // Bad candidate: keep on looping, this time using the parent directory as path.
                path = path.parent()?;
            }
        }
    }

    /// Finds the used features by ripgrep'ing the path, looking for occurences of the pattern "feature = ".
    /// Then groups those occurences by crates.
    pub fn find_used_features(&mut self, path: &Path) -> Result<(), String> {
        let walker = WalkDir::new(path).into_iter();
        // Using a vec to store features, because borrow checker wasn't happy.
        let mut features = Vec::new();
        for entry in
            walker.filter_entry(|e| !is_hidden(e) && !self.ignored_paths.contains(e.path()))
        {
            let entry = entry.map_err(|e| e.to_string())?;
            let entry_path = entry.path();
            // If the entry path figures amongst the list of ignored paths, then skip it.
            if self.ignored_paths.contains(entry_path) {
                continue;
            }
            let is_rust_file = entry_path
                .extension()
                .map_or(false, |ext| ext.to_str().map_or(false, |s| s == "rs"));
            // We only wish to parse .rs files!
            if is_rust_file {
                let file = File::open(entry.path()).map_err(|e| e.to_string())?;
                let lines = BufReader::new(file).lines();
                let path_buf = entry_path.to_path_buf();
                // Go through every line of the file.
                for (line_number, line) in lines.enumerate() {
                    // Make sure the line is an acceptable `String`.
                    if let Ok(line) = line {
                        // Extract the feature names.
                        let feature_names = extract_feature_names(&line);

                        // If we found some features, add them!
                        if let Some(f) = feature_names {
                            for feature_name in f {
                                if !self.ignored_features.contains(feature_name) {
                                    let feature = Feature::UsedFeature {
                                        name: feature_name.to_string(),
                                        path: path_buf.clone(),
                                        line_number: line_number as u64,
                                    };
                                    features.push(feature);
                                }
                            }
                        }
                    }
                }
            }
        }
        for feature in features {
            self.add_feature(feature)?;
        }
        Ok(())
    }

    /// Adds the feature to the mapping from paths to crates.
    pub fn add_feature(&mut self, feature: Feature) -> Result<(), String> {
        let path = feature
            .path()
            .ok_or_else(|| "internal error: should have a path")?;
        // The path to the parent directory
        let parent = path.parent().ok_or_else(|| "path has no parent")?;
        // Create a Cargo.toml path candidate: a Cargo file that would be in the same directory as the .rs file we just matched.
        let cargo_path = self
            .find_associated_cargo(&parent)
            .ok_or_else(|| "could not find corresponding Cargo file")?;

        if let Some(crate_info) = self.mapping.get_mut(&cargo_path) {
            // This crate is already in the map, so simply add the feature to the list of used features.
            crate_info.used_features.insert(feature);
        } else {
            let mut used_features_set = HashSet::new();
            // Populate the set with the list of used features.
            used_features_set.insert(feature);

            // Create a cargo entry, filled with the used vec.
            let mut crate_info = CrateInfo::new(&cargo_path);
            crate_info.add_used_features(&used_features_set);
            // Insert the Cargo entry in the path mapping.
            self.mapping.insert(cargo_path, crate_info);
        }
        Ok(())
    }

    /// Finds the exposed features of every Cargo.toml file in the mapping.
    pub fn find_exposed_features(&mut self) {
        // Iterate over every Cargo.
        for v in self.mapping.values_mut() {
            // Load its content in a String. Using unwrap because we want our program to stop in case of an error.
            let s = read_to_string(&v.path).unwrap();
            // Parse the Cargo into a TOML structure. Using unwrap because we want our program to stop in case of an error.
            let toml = s.parse::<toml::Value>().unwrap();
            let table = match &toml.get("features") {
                Some(toml::Value::Table(table)) => Some(table),
                _ => None,
            };
            let mut exposed = HashSet::new();
            if let Some(table) = table {
                for (feature_name, _) in table.iter() {
                    let name = feature_name.to_string();
                    // Make sure the feature is not one of the ignored features.
                    if !self.ignored_features.contains(&name) {
                        exposed.insert(Feature::ExposedFeature { name });
                    };
                }
            }
            v.exposed_features = exposed;
        }
    }

    /// Finds the hidden features, i.e. features that are used in the code but not exposed in their corresponding Cargo.toml file.
    pub fn find_hidden_features(&mut self) {
        // Iterate over the HiddenFeaturesFinder's crates.
        for crate_info in self.mapping.values_mut() {
            // Find the difference between the used features and the exposed ones, and collects it into a set.
            crate_info.hidden_features = crate_info
                .used_features
                .difference(&crate_info.exposed_features)
                .cloned()
                .collect();
        }
    }

    // todo pretty print
    pub fn check_hidden_features(&self) -> Result<(), String> {
        let mut empty = true;
        for cargo in self.mapping.values() {
            for feature in &cargo.hidden_features {
                empty = false;
                let msg = format!(
                    "{}\t{}",
                    feature.name(),
                    feature
                        .clickable_path()
                        .unwrap_or_else(|| String::from(feature.name()))
                );
                match self.config {
                    Some(c) => {
                        c.shell().warn(msg).map_err(|e| e.to_string())?;
                    }
                    None => println!("{}", msg),
                }
            }
        }
        if empty {
            Ok(())
        } else {
            Err("Hidden features detected\n
				Hint: you can ignore specific features with `--ignored-features`
				and/or specific paths with `--ignored-paths`"
                .to_string())
        }
    }

    #[cfg(test)]
    /// Returns a set of all the hidden features names.
    /// Used for testing purposes.
    pub fn hidden_features(&self) -> HashSet<&str> {
        let mut res = HashSet::new();
        for cargo in self.mapping.values() {
            for feature in &cargo.used_features {
                res.insert(feature.name());
            }
        }
        res
    }

    #[cfg(test)]
    pub fn find_and_check(&mut self, path: &Path) -> Result<(), String> {
        self.find_used_features(path)?;
        self.find_exposed_features();
        self.find_hidden_features();
        self.check_hidden_features()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // we do not define a TEST_DIR because concat! does not work with consts...
    const NO_FEATURES_FILE: &str = "test_files/no_features.rs";
    const ONE_FEATURE_FILE: &str = "test_files/one_feature.rs";
    const FOUR_FEATURES_FILE: &str = "test_files/four_features.rs";
    const ONE_LINE_FEATURES_FILE: &str = "test_files/one_line_features.rs";

    #[test]
    fn empty_features() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let res = p.check_hidden_features();
        dbg!(&res);
        assert!(res.is_ok());
    }

    #[test]
    fn no_features() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(NO_FEATURES_FILE);
        let res = p.find_and_check(&path);
        dbg!(&res);
        assert!(res.is_ok());
    }

    #[test]
    fn does_not_exist() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::new();
        let res = p.find_and_check(&path);
        dbg!(&res);
        assert!(res.is_err());
    }

    #[test]
    fn one_feature() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(ONE_FEATURE_FILE);
        let res = p.find_and_check(&path);
        let features = p.hidden_features();
        assert!(features.contains("hidden-feature"));
        dbg!(&res);
        assert!(res.is_err());
    }

    #[test]
    fn one_feature_but_excluded() {
        let excluded_paths = HashSet::new();
        let mut excluded_features = HashSet::new();
        excluded_features.insert(String::from("hidden-feature"));
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(ONE_FEATURE_FILE);
        let res = p.find_and_check(&path);
        let features = p.hidden_features();
        assert!(features.is_empty());
        dbg!(&res);
        assert!(res.is_ok());
    }

    #[test]
    fn one_feature_but_path_excluded() {
        let mut excluded_paths = HashSet::new();
        excluded_paths.insert(PathBuf::from(ONE_FEATURE_FILE));
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(ONE_FEATURE_FILE);
        let res = p.find_and_check(&path);
        let features = p.hidden_features();
        assert!(features.is_empty());
        dbg!(&res);
        assert!(res.is_ok());
    }

    #[test]
    fn four_features() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(FOUR_FEATURES_FILE);
        let res = p.find_and_check(&path);
        let mut features = p.hidden_features();
        dbg!(&features);
        assert!(features.remove("hidden-feature-1"));
        assert!(features.remove("hidden-feature-2"));
        assert!(features.remove("hidden-feature-3"));
        assert!(features.remove("hidden-feature-4"));
        assert!(features.is_empty());
        dbg!(&res);
        assert!(res.is_err());
    }

    #[test]
    fn one_line_features() {
        let excluded_paths = HashSet::new();
        let excluded_features = HashSet::new();
        let mut p = HiddenFeaturesFinder::new(excluded_paths, excluded_features, None);
        let path = PathBuf::from(ONE_LINE_FEATURES_FILE);
        let res = p.find_and_check(&path);
        let mut features = p.hidden_features();
        dbg!(&features);
        assert!(features.remove("get-your"));
        assert!(features.remove("shit-together"));
        assert!(features.remove("get-it-all-together"));
        assert!(features.remove("and-put-it-all-in-a-backpack"));
        assert!(features.is_empty());
        dbg!(&res);
        assert!(res.is_err());
    }
}

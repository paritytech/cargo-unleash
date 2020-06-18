use semver::Version;

pub struct Package {
    name: String,
    version: Version,
}

impl Package {
    pub fn version(&self) -> &Version {
        &self.version
    }
    pub fn name(&self) -> &String {
        &self.name
    }
}

pub enum Comparator {
    Eq(String),
    Neq(String),
    Starts(String),
    Ends(String),
}

impl Comparator {
    pub fn matches(&self, name: &String) -> bool {
        match &*self {
            Comparator::Eq(v) => name == v,
            Comparator::Neq(v) => name != v,
            Comparator::Starts(v) => name.starts_with(v),
            Comparator::Ends(v) => name.ends_with(v),
        }
    }
}

pub enum Matcher {
    Version(Version),
    Name(Comparator),
}


impl Matcher {
    pub fn matches(&self, pkg: &Package) -> bool {
        match &*self {
            Matcher::Version(v) =>  pkg.version() == v,
            Matcher::Name(comp) =>  comp.matches(pkg.name()),
        }

    }
}

pub fn parse(input: &str) -> Result<Matcher, String> {
    
    if input.starts_with("version=") {
        Ok(Matcher::Version(
            Version::parse(&input[8..])
                .map_err(|e| format!("Could not parse version: {:}", e))
        ?))
    } else if input.starts_with("name") {
        let comparator = if input[4..].starts_with("^=") {
                Comparator::Starts(input[6..].to_string())
            } else if input[4..].starts_with("$=") {
                Comparator::Ends(input[6..].to_string())
            } else if input[4..].starts_with("!=") {
                Comparator::Neq(input[6..].to_string())
            } else if input[4..].starts_with("==") {
                Comparator::Eq(input[6..].to_string())
            } else if input[4..].starts_with("=") {
                Comparator::Eq(input[5..].to_string())
            } else {
                return Err(format!("Could not parse name match {:}", input))
            };
        Ok(Matcher::Name(comparator))
    } else {
        todo!()
    }
}


#[cfg(test)]
mod tests {
    use super::{Package, Version, parse};
    #[test]
    fn simple_parse_test() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("1.0.0").unwrap()
        };
        assert!(parse("version=1.0.0")?.matches(&pkg));
        assert!(parse("name^=pallet-")?.matches(&pkg));
        assert!(parse("name==pallet-aura")?.matches(&pkg));
        assert!(parse("name=pallet-aura")?.matches(&pkg));
        assert!(parse("name!=frame-aura")?.matches(&pkg));
        assert!(parse("name$=-aura")?.matches(&pkg));

        Ok(())
    }
}
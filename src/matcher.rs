use semver::Version;
use std::str::FromStr;

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

pub enum VersionMatch {
    Full(Version),
    Major(u64),
    Minor(u64),
    Patch(u64),
    Pre(Comparator),
    Build(Comparator),
    Not(Box<VersionMatch>),
}

impl VersionMatch {
    pub fn matches(&self, v: &Version) -> bool {
        match &*self {
            VersionMatch::Full(f) => f == v,
            VersionMatch::Major(m) => &v.major == m,
            VersionMatch::Minor(m) => &v.minor == m,
            VersionMatch::Patch(p) => &v.patch == p,
            VersionMatch::Pre(p) => 
                p.matches(&v.pre
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(".")
                ),
            VersionMatch::Build(b) =>
                b.matches(&v.build
                    .iter()
                    .map(|i| i.to_string())
                    .collect::<Vec<_>>()
                    .join(".")),
            VersionMatch::Not(inner) => !inner.matches(v),
        }
    }
}

pub enum Matcher {
    Version(VersionMatch),
    Name(Comparator),
    And(Box<Matcher>, Box<Matcher>),
    Or(Box<Matcher>, Box<Matcher>)
}


impl Matcher {
    pub fn matches(&self, pkg: &Package) -> bool {
        match &*self {
            Matcher::Version(v) => v.matches(pkg.version()),
            Matcher::Name(comp) =>  comp.matches(pkg.name()),
            Matcher::And(a, b) => a.matches(pkg) && b.matches(pkg),
            Matcher::Or(a, b) => a.matches(pkg) || b.matches(pkg),
        }
    }
}

enum LexItem {
    Token(Vec<char>),
    OpenParen,
    CloseParen,
    And,
    Or
}

fn lex(input: &str) -> Result<Vec<LexItem>, String> {
    let mut result = Vec::new();
    let mut token = Vec::new();
    let mut it = input.chars().peekable();

    while let Some(&c) = it.peek() {
        match c {
            '&' => {
                if token.len() > 0 {
                    result.push(LexItem::Token(token));
                    token = Vec::new();
                }
                result.push(LexItem::And);
                it.next();
                if it.peek() == Some(&'&') {
                    it.next();
                }
            }
            '|' => {
                if token.len() > 0 {
                    result.push(LexItem::Token(token));
                    token = Vec::new();
                }
                result.push(LexItem::Or);
                it.next();
                if it.peek() == Some(&'|') {
                    it.next();
                }
            }
            '('  => {
                if token.len() > 0 {
                    result.push(LexItem::Token(token));
                    token = Vec::new();
                }
                result.push(LexItem::OpenParen);
                it.next();
            }
            ')'  => {
                if token.len() > 0 {
                    result.push(LexItem::Token(token));
                    token = Vec::new();
                }
                result.push(LexItem::CloseParen);
                it.next();
            }
            ' ' => {
                it.next();
            }
            c => {
                token.push(c);
                it.next();
            }
        }
    }

    // consume any remaining tokens, too
    if token.len() > 0 {
        result.push(LexItem::Token(token));
    }

    Ok(result)
}
enum Node {
    Entry(LexItem),
    Children(Vec<Node>),
}

fn traverse<I>(inp: &mut I) -> Result<Node, String>
    where I: Iterator<Item=LexItem>    
{
    let mut current_nodes = Vec::new();
    while let Some(l) = inp.next() {
        match l {
            LexItem::CloseParen => {
                return Ok(Node::Children(current_nodes))
            }
            LexItem::OpenParen => {
                current_nodes.push(traverse(inp)?);
            },
            _ => {
                current_nodes.push(Node::Entry(l));
            }
        }
    }

    Ok(Node::Children(current_nodes))
}

fn translate(inp: Node) -> Result<Matcher,String> {
    match inp {
        Node::Entry(l) =>
            if let LexItem::Token(t) =  l {
                return parse_token(t.into());
            } else {
                return Err("Item not supported".into())
            },
        Node::Children(ch) => {
            let mut it = ch.into_iter();
            let mut matchers = Vec::new();
            while let Some(n) = it.next() {
                match n {
                    Node::Entry(LexItem::Token(t)) => {
                        matchers.push(parse_token(t.to_vec())?);
                    },
                    Node::Entry(LexItem::And) => {
                        let prev = matchers.pop()
                        .ok_or("no item before and".to_string())?;
                        let next = translate(
                            it.next().ok_or("missing item after end".to_string())?
                        )?;
                        matchers.push(
                            Matcher::And(prev.into(), next.into())
                        );
                    }
                    Node::Entry(LexItem::Or) => {
                        let prev = matchers.pop().ok_or("no item before or".to_string())?;
                        let next = translate(
                            it.next().ok_or("missing item after or".to_string())?
                        )?;
                        matchers.push(
                            Matcher::Or(prev.into(), next.into())
                        );
                    }
                    Node::Entry(_) => unreachable!(),
                    _ => {
                        matchers.push(translate(n)?);
                    }
                }
            }

            if matchers.len() > 1{
                let mut it = matchers.into_iter();
                let mut cur = it.next().expect("Exists, we just checked");
                while let Some(next) = it.next() {
                    // consume all that is left
                    cur = Matcher::And(cur.into(), next.into());
                }
                Ok(cur)
            } else {
                Ok(matchers.pop().expect("Exists. We just checked"))
            }

        }
    }
}


pub fn parse(input: &str) -> Result<Matcher, String> {
    let lexed =  lex(input)?;
    let node = traverse(&mut lexed.into_iter())?;
    translate(node)
}

fn make_comparator(input: &str) -> Result<Comparator, String> {
    if input.starts_with("^=") {
        Ok(Comparator::Starts(input[2..].to_string()))
    } else if input.starts_with("$=") {
        Ok(Comparator::Ends(input[2..].to_string()))
    } else if input.starts_with("!=") {
        Ok(Comparator::Neq(input[2..].to_string()))
    } else if input.starts_with("==") {
        Ok(Comparator::Eq(input[2..].to_string()))
    } else if input.starts_with("=") {
        Ok(Comparator::Eq(input[1..].to_string()))
    } else {
        Err(format!("Could not parse comparator {:}", input))
    }
}

fn parse_u64(input: &str) -> Result<(bool, u64), String> {
    parse_into(input, |v| u64::from_str(v)
        .map_err(|e| format!("Could not parse version {:}: {:}", v, e))
    )
}

fn parse_into<F, U>(input: &str, f: F) -> Result<(bool, U), String>
    where F: Fn(&str) -> Result<U, String>
{
    if input.starts_with("!=") {
        Ok((false, f(&input[2..])?))
    } else if input.starts_with("==") {
        Ok((true, f(&input[2..])?))
    } else if input.starts_with("=") {
        Ok((true, f(&input[1..])?))
    } else {
        Err(format!("could not parse matcher {:}",  input))
    }
}


fn parse_maybe_not(pos: bool, inner: VersionMatch) -> VersionMatch {
    if pos {
        inner
    } else {
        VersionMatch::Not(Box::new(inner))
    }
}

fn parse_token(inp: Vec<char>) -> Result<Matcher, String> {
    let input: String = inp.iter().collect();
    if input.starts_with("version") {
        if inp[7] == '.' {
            if input[8..].starts_with("major") {
                parse_u64(&input[13..])
                    .map(|(p,  v)| (p, VersionMatch::Major(v)))
                    .map(|(a, b)| parse_maybe_not(a, b))
                    .map(|v| Matcher::Version(v))
            } else if input[8..].starts_with("minor") {
                parse_u64(&input[13..])
                    .map(|(p,  v)| (p, VersionMatch::Minor(v)))
                    .map(|(a, b)| parse_maybe_not(a, b))
                    .map(|v| Matcher::Version(v))
            } else if input[8..].starts_with("patch") {
                parse_u64(&input[13..])
                    .map(|(p,  v)| (p, VersionMatch::Patch(v)))
                    .map(|(a, b)| parse_maybe_not(a, b))
                    .map(|v| Matcher::Version(v))
            } else if input[8..].starts_with("pre") {
                make_comparator(&input[11..])
                    .map(|c| Matcher::Version(VersionMatch::Pre(c)))
            } else if input[8..].starts_with("build") {
                make_comparator(&input[13..])
                    .map(|c| Matcher::Version(VersionMatch::Build(c)))
            } else {
                Err(format!("Unknown version definition {:}", input))
            }

        } else {
            parse_into(&input[7..], |r|
                        Version::parse(r)
                            .map_err(|e| format!("Can't parse version {:}: {:}", r, e))
            ).map(|(pos, v)| {
                if pos {
                    VersionMatch::Full(v)
                } else {
                    VersionMatch::Not(Box::new(VersionMatch::Full(v)))
                }
            })
            .map(|v| Matcher::Version(v))
        }
    } else if input.starts_with("name") {
        make_comparator(&input[4..])
            .map(|c| Matcher::Name(c))
    } else {
        Err(format!("Unknown Token {:}", input))
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

    #[test]
    fn group_test() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("1.0.0").unwrap()
        };
        assert!(parse("(version=1.0.0 && name^=pallet-) || name==pallet-aura")?.matches(&pkg));
        assert!(parse("name=pallet-aura")?.matches(&pkg));
        assert!(parse("name!=frame-aura")?.matches(&pkg));
        assert!(parse("name$=-aura")?.matches(&pkg));

        Ok(())
    }

    #[test]
    fn version_test() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("1.0.0-dev.1+H1638").unwrap()
        };
        assert!(parse("version.pre=dev.1")?.matches(&pkg));
        assert!(parse("version.major=1")?.matches(&pkg));
        assert!(parse("version.minor=0")?.matches(&pkg));
        assert!(parse("version.patch=0")?.matches(&pkg));
        assert!(parse("version.build=H1638")?.matches(&pkg));
        assert!(parse("(version.major=1 && version.minor=0)")?.matches(&pkg));
        assert!(parse("(version.major=1 && version.minor=0 && version.patch=0)")?.matches(&pkg));
        assert!(parse("version=1.0.0-dev.1+H1638")?.matches(&pkg));
        
        assert!(!parse("version.pre=dev.2")?.matches(&pkg));
        assert!(!parse("version.major=0")?.matches(&pkg));
        assert!(!parse("version.minor=1")?.matches(&pkg));
        assert!(!parse("version.patch=1")?.matches(&pkg));
        assert!(!parse("version.build=H2638")?.matches(&pkg));
        assert!(!parse("(version.major=1 && version.minor=1)")?.matches(&pkg));
        assert!(!parse("(version.major=1 && version.minor=0 && version.patch=1)")?.matches(&pkg));
        assert!(!parse("version=1.0.0-dev.2+H2638")?.matches(&pkg));
        Ok(())
    }

    #[test]
    fn negated_version() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("2.1.1-alpha.1").unwrap()
        };
        assert!(parse("version.pre!=dev.1")?.matches(&pkg));
        assert!(parse("version.major!=1")?.matches(&pkg));
        assert!(parse("version.minor!=0")?.matches(&pkg));
        assert!(parse("version.patch!=0")?.matches(&pkg));
        assert!(parse("version.build!=H1638")?.matches(&pkg));
        assert!(parse("(version.major=2 && version.minor!=0)")?.matches(&pkg));
        assert!(parse("(version.major=2 && version.minor=1 && version.patch!=0)")?.matches(&pkg));
        assert!(parse("version!=1.0.0-dev.1+H1638")?.matches(&pkg));
        Ok(())
    }

    #[test]
    fn comparing_pre() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("2.1.1-alpha.1").unwrap()
        };
        assert!(parse("version.pre^=alpha")?.matches(&pkg));
        assert!(parse("version.pre$=pha.1")?.matches(&pkg));
        Ok(())
    }
    #[test]
    fn comparing_build() -> Result<(), String> {
        let pkg = Package {
            name: "pallet-aura".to_owned(),
            version: Version::parse("2.1.1-alpha.1+H2918").unwrap()
        };
        assert!(parse("version.build^=H29")?.matches(&pkg));
        assert!(parse("version.build$=918")?.matches(&pkg));
        Ok(())
    }
}
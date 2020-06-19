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
    And(Box<Matcher>, Box<Matcher>),
    Or(Box<Matcher>, Box<Matcher>)
}


impl Matcher {
    pub fn matches(&self, pkg: &Package) -> bool {
        match &*self {
            Matcher::Version(v) =>  pkg.version() == v,
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

impl Node {
    fn entry(l: LexItem) -> Self {
        Node::Entry(l)
    }
    fn children(v: Vec<Node>) -> Self {
        Node::Children(v)
    }
}

fn traverse<I>(inp: &mut I) -> Result<Node, String>
    where I: Iterator<Item=LexItem>    
{
    let mut current_nodes = Vec::new();
    while let Some(l) = inp.next() {
        match l {
            LexItem::CloseParen => {
                return Ok(Node::children(current_nodes))
            }
            LexItem::OpenParen => {
                current_nodes.push(traverse(inp)?);
            },
            _ => {
                current_nodes.push(Node::entry(l));
            }
        }
    }

    Ok(Node::children(current_nodes))
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
                    Node::Entry(l) => {
                        match l {
                            LexItem::Token(t) => {
                                matchers.push(parse_token(t.to_vec())?)
                            },
                            LexItem::And => {
                                let prev = matchers.pop()
                                .ok_or("no item before and".to_string())?;
                                let next = translate(
                                    it.next().ok_or("missing item after end".to_string())?
                                )?;
                                matchers.push(
                                    Matcher::And(prev.into(), next.into())
                                )
                            }
                            LexItem::Or => {
                                let prev = matchers.pop().ok_or("no item before or".to_string())?;
                                let next = translate(
                                    it.next().ok_or("missing item after or".to_string())?
                                )?;
                                matchers.push(
                                    Matcher::Or(prev.into(), next.into())
                                )
                            }
                            _ => unreachable!()
                        }
                    }
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

fn parse_token(inp: Vec<char>) -> Result<Matcher, String> {
    let input: String = inp.into_iter().collect();
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
}
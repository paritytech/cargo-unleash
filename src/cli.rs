use std::path::PathBuf;
use structopt::StructOpt;
use cargo::core::InternedString;
use semver::Identifier;

fn parse_identifiers(src: &str) -> Identifier {
    Identifier::AlphaNumeric(src.to_owned())
}

#[derive(StructOpt, Debug)]
pub enum Command {
    /// deactivate the development dependencies
    DeDevDeps,
    /// calculate the packages that should be released, in the order they should be released
    ToRelease {
        /// skip the packages named ...
        #[structopt(long, parse(from_str))]
        skip: Vec<InternedString>,
        /// ignore version pre-releases, comma separated
        #[structopt(short = "i", long="ignore-version-pre", parse(from_str = parse_identifiers), default_value = "dev git master")]
        ignore_version_pre: Vec<Identifier>,
    }
}

#[derive(Debug, StructOpt)]
#[structopt(
    name = "carg-unleash",
    about = "Release the crates of this massiv monorepo"
)]
pub struct Opt {
    /// Output file, stdout if not present
    #[structopt(long, parse(from_os_str), default_value = "Cargo.toml")]
    pub manifest_path: PathBuf,
    /// Specify the log levels
    #[structopt(long = "log-level", short = "l", default_value = "warn")]
    pub log: String,

    #[structopt(subcommand)]
    pub cmd: Command,
}

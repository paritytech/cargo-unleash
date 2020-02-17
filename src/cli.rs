use std::path::PathBuf;
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
pub enum Command {
    // deactivate the development dependencies
    DeDevDeps,
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

    #[structopt(subcommand)]
    pub cmd: Command,
}

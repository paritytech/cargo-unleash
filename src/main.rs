use std::error::Error;
use structopt::StructOpt;
mod cli;
mod commands;

use cli::Opt;

fn main() -> Result<(), Box<dyn Error>> {
    commands::run(Opt::from_args())
}

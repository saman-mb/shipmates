mod cli;
mod manifest;
mod catalog;
mod digest;
mod adapters;
mod installer;

use clap::Parser;
use cli::{Cli, Command};

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Install => println!("installing"),
        Command::Build => println!("building"),
        Command::Check => println!("checking"),
        Command::Update => println!("updating"),
        Command::Targets => println!("targets"),
    }
}

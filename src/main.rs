mod cli;
mod commands;
mod error;
mod ffmpeg;
mod planner;
mod probe;
mod ui;

use clap::Parser;

fn main() {
    if let Err(error) = cli::run(cli::Cli::parse()) {
        eprintln!("✖ {error:#}");
        std::process::exit(1);
    }
}

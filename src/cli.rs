use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "tool",
    version,
    about = "A simple, interactive media CLI powered by FFmpeg.",
    after_help = "Examples:\n  tool compress video.mp4\n  tool compress video.mp4 --size 8mb\n  tool compress video.mp4 --size 8mb -o small.mp4\n  tool compress video.mp4 --size 8mb --dry-run"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Compress {
        input: PathBuf,
        #[arg(long, value_name = "SIZE")]
        size: Option<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long, short)]
        verbose: bool,
        #[arg(long)]
        force: bool,
    },
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Compress {
            input,
            size,
            output,
            dry_run,
            verbose,
            force,
        } => crate::commands::compress::run(input, size, output, dry_run, verbose, force),
    }
}

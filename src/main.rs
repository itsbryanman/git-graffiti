use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use git_graffiti::{rewrite, words};

#[derive(Parser)]
#[command(name = "git-graffiti", version, about = "Spray paint your git history")]
struct Cli {
    #[arg(long, global = true)]
    gpu: bool,

    #[arg(long, global = true, default_value_t = default_threads())]
    threads: usize,

    #[arg(long, global = true)]
    force: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Mine a prefix onto HEAD.
    Mine { prefix: String },
    /// Rewrite the newest commits, oldest first.
    Spray {
        #[arg(long)]
        dry_run: bool,
        #[arg(required = true)]
        prefixes: Vec<String>,
    },
    /// Turn words into the hex alphabet where possible.
    Words { phrase: String },
    /// Put HEAD back at the newest graffiti backup.
    Undo,
}

fn default_threads() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let options = rewrite::MineOptions {
        threads: cli.threads,
        gpu: cli.gpu,
        force: cli.force,
    };
    match cli.command {
        Commands::Mine { prefix } => {
            rewrite::mine(&prefix, options)?;
        }
        Commands::Spray { dry_run, prefixes } => {
            rewrite::spray(&prefixes, options, dry_run)?;
        }
        Commands::Words { phrase } => {
            for line in words::lines(&phrase) {
                println!("{line}");
            }
        }
        Commands::Undo => {
            rewrite::undo()?;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

use std::fs;
use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use foyer_patch::{Manifest, diff};

/// Foyer developer tool.
#[derive(Parser)]
#[command(name = "foyer", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Hash every file in a build directory and print the manifest as JSON.
    Manifest {
        /// Build directory to scan.
        dir: PathBuf,
        /// Write the manifest to this file instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Show what changes between two builds (directories or manifest files).
    Diff { from: PathBuf, to: PathBuf },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Manifest { dir, output } => {
            let manifest = Manifest::from_dir(&dir)?;
            let json = serde_json::to_string_pretty(&manifest)?;
            match output {
                Some(path) => fs::write(&path, json)
                    .with_context(|| format!("failed to write {}", path.display()))?,
                None => println!("{json}"),
            }
        }
        Command::Diff { from, to } => {
            let from = load(&from)?;
            let to = load(&to)?;
            let plan = diff(&from, &to);
            for path in &plan.added {
                println!("+ {path}");
            }
            for path in &plan.changed {
                println!("~ {path}");
            }
            for path in &plan.removed {
                println!("- {path}");
            }
            println!(
                "{} added, {} changed, {} removed, {} unchanged; {} bytes to download",
                plan.added.len(),
                plan.changed.len(),
                plan.removed.len(),
                plan.unchanged.len(),
                plan.download_size(&to),
            );
        }
    }
    Ok(())
}

/// Accept either a build directory or a manifest JSON file.
fn load(path: &PathBuf) -> anyhow::Result<Manifest> {
    if path.is_dir() {
        return Ok(Manifest::from_dir(path)?);
    }
    let json =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("{} is not a manifest", path.display()))
}

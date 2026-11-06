use std::fs;
use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use foyer_patch::{Manifest, chunk_delta, diff};

mod api;
mod client;
mod config;
mod format;
mod push;
mod status;

use api::Platform;
use client::ApiClient;
use config::{Credentials, DEFAULT_API_URL};
use format::bytes;

/// Foyer developer tool: push game builds and check what players get.
#[derive(Parser)]
#[command(name = "foyer", version, about)]
struct Cli {
    /// Foyer API to talk to [default: saved by `login`, else https://api.foyer.lol]
    #[arg(long, env = "FOYER_API_URL", global = true)]
    api_url: Option<String>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Save an API token so other commands can use it.
    ///
    /// Create a token in the developer dashboard. In CI, skip `login` and set
    /// FOYER_TOKEN instead.
    Login {
        /// The token. Read from stdin when omitted.
        #[arg(long)]
        token: Option<String>,
    },
    /// Forget the saved token.
    Logout,
    /// Show who the current token belongs to.
    Whoami,
    /// Upload a build directory to a channel. Only chunks the server does
    /// not already have are uploaded; re-run to resume an interrupted push.
    Push {
        /// Build directory to upload.
        dir: PathBuf,
        /// Game slug, as in foyer.lol/games/<slug>.
        #[arg(long, short)]
        game: String,
        /// Channel to publish to, such as `stable` or `beta`.
        #[arg(long, short, default_value = "stable")]
        channel: String,
        /// Platform the build runs on: windows, macos, linux or web.
        #[arg(long, short, value_parser = api::parse_platform)]
        platform: Platform,
        /// Version label shown to players, such as `1.2.0`.
        #[arg(long = "build-version", short = 'v')]
        version: Option<String>,
        /// Chunks to upload at the same time.
        #[arg(long, default_value_t = 8)]
        concurrency: usize,
        /// Scan the build and print its size without contacting the API.
        #[arg(long)]
        dry_run: bool,
        /// Print the resulting build as JSON on stdout.
        #[arg(long)]
        json: bool,
    },
    /// Show the latest build on every channel of a game.
    Status {
        /// Game slug.
        game: String,
        /// Show one build instead.
        #[arg(long)]
        build: Option<String>,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
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

#[tokio::main]
async fn main() {
    if let Err(err) = run(Cli::parse()).await {
        eprintln!("error: {err}");
        let mut source = err.source();
        while let Some(cause) = source {
            eprintln!("  caused by: {cause}");
            source = cause.source();
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let config_dir = config::config_dir()?;
    let saved = Credentials::load(&config_dir)?;
    let api_url = cli
        .api_url
        .clone()
        .or_else(|| saved.api_url.clone())
        .unwrap_or_else(|| DEFAULT_API_URL.to_string());
    let env_token = std::env::var("FOYER_TOKEN").ok().filter(|t| !t.is_empty());
    let authed = || -> anyhow::Result<ApiClient> {
        let token = env_token.clone().or_else(|| saved.token.clone());
        if token.is_none() {
            bail!("not logged in; run `foyer login` or set FOYER_TOKEN");
        }
        ApiClient::new(&api_url, token)
    };

    match cli.command {
        Command::Login { token } => {
            let token = match token {
                Some(token) => token,
                None => read_token()?,
            };
            let token = token.trim().to_string();
            if token.is_empty() {
                bail!("no token given");
            }
            let user = ApiClient::new(&api_url, Some(token.clone()))?
                .me()
                .await
                .context("the token was not accepted")?;
            let creds = Credentials {
                // Remember a non-default API so later commands use it too.
                api_url: cli.api_url.or(saved.api_url),
                token: Some(token),
            };
            let path = creds.save(&config_dir)?;
            println!(
                "Logged in as {} (token saved to {})",
                user.username,
                path.display()
            );
        }
        Command::Logout => {
            if Credentials::remove(&config_dir)? {
                println!("Logged out.");
            } else {
                println!("Not logged in.");
            }
        }
        Command::Whoami => {
            let user = authed()?.me().await?;
            println!("{} ({api_url})", user.username);
        }
        Command::Push {
            dir,
            game,
            channel,
            platform,
            version,
            concurrency,
            dry_run,
            json,
        } => {
            let args = push::PushArgs {
                dir,
                game,
                channel,
                platform,
                version,
                concurrency,
                dry_run,
                json,
            };
            if dry_run {
                // No API calls, so no token needed.
                let api = ApiClient::new(&api_url, None)?;
                push::run(&api, args).await?;
            } else {
                push::run(&authed()?, args).await?;
            }
        }
        Command::Status { game, build, json } => {
            status::run(&authed()?, &game, build.as_deref(), json).await?;
        }
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
            let delta: u64 = chunk_delta(&from, &to).iter().map(|c| c.size).sum();
            println!(
                "{} added, {} changed, {} removed, {} unchanged; {} in changed files, {} in new chunks",
                plan.added.len(),
                plan.changed.len(),
                plan.removed.len(),
                plan.unchanged.len(),
                bytes(plan.download_size(&to)),
                bytes(delta),
            );
        }
    }
    Ok(())
}

fn read_token() -> anyhow::Result<String> {
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        eprint!("Paste an API token from your Foyer developer dashboard: ");
    }
    let mut line = String::new();
    stdin.lock().read_line(&mut line)?;
    Ok(line)
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

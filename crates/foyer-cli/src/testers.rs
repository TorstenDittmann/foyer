//! `foyer keys`: tester keys for unreleased games and private channels.

use clap::Subcommand;

use crate::api::{CreateTesterKey, TesterKey, validate_channel};
use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum KeysCommand {
    /// List a game's tester keys.
    List {
        /// Game slug.
        game: String,
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Create a tester key and print it with a link to share. The key is
    /// shown only once.
    Create {
        /// Game slug.
        game: String,
        /// Who the key is for, shown in the list.
        #[arg(long, short, default_value = "")]
        label: String,
        /// Channel the key unlocks. Repeat for several; all channels when
        /// left out.
        #[arg(long = "channel", short)]
        channels: Vec<String>,
        /// Days until the key stops working (1 to 365).
        #[arg(long, default_value_t = 30)]
        expires_in_days: u32,
        /// Print JSON instead of text.
        #[arg(long)]
        json: bool,
    },
    /// Revoke a tester key. Testers lose access at once.
    Revoke {
        /// Game slug.
        game: String,
        /// The key's id, from `foyer keys list`.
        id: String,
    },
}

pub async fn run(api: &ApiClient, command: KeysCommand) -> anyhow::Result<()> {
    match command {
        KeysCommand::List { game, json } => {
            let keys = api.tester_keys(&game).await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&keys)?);
            } else {
                print!("{}", render_keys(&game, &keys));
            }
        }
        KeysCommand::Create {
            game,
            label,
            channels,
            expires_in_days,
            json,
        } => {
            for channel in &channels {
                validate_channel(channel).map_err(anyhow::Error::msg)?;
            }
            let created = api
                .create_tester_key(
                    &game,
                    &CreateTesterKey {
                        label,
                        channels,
                        expires_in_days: Some(expires_in_days),
                    },
                )
                .await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&created)?);
            } else {
                let scope = if created.key.channels.is_empty() {
                    "every channel".to_string()
                } else {
                    format!("channels {}", created.key.channels.join(", "))
                };
                let expires = created
                    .key
                    .expires_at
                    .map_or("never".into(), |t| t.format("%Y-%m-%d").to_string());
                println!("Tester key for {game} ({scope}, expires {expires}):\n");
                println!("  {}\n", created.secret);
                println!("Share this link; it unlocks the game in the browser:\n");
                println!("  {}\n", created.link);
                println!("The key is shown only once. The desktop client accepts it too.");
            }
        }
        KeysCommand::Revoke { game, id } => {
            api.delete_tester_key(&game, &id).await?;
            println!("Revoked tester key {id}.");
        }
    }
    Ok(())
}

fn render_keys(game: &str, keys: &[TesterKey]) -> String {
    if keys.is_empty() {
        return format!("{game} has no tester keys. Create one with `foyer keys create {game}`.\n");
    }
    let date = |t: Option<chrono::DateTime<chrono::Utc>>| {
        t.map_or("-".to_string(), |t| t.format("%Y-%m-%d").to_string())
    };
    let header = ["ID", "KEY", "LABEL", "CHANNELS", "EXPIRES", "LAST USED"];
    let rows: Vec<[String; 6]> = keys
        .iter()
        .map(|k| {
            [
                k.id.to_string(),
                format!("{}…", k.prefix),
                if k.label.is_empty() {
                    "-".into()
                } else {
                    k.label.clone()
                },
                if k.channels.is_empty() {
                    "all".into()
                } else {
                    k.channels.join(",")
                },
                date(k.expires_at),
                date(k.last_used_at),
            ]
        })
        .collect();
    let mut widths = header.map(|h| h.chars().count());
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let line = |cells: &[&str]| {
        let mut out = cells
            .iter()
            .zip(widths)
            .map(|(cell, w)| format!("{cell:<w$}"))
            .collect::<Vec<_>>()
            .join("  ");
        out.truncate(out.trim_end().len());
        out.push('\n');
        out
    };
    let mut out = line(&header);
    for row in &rows {
        out.push_str(&line(&row.each_ref().map(String::as_str)));
    }
    out
}

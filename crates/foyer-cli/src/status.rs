//! `foyer status`: what each channel of a game currently serves.

use crate::api::{Build, ChannelStatus};
use crate::client::ApiClient;
use crate::format::bytes;

pub async fn run(
    api: &ApiClient,
    game: &str,
    build_id: Option<&str>,
    json: bool,
) -> anyhow::Result<()> {
    if let Some(id) = build_id {
        let build = api.build(id).await?;
        if json {
            println!("{}", serde_json::to_string_pretty(&build)?);
        } else {
            print!("{}", render_build(&build));
        }
        return Ok(());
    }

    let channels = api.channels(game).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&channels)?);
    } else {
        print!("{}", render_channels(game, &channels));
    }
    Ok(())
}

fn render_build(build: &Build) -> String {
    format!(
        "build      {}\ngame       {}\nchannel    {}\nplatform   {}\nversion    {}\nstate      {}\nsize       {} in {} chunks\ncreated    {}\n",
        build.id,
        build.game,
        build.channel,
        build.platform.as_str(),
        build.version.as_deref().unwrap_or("-"),
        build.state.as_str(),
        bytes(build.total_size),
        build.chunk_count,
        pushed_at(build),
    )
}

pub fn render_channels(game: &str, channels: &[ChannelStatus]) -> String {
    if channels.is_empty() {
        return format!("{game} has no builds yet. Push one with `foyer push`.\n");
    }
    let header = [
        "CHANNEL", "PLATFORM", "VERSION", "STATE", "SIZE", "BUILD", "PUSHED",
    ];
    let rows: Vec<[String; 7]> = channels
        .iter()
        .map(|c| {
            let latest = c.latest.as_ref();
            [
                c.channel.clone(),
                c.platform.as_str().to_string(),
                latest
                    .and_then(|b| b.version.clone())
                    .unwrap_or_else(|| "-".into()),
                latest.map_or("-".into(), |b| b.state.as_str().to_string()),
                latest.map_or("-".into(), |b| bytes(b.total_size)),
                latest.map_or("-".into(), |b| b.id.to_string()),
                latest.map_or("-".into(), pushed_at),
            ]
        })
        .collect();

    let mut widths = header.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.len());
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

fn pushed_at(build: &Build) -> String {
    build.created_at.format("%Y-%m-%d %H:%M UTC").to_string()
}

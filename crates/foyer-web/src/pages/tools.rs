//! Developer tooling in the dashboard: API tokens for the CLI and CI, a
//! game's download numbers, and tester keys with private channels.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};

use crate::api::{
    NewApiToken, NewTesterKey, RevokeApiToken, RevokeTesterKey, SetChannelPrivate, get_game_stats,
    get_testers, list_api_tokens,
};
use crate::components::*;
use crate::model::*;
use crate::pages::dashboard::RequireSession;

/// Tabs on a game's dashboard pages.
#[component]
pub fn GameTabs(#[prop(into)] slug: String, active: &'static str) -> impl IntoView {
    let tabs = [
        ("page", "Game page", format!("/dashboard/games/{slug}")),
        (
            "downloads",
            "Downloads",
            format!("/dashboard/games/{slug}/downloads"),
        ),
        (
            "testers",
            "Testers",
            format!("/dashboard/games/{slug}/testers"),
        ),
    ];
    view! {
        <nav aria-label="Game sections" class="mt-6 flex gap-1 border-b border-line">
            {tabs
                .into_iter()
                .map(|(id, label, href)| {
                    let current = id == active;
                    let class = if current {
                        "-mb-px border-b-2 border-accent px-3 py-2 text-sm font-medium text-ink"
                    } else {
                        "-mb-px border-b-2 border-transparent px-3 py-2 text-sm text-muted hover:text-ink"
                    };
                    view! {
                        <a href=href class=class aria-current=current.then_some("page")>
                            {label}
                        </a>
                    }
                })
                .collect_view()}
        </nav>
    }
}

/// A read-only field with a value to copy, shown once.
#[component]
fn SecretField(#[prop(into)] label: String, #[prop(into)] value: String) -> impl IntoView {
    view! {
        <div>
            <p class="label">{label}</p>
            <input
                class="field font-mono text-sm"
                readonly
                value=value
                onclick="this.select()"
                aria-label="Select to copy"
            />
        </div>
    }
}

fn show_error<T>(value: Option<Result<T, ServerFnError>>) -> Option<AnyView> {
    match value {
        Some(Err(e)) => Some(view! { <ErrorNote message=server_error_text(&e) /> }.into_any()),
        _ => None,
    }
}

// ---- API tokens --------------------------------------------------------------

#[component]
pub fn ApiTokens() -> impl IntoView {
    view! {
        <Title text="API tokens · Foyer" />
        <RequireSession children=|_| view! { <ApiTokensInner /> } />
    }
}

#[component]
fn ApiTokensInner() -> impl IntoView {
    let create = ServerAction::<NewApiToken>::new();
    let revoke = ServerAction::<RevokeApiToken>::new();
    let tokens = Resource::new(
        move || (create.version().get(), revoke.version().get()),
        |_| list_api_tokens(),
    );

    view! {
        <PageShell>
            <A href="/dashboard" attr:class="link text-sm">
                "← Dashboard"
            </A>
            <h1 class="mt-2 font-display text-3xl font-semibold">"API tokens"</h1>
            <p class="mt-2 max-w-prose text-muted">
                "Tokens let the "
                <code class="text-ink">"foyer"</code>
                " CLI and CI push builds as you. Treat them like passwords: anyone with one can push to your games."
            </p>
            <div class="mt-8 grid gap-10 lg:grid-cols-[1fr_22rem]">
                <section class="card">
                    {move || Suspend::new(async move {
                        match tokens.await {
                            Ok(list) if list.is_empty() => {
                                view! { <p class="p-6 text-muted">"No tokens yet."</p> }.into_any()
                            }
                            Ok(list) => {
                                view! {
                                    <ul class="divide-y divide-line">
                                        {list
                                            .into_iter()
                                            .map(|t| {
                                                let used = t
                                                    .last_used_at
                                                    .map_or("Never used".to_string(), |d| format!("Last used {}", date_only(&d)));
                                                let id = t.id.to_string();
                                                view! {
                                                    <li class="flex items-center gap-4 p-4">
                                                        <div class="min-w-0 flex-1">
                                                            <p class="font-medium">{t.name.clone()}</p>
                                                            <p class="text-sm text-faint">
                                                                {format!("Created {} · {used}", date_only(&t.created_at))}
                                                            </p>
                                                        </div>
                                                        <ActionForm action=revoke>
                                                            <input type="hidden" name="id" value=id.clone() />
                                                            <button type="submit" class="btn-secondary">
                                                                "Revoke"
                                                            </button>
                                                        </ActionForm>
                                                    </li>
                                                }
                                            })
                                            .collect_view()}
                                    </ul>
                                }
                                    .into_any()
                            }
                            Err(e) => {
                                view! {
                                    <div class="p-6">
                                        <ErrorNote message=server_error_text(&e) />
                                    </div>
                                }
                                    .into_any()
                            }
                        }
                    })}
                    {move || show_error(revoke.value().get())}
                </section>
                <aside class="flex flex-col gap-6">
                    <section class="card p-5">
                        <h2 class="font-display text-lg font-semibold">"New token"</h2>
                        <ActionForm action=create attr:class="mt-4 flex flex-col gap-3">
                            <div>
                                <label class="label" for="t-name">
                                    "Name"
                                </label>
                                <input
                                    id="t-name"
                                    class="field"
                                    name="name"
                                    maxlength="60"
                                    required
                                    placeholder="GitHub Actions"
                                />
                            </div>
                            <button type="submit" class="btn-primary self-start" disabled=move || create.pending().get()>
                                "Create token"
                            </button>
                        </ActionForm>
                        {move || match create.value().get() {
                            Some(Ok(created)) => {
                                view! {
                                    <div class="mt-5 flex flex-col gap-3 border-t border-line pt-5">
                                        <SecretField label="Your new token" value=created.token />
                                        <p class="hint mt-0">
                                            "Copy it now; it won't be shown again."
                                        </p>
                                    </div>
                                }
                                    .into_any()
                            }
                            other => show_error(other).unwrap_or_else(|| ().into_any()),
                        }}
                    </section>
                    <section class="card p-5 text-sm">
                        <h2 class="font-display text-lg font-semibold">"Use it"</h2>
                        <p class="mt-2 text-muted">"Install the CLI and log in:"</p>
                        <pre class="mt-2 overflow-x-auto rounded-control bg-canvas p-3 text-xs">
                            "curl -fsSL https://foyer.lol/install.sh | sh\nfoyer login"
                        </pre>
                        <p class="mt-4 text-muted">
                            "In GitHub Actions, save it as the "
                            <code class="text-ink">"FOYER_TOKEN"</code>
                            " secret and add a step:"
                        </p>
                        <pre class="mt-2 overflow-x-auto rounded-control bg-canvas p-3 text-xs">
                            "- uses: TorstenDittmann/foyer@cli-v0.1.0\n  with:\n    token: ${{ secrets.FOYER_TOKEN }}\n    game: your-game\n    platform: windows\n    path: build/windows"
                        </pre>
                    </section>
                </aside>
            </div>
        </PageShell>
    }
}

// ---- downloads -----------------------------------------------------------------

#[component]
pub fn GameDownloads() -> impl IntoView {
    view! {
        <Title text="Downloads · Foyer" />
        <RequireSession children=|_| view! { <GameDownloadsInner /> } />
    }
}

const RANGES: [i64; 3] = [7, 30, 90];

#[component]
fn GameDownloadsInner() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let days = move || {
        query
            .read()
            .get("days")
            .and_then(|d| d.parse().ok())
            .filter(|d| RANGES.contains(d))
            .unwrap_or(30)
    };
    let stats = Resource::new(move || (slug(), days()), |(s, d)| get_game_stats(s, d));

    view! {
        <PageShell>
            <A href="/dashboard" attr:class="link text-sm">
                "← Dashboard"
            </A>
            <h1 class="mt-2 font-display text-3xl font-semibold">"Downloads"</h1>
            {move || view! { <GameTabs slug=slug() active="downloads" /> }}
            <div class="mt-6 flex flex-wrap items-center justify-between gap-3">
                <p class="text-sm text-faint">
                    "Counted per day in UTC. Foyer stores no data about who downloaded."
                </p>
                <div class="flex gap-1" role="group" aria-label="Time range">
                    {move || {
                        RANGES
                            .into_iter()
                            .map(|d| {
                                let class = if d == days() { "btn-primary" } else { "btn-secondary" };
                                let href = format!("/dashboard/games/{}/downloads?days={d}", slug());
                                view! {
                                    <A href=href attr:class=class>
                                        {format!("{d} days")}
                                    </A>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </div>
            {move || Suspend::new(async move {
                match stats.await {
                    Ok(stats) => view! { <StatsView stats days=days() /> }.into_any(),
                    Err(e) => {
                        view! {
                            <div class="mt-6">
                                <ErrorNote message=server_error_text(&e) />
                            </div>
                        }
                            .into_any()
                    }
                }
            })}
        </PageShell>
    }
}

fn sum(c: &DownloadCounts) -> u64 {
    c.web + c.client_installs + c.client_updates
}

#[component]
fn Tile(label: &'static str, value: u64, #[prop(optional, into)] note: String) -> impl IntoView {
    view! {
        <div class="card p-5">
            <p class="text-sm text-faint">{label}</p>
            <p class="mt-1 font-display text-3xl font-semibold tabular-nums">{value.to_string()}</p>
            {(!note.is_empty()).then(|| view! { <p class="mt-1 text-xs text-faint">{note}</p> })}
        </div>
    }
}

#[component]
fn StatsView(stats: GameStats, days: i64) -> impl IntoView {
    let in_range: u64 = stats.daily.iter().map(|d| sum(&d.counts)).sum();
    let mut range = DownloadCounts::default();
    for d in &stats.daily {
        range.web += d.counts.web;
        range.client_installs += d.counts.client_installs;
        range.client_updates += d.counts.client_updates;
    }
    let all_time = sum(&stats.total);
    let new_players = range.web + range.client_installs;
    let empty = all_time == 0;

    view! {
        <div class="mt-6 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Tile label="Downloads" value=in_range note=format!("Last {days} days") />
            <Tile label="New players" value=new_players note="Zips and desktop installs" />
            <Tile label="Updates" value=range.client_updates note="Desktop client updates" />
            <Tile label="All time" value=all_time />
        </div>
        {if empty {
            view! {
                <div class="card mt-6 p-10 text-center">
                    <p class="text-muted">"No downloads yet."</p>
                    <p class="hint">"Downloads are counted once the game is published."</p>
                </div>
            }
                .into_any()
        } else {
            view! {
                <section class="card mt-6 p-5">
                    <h2 class="font-display text-lg font-semibold">"Downloads per day"</h2>
                    <DailyChart daily=stats.daily.clone() />
                    <details class="mt-4 text-sm">
                        <summary class="cursor-pointer text-muted">"Show as a table"</summary>
                        <DailyTable daily=stats.daily.clone() />
                    </details>
                </section>
                <div class="mt-6 grid gap-6 lg:grid-cols-2">
                    <section class="card p-5">
                        <h2 class="font-display text-lg font-semibold">{format!("Where, last {days} days")}</h2>
                        <SourceBreakdown counts=range />
                    </section>
                    <section class="card p-5">
                        <h2 class="font-display text-lg font-semibold">"By platform, all time"</h2>
                        <PlatformTable platforms=stats.platforms.clone() />
                    </section>
                </div>
            }
                .into_any()
        }}
    }
}

/// One bar per day, with the day and count on hover.
#[component]
fn DailyChart(daily: Vec<DailyDownloads>) -> impl IntoView {
    const H: f64 = 160.0;
    const GAP: f64 = 2.0;
    let n = daily.len().max(1) as f64;
    // Bars are laid out in a 1000-wide viewBox and stretched to the card.
    let w = 1000.0;
    let bar = ((w - GAP * (n - 1.0)) / n).max(1.0);
    let max = daily
        .iter()
        .map(|d| sum(&d.counts))
        .max()
        .unwrap_or(0)
        .max(1);
    let first = daily.first().map(|d| d.day.format("%b %-d").to_string());
    let last = daily.last().map(|d| d.day.format("%b %-d").to_string());
    let bars = daily
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let total = sum(&d.counts);
            let h = (total as f64 / max as f64 * (H - 4.0)).max(if total > 0 { 2.0 } else { 0.0 });
            let x = i as f64 * (bar + GAP);
            let title = format!(
                "{}: {total} downloads ({} web, {} installs, {} updates)",
                d.day.format("%a %b %-d"),
                d.counts.web,
                d.counts.client_installs,
                d.counts.client_updates
            );
            view! {
                <g class="group">
                    // Full-height hit target so thin bars are easy to hover.
                    <rect x=x y="0" width=bar height=H fill="transparent" />
                    <rect
                        x=x
                        y=H - h
                        width=bar
                        height=h
                        rx="2"
                        class="fill-muted transition-colors group-hover:fill-ink"
                    />
                    <title>{title}</title>
                </g>
            }
        })
        .collect_view();

    view! {
        <div class="mt-4">
            <div class="flex items-end justify-between text-xs text-faint tabular-nums">
                <span>{format!("{max} max")}</span>
            </div>
            <svg
                viewBox=format!("0 0 {w} {H}")
                preserveAspectRatio="none"
                class="mt-1 h-40 w-full"
                role="img"
                aria-label="Downloads per day"
            >
                <line x1="0" y1=H x2=w y2=H class="stroke-line" stroke-width="1" vector-effect="non-scaling-stroke" />
                {bars}
            </svg>
            <div class="mt-1 flex justify-between text-xs text-faint">
                <span>{first}</span>
                <span>{last}</span>
            </div>
        </div>
    }
}

#[component]
fn DailyTable(daily: Vec<DailyDownloads>) -> impl IntoView {
    view! {
        <table class="mt-3 w-full text-left tabular-nums">
            <thead class="text-faint">
                <tr>
                    <th class="py-1 font-normal">"Day"</th>
                    <th class="py-1 text-right font-normal">"Website"</th>
                    <th class="py-1 text-right font-normal">"Installs"</th>
                    <th class="py-1 text-right font-normal">"Updates"</th>
                </tr>
            </thead>
            <tbody>
                {daily
                    .into_iter()
                    .rev()
                    .map(|d| {
                        view! {
                            <tr class="border-t border-line">
                                <td class="py-1">{d.day.to_string()}</td>
                                <td class="py-1 text-right">{d.counts.web}</td>
                                <td class="py-1 text-right">{d.counts.client_installs}</td>
                                <td class="py-1 text-right">{d.counts.client_updates}</td>
                            </tr>
                        }
                    })
                    .collect_view()}
            </tbody>
        </table>
    }
}

/// Share of each source as a labelled bar.
#[component]
fn SourceBreakdown(counts: DownloadCounts) -> impl IntoView {
    let total = sum(&counts).max(1);
    let rows = [
        ("Website zips", counts.web),
        ("Desktop installs", counts.client_installs),
        ("Desktop updates", counts.client_updates),
    ];
    view! {
        <ul class="mt-4 flex flex-col gap-3 text-sm">
            {rows
                .into_iter()
                .map(|(label, n)| {
                    let pct = n as f64 / total as f64 * 100.0;
                    view! {
                        <li>
                            <div class="flex justify-between">
                                <span>{label}</span>
                                <span class="tabular-nums text-muted">
                                    {format!("{n} · {pct:.0}%")}
                                </span>
                            </div>
                            <div class="mt-1 h-1.5 rounded-full bg-raised">
                                <div class="h-1.5 rounded-full bg-muted" style=format!("width: {pct:.1}%") />
                            </div>
                        </li>
                    }
                })
                .collect_view()}
        </ul>
    }
}

#[component]
fn PlatformTable(platforms: Vec<PlatformDownloads>) -> impl IntoView {
    view! {
        <table class="mt-4 w-full text-left text-sm tabular-nums">
            <thead class="text-faint">
                <tr>
                    <th class="py-1 font-normal">"Platform"</th>
                    <th class="py-1 text-right font-normal">"Website"</th>
                    <th class="py-1 text-right font-normal">"Installs"</th>
                    <th class="py-1 text-right font-normal">"Updates"</th>
                </tr>
            </thead>
            <tbody>
                {platforms
                    .into_iter()
                    .map(|p| {
                        view! {
                            <tr class="border-t border-line">
                                <td class="py-1.5">{p.platform.label()}</td>
                                <td class="py-1.5 text-right">{p.counts.web}</td>
                                <td class="py-1.5 text-right">{p.counts.client_installs}</td>
                                <td class="py-1.5 text-right">{p.counts.client_updates}</td>
                            </tr>
                        }
                    })
                    .collect_view()}
            </tbody>
        </table>
    }
}

// ---- testers -------------------------------------------------------------------

#[component]
pub fn GameTesters() -> impl IntoView {
    view! {
        <Title text="Testers · Foyer" />
        <RequireSession children=|_| view! { <GameTestersInner /> } />
    }
}

#[component]
fn GameTestersInner() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let create = ServerAction::<NewTesterKey>::new();
    let revoke = ServerAction::<RevokeTesterKey>::new();
    let privacy = ServerAction::<SetChannelPrivate>::new();
    let page = Resource::new(
        move || {
            (
                slug(),
                create.version().get(),
                revoke.version().get(),
                privacy.version().get(),
            )
        },
        |(s, ..)| get_testers(s),
    );

    view! {
        <PageShell>
            <A href="/dashboard" attr:class="link text-sm">
                "← Dashboard"
            </A>
            <h1 class="mt-2 font-display text-3xl font-semibold">"Testers"</h1>
            {move || view! { <GameTabs slug=slug() active="testers" /> }}
            <p class="mt-6 max-w-prose text-muted">
                "Tester keys let people play before you publish, or try a private channel such as a beta next to the public release. Testers need no account: they open the link, or paste the key into the desktop client."
            </p>
            <div class="mt-8 grid gap-10 lg:grid-cols-[1fr_22rem]">
                <div class="flex flex-col gap-6">
                    {move || Suspend::new(async move {
                        match page.await {
                            Ok(p) => {
                                view! {
                                    <ChannelList slug=slug() channels=p.channels action=privacy />
                                    <KeyList slug=slug() keys=p.keys action=revoke />
                                }
                                    .into_any()
                            }
                            Err(e) => view! { <ErrorNote message=server_error_text(&e) /> }.into_any(),
                        }
                    })}
                    {move || show_error(privacy.value().get())}
                    {move || show_error(revoke.value().get())}
                </div>
                <aside>
                    <section class="card p-5">
                        <h2 class="font-display text-lg font-semibold">"New tester key"</h2>
                        <ActionForm action=create attr:class="mt-4 flex flex-col gap-3">
                            <input type="hidden" name="slug" value=slug />
                            <div>
                                <label class="label" for="k-label">
                                    "Who is it for?"
                                </label>
                                <input id="k-label" class="field" name="label" maxlength="80" placeholder="Playtest group" />
                            </div>
                            <div>
                                <label class="label" for="k-channels">
                                    "Channels"
                                </label>
                                <input id="k-channels" class="field" name="channels" placeholder="All channels" />
                                <p class="hint">"Comma separated, e.g. beta. Empty unlocks every channel."</p>
                            </div>
                            <div>
                                <label class="label" for="k-days">
                                    "Expires after"
                                </label>
                                <select id="k-days" class="field" name="expires_in_days">
                                    <option value="7">"7 days"</option>
                                    <option value="30" selected>"30 days"</option>
                                    <option value="90">"90 days"</option>
                                    <option value="365">"1 year"</option>
                                </select>
                            </div>
                            <button type="submit" class="btn-primary self-start" disabled=move || create.pending().get()>
                                "Create key"
                            </button>
                        </ActionForm>
                        {move || match create.value().get() {
                            Some(Ok(created)) => {
                                view! {
                                    <div class="mt-5 flex flex-col gap-3 border-t border-line pt-5">
                                        <SecretField label="Link to share" value=created.link />
                                        <SecretField label="Key (for the desktop client)" value=created.secret />
                                        <p class="hint mt-0">"Copy them now; they won't be shown again."</p>
                                    </div>
                                }
                                    .into_any()
                            }
                            other => show_error(other).unwrap_or_else(|| ().into_any()),
                        }}
                    </section>
                </aside>
            </div>
        </PageShell>
    }
}

#[component]
fn ChannelList(
    slug: String,
    channels: Vec<ChannelStatus>,
    action: ServerAction<SetChannelPrivate>,
) -> impl IntoView {
    view! {
        <section class="card">
            <div class="p-5 pb-0">
                <h2 class="font-display text-lg font-semibold">"Channels"</h2>
                <p class="mt-1 text-sm text-faint">
                    "Private channels are only served to tester keys that cover them. Push with "
                    <code class="text-ink">"foyer push --private"</code>
                    " to start one."
                </p>
            </div>
            {if channels.is_empty() {
                view! { <p class="p-5 text-sm text-muted">"No channels yet. Push a build first."</p> }.into_any()
            } else {
                view! {
                    <ul class="mt-3 divide-y divide-line">
                        {channels
                            .into_iter()
                            .map(|c| {
                                let version = c
                                    .latest
                                    .as_ref()
                                    .and_then(|b| b.version.clone())
                                    .unwrap_or_else(|| "no ready build".into());
                                let slug = slug.clone();
                                let channel = c.channel.clone();
                                let next = (!c.private).to_string();
                                view! {
                                    <li class="flex items-center gap-4 px-5 py-3">
                                        <div class="min-w-0 flex-1">
                                            <p class="font-medium">
                                                {c.channel.clone()}
                                                {c.private.then(|| view! {
                                                    <span class="ml-2 rounded-full bg-raised px-2 py-0.5 text-xs text-muted">"Private"</span>
                                                })}
                                            </p>
                                            <p class="text-sm text-faint">{format!("{} · {version}", c.platform.label())}</p>
                                        </div>
                                        <ActionForm action>
                                            <input type="hidden" name="slug" value=slug.clone() />
                                            <input type="hidden" name="channel" value=channel.clone() />
                                            <input type="hidden" name="private" value=next.clone() />
                                            <button type="submit" class="btn-secondary">
                                                {if c.private { "Make public" } else { "Make private" }}
                                            </button>
                                        </ActionForm>
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ul>
                }
                    .into_any()
            }}
        </section>
    }
}

#[component]
fn KeyList(
    slug: String,
    keys: Vec<TesterKey>,
    action: ServerAction<RevokeTesterKey>,
) -> impl IntoView {
    view! {
        <section class="card">
            <h2 class="p-5 pb-0 font-display text-lg font-semibold">"Tester keys"</h2>
            {if keys.is_empty() {
                view! { <p class="p-5 text-sm text-muted">"No keys yet."</p> }.into_any()
            } else {
                view! {
                    <ul class="mt-3 divide-y divide-line">
                        {keys
                            .into_iter()
                            .map(|k| {
                                let scope = if k.channels.is_empty() {
                                    "All channels".to_string()
                                } else {
                                    k.channels.join(", ")
                                };
                                let expires = k
                                    .expires_at
                                    .map_or("never expires".to_string(), |d| format!("expires {}", date_only(&d)));
                                let used = k
                                    .last_used_at
                                    .map_or("not used yet".to_string(), |d| format!("last used {}", date_only(&d)));
                                let label = if k.label.is_empty() { format!("{}…", k.prefix) } else { k.label.clone() };
                                let slug = slug.clone();
                                let id = k.id.to_string();
                                view! {
                                    <li class="flex items-center gap-4 px-5 py-3">
                                        <div class="min-w-0 flex-1">
                                            <p class="font-medium">{label}</p>
                                            <p class="text-sm text-faint">{format!("{scope} · {expires} · {used}")}</p>
                                        </div>
                                        <ActionForm action>
                                            <input type="hidden" name="slug" value=slug.clone() />
                                            <input type="hidden" name="id" value=id.clone() />
                                            <button type="submit" class="btn-secondary">
                                                "Revoke"
                                            </button>
                                        </ActionForm>
                                    </li>
                                }
                            })
                            .collect_view()}
                    </ul>
                }
                    .into_any()
            }}
        </section>
    }
}

/// Where `/tester?key=...` sends visitors whose key doesn't work.
#[component]
pub fn TesterKeyInvalid() -> impl IntoView {
    view! {
        <Title text="Tester key · Foyer" />
        <PageShell>
            <div class="card mx-auto max-w-md p-8 text-center">
                <h1 class="font-display text-2xl font-semibold">"This tester key doesn't work"</h1>
                <p class="mt-2 text-muted">
                    "It may have expired or been revoked. Ask the developer for a new link."
                </p>
                <A href="/" attr:class="btn-secondary mt-6">
                    "Browse games"
                </A>
            </div>
        </PageShell>
    }
}

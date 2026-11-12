//! Public pages: discover, browse, game page and developer profile.

use leptos::prelude::*;
use leptos_meta::{Meta, Title};
use leptos_router::components::A;
use leptos_router::hooks::{use_params_map, use_query_map};

use crate::api::{
    ReportGame, SetFollowing, SetWishlisted, get_developer, get_discover, get_game_page,
    get_player_state, list_games,
};
use crate::components::*;
use crate::model::*;

#[component]
pub fn Discover() -> impl IntoView {
    let data = Resource::new(|| (), |_| get_discover());

    view! {
        <Title text="Foyer: indie game store" />
        <Meta
            name="description"
            content="A store for indie games. Free to download, with links to support the developers directly."
        />
        <section class="border-b border-line">
            <div class="mx-auto max-w-6xl px-6 py-16">
                <h1 class="max-w-2xl font-display text-4xl font-semibold tracking-tight sm:text-5xl">
                    "A store for indie games."
                </h1>
                <p class="mt-4 max-w-2xl text-lg text-muted">
                    "Games are free to download. If you like one, support the developer directly."
                </p>
                <div class="mt-8 flex gap-3">
                    <A href="/games" attr:class="btn-primary">
                        "Browse all games"
                    </A>
                    <A href="/signin?next=/dashboard" attr:class="btn-secondary">
                        "Submit your game"
                    </A>
                </div>
            </div>
        </section>
        <PageShell>
            {move || Suspend::new(async move {
                match data.await {
                    Ok(d) => {
                        view! {
                            <section aria-labelledby="new">
                                <div class="mb-6 flex items-baseline justify-between">
                                    <h2 id="new" class="font-display text-2xl font-semibold">
                                        "New games"
                                    </h2>
                                    <A href="/games" attr:class="link text-sm">
                                        "See all"
                                    </A>
                                </div>
                                <GameGrid games=d.new_and_approved />
                            </section>
                            <section aria-labelledby="tags" class="mt-16">
                                <h2 id="tags" class="mb-4 font-display text-2xl font-semibold">
                                    "Browse by tag"
                                </h2>
                                <div class="flex flex-wrap gap-2">
                                    {d.tags.into_iter().map(|t| view! { <TagLink tag=t /> }).collect_view()}
                                </div>
                            </section>
                        }
                            .into_any()
                    }
                    Err(e) => view! { <ErrorNote message=server_error_text(&e) /> }.into_any(),
                }
            })}
        </PageShell>
    }
}

#[component]
pub fn Browse() -> impl IntoView {
    let query = use_query_map();
    let tag = move || query.read().get("tag").filter(|t| !t.is_empty());
    let q = move || query.read().get("q").filter(|q| !q.is_empty());
    let games = Resource::new(move || (tag(), q()), |(tag, q)| list_games(tag, q));
    let tags = Resource::new(|| (), |_| get_discover());

    let heading = move || match (tag(), q()) {
        (_, Some(q)) => format!("Results for “{q}”"),
        (Some(t), None) => format!("Tagged {t}"),
        (None, None) => "All games".to_string(),
    };

    view! {
        <Title text=move || format!("{} · Foyer", heading()) />
        <PageShell>
            <h1 class="font-display text-3xl font-semibold">{heading}</h1>
            <form action="/games" method="get" class="mt-6 flex max-w-xl gap-2">
                <label class="sr-only" for="browse-search">
                    "Search"
                </label>
                <input
                    id="browse-search"
                    name="q"
                    type="search"
                    placeholder="Search by title, developer or tag"
                    class="field"
                    prop:value=move || q().unwrap_or_default()
                />
                <button type="submit" class="btn-primary">
                    "Search"
                </button>
            </form>
            {move || Suspend::new(async move {
                let current = tag();
                tags.await
                    .ok()
                    .map(|d| {
                        view! {
                            <div class="mt-6 flex flex-wrap gap-2">
                                {d
                                    .tags
                                    .into_iter()
                                    .map(|t| {
                                        let active = current.as_deref() == Some(t.as_str());
                                        view! { <TagLink tag=t active /> }
                                    })
                                    .collect_view()}
                            </div>
                        }
                    })
            })}
            <div class="mt-10">
                {move || Suspend::new(async move {
                    match games.await {
                        Ok(page) if page.items.is_empty() => {
                            view! {
                                <div class="card p-10 text-center">
                                    <p class="text-muted">"No games match that yet."</p>
                                    <A href="/games" attr:class="link mt-2 inline-block text-sm">
                                        "Clear filters"
                                    </A>
                                </div>
                            }
                                .into_any()
                        }
                        Ok(page) => view! { <GameGrid games=page.items /> }.into_any(),
                        Err(e) => view! { <ErrorNote message=server_error_text(&e) /> }.into_any(),
                    }
                })}
            </div>
        </PageShell>
    }
}

#[component]
pub fn GameDetail() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let page = Resource::new(slug, get_game_page);

    view! {
        {move || Suspend::new(async move {
            match page.await {
                Ok(Some(p)) => view! { <GameView page=p /> }.into_any(),
                Ok(None) => view! { <NotFound /> }.into_any(),
                Err(e) => {
                    view! {
                        <PageShell>
                            <ErrorNote message=server_error_text(&e) />
                        </PageShell>
                    }
                        .into_any()
                }
            }
        })}
    }
}

#[component]
fn GameView(page: GamePage) -> impl IntoView {
    let GamePage {
        game,
        developer,
        downloads,
        more_from_developer,
    } = page;
    let title = format!("{} · Foyer", game.title);
    let screenshots: Vec<Media> = screenshots(&game).cloned().collect();
    let dev_href = format!("/developers/{}", developer.slug);
    let dev_name = developer.display_name.clone();
    // The game's own support links first, then the developer's.
    let mut support = game.support_links.clone();
    for l in &developer.support_links {
        if !support.iter().any(|s| s.url == l.url) {
            support.push(l.clone());
        }
    }
    let has_support = !support.is_empty();
    let has_more = !more_from_developer.is_empty();
    let has_downloads = !downloads.is_empty();
    let slug = game.slug.clone();
    // Set by the download redirect when it couldn't get a link.
    let query = use_query_map();
    let download_note = move || {
        query.read().get("download").map(|reason| {
            let text = if reason == "busy" {
                "Too many downloads from your network right now. Try again in a minute."
            } else {
                "That download isn't available right now. Try again in a moment."
            };
            view! { <ErrorNote message=text /> }
        })
    };
    let descriptors = game.content_descriptors.clone();
    let preview = game.status != GameStatus::Published;
    let paragraphs: Vec<String> = game.description.split("\n\n").map(str::to_string).collect();

    view! {
        <Title text=title />
        <Meta name="description" content=game.tagline.clone() />
        <PageShell>
            {preview
                .then(|| {
                    view! {
                        <p
                            role="status"
                            class="mb-6 rounded-control border border-info/40 bg-info/10 px-4 py-3 text-sm text-info"
                        >
                            "Private preview: a tester key unlocked this game. It isn't public yet, so please don't share the link."
                        </p>
                    }
                })}
            <div class="grid gap-10 lg:grid-cols-[1fr_20rem]">
                <div>
                    <Cover
                        seed=game.slug.clone()
                        title=game.title.clone()
                        class="aspect-[16/9] rounded-card"
                    />
                    <div class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
                        {screenshots
                            .into_iter()
                            .map(|m| {
                                view! {
                                    <Cover
                                        seed=format!("{}-{}", game.slug, m.position)
                                        title=""
                                        class="aspect-video rounded-control"
                                    />
                                }
                            })
                            .collect_view()}
                    </div>
                    <section class="mt-10">
                        <h2 class="mb-3 font-display text-xl font-semibold">"About this game"</h2>
                        <div class="flex max-w-prose flex-col gap-4 text-muted">
                            {paragraphs.into_iter().map(|p| view! { <p>{p}</p> }).collect_view()}
                        </div>
                    </section>
                    {game
                        .ai_disclosure
                        .clone()
                        .map(|d| {
                            view! {
                                <section class="mt-8">
                                    <h2 class="mb-2 font-display text-lg font-semibold">
                                        "Generative AI disclosure"
                                    </h2>
                                    <p class="max-w-prose text-muted">{d}</p>
                                </section>
                            }
                        })}
                </div>
                <aside class="order-first flex flex-col gap-6 lg:order-none">
                    <div>
                        <h1 class="font-display text-3xl font-semibold tracking-tight">
                            {game.title.clone()}
                        </h1>
                        <p class="mt-2 text-muted">{game.tagline.clone()}</p>
                        <p class="mt-3 text-sm text-faint">
                            "by "
                            <A href=dev_href.clone() attr:class="link">
                                {dev_name}
                            </A>
                        </p>
                        <div class="mt-4">
                            <PlayerActions
                                game=Some(game.slug.clone())
                                developer=developer.slug.clone()
                            />
                        </div>
                    </div>
                    <div class="card p-4">
                        <h2 class="mb-3 text-sm font-semibold">"Download for free"</h2>
{download_note}
                        <ul class="flex flex-col gap-2">
                            {downloads
                                .into_iter()
                                .map(|d| {
                                    // A plain link: the website redirects it to a fresh
                                    // download URL, so it must not go through the router.
                                    view! {
                                        <li>
                                            <a
                                                href=d.href(&slug)
                                                rel="external nofollow"
                                                class="btn-primary w-full justify-between"
                                            >
                                                <span>{d.platform.label()}</span>
                                                <span class="text-xs font-normal opacity-80">
                                                    {match &d.version {
                                                        Some(v) => format!("{v} · {}", format_size(d.size_bytes)),
                                                        None => format_size(d.size_bytes),
                                                    }}
                                                </span>
                                            </a>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                        <p class="hint">
                            {if has_downloads {
                                view! {
                                    "A zip of the game. "
                                    <A href="/app" attr:class="link">
                                        "The Foyer app"
                                    </A>
                                    " installs it and keeps it up to date."
                                }
                                    .into_any()
                            } else {
                                "No downloads yet.".into_any()
                            }}
                        </p>
                    </div>
                    <Show when=move || has_support>
                        <div class="card p-4">
                            <h2 class="text-sm font-semibold">"Support the developer"</h2>
                            <p class="mb-3 mt-1 text-sm text-faint">
                                "Foyer doesn't take a cut. Support goes straight to them."
                            </p>
                            <SupportLinks links=support.clone() />
                        </div>
                    </Show>
                    <dl class="card grid grid-cols-[auto_1fr] gap-x-4 gap-y-3 p-4 text-sm">
                        <dt class="text-faint">"Platforms"</dt>
                        <dd>
                            {game
                                .platforms
                                .iter()
                                .map(|p| p.label())
                                .collect::<Vec<_>>()
                                .join(", ")}
                        </dd>
                        <dt class="text-faint">"Released"</dt>
                        <dd>{game.published_at.as_ref().map(date_only).unwrap_or_else(|| "Unreleased".into())}</dd>
                        <dt class="text-faint">"Content"</dt>
                        <dd>
                            {if descriptors.is_empty() {
                                "No content warnings".to_string()
                            } else {
                                descriptors.iter().map(|c| c.label()).collect::<Vec<_>>().join(", ")
                            }}
                        </dd>
                        <dt class="text-faint">"Tags"</dt>
                        <dd class="flex flex-wrap gap-1.5">
                            {game.tags.iter().cloned().map(|t| view! { <TagLink tag=t /> }).collect_view()}
                        </dd>
                    </dl>
                    <ReportForm slug=game.slug.clone() />
                </aside>
            </div>
            <Show when=move || has_more>
                <section class="mt-16">
                    <h2 class="mb-6 font-display text-2xl font-semibold">
                        {format!("More from {}", developer.display_name)}
                    </h2>
                    <GameGrid games=more_from_developer.clone() />
                </section>
            </Show>
        </PageShell>
    }
    .into_any()
}

/// Lets any visitor, signed in or not, flag the game for staff.
#[component]
fn ReportForm(slug: String) -> impl IntoView {
    let action = ServerAction::<ReportGame>::new();
    let sent = move || matches!(action.value().get(), Some(Ok(())));
    view! {
        <details class="card p-4 text-sm">
            <summary class="cursor-pointer text-muted">"Report this game"</summary>
            <Show
                when=sent
                fallback=move || {
                    let slug = slug.clone();
                    view! {
                        <ActionForm action attr:class="mt-3 flex flex-col gap-3">
                            <input type="hidden" name="slug" value=slug />
                            <fieldset class="flex flex-col gap-1">
                                <legend class="label">"What's wrong?"</legend>
                                {ReportReason::ALL
                                    .into_iter()
                                    .map(|r| {
                                        view! {
                                            <label class="flex items-center gap-2">
                                                <input type="radio" name="reason" value=r.as_str() required />
                                                {r.label()}
                                            </label>
                                        }
                                    })
                                    .collect_view()}
                            </fieldset>
                            <div>
                                <label class="label" for="report-details">"Details"</label>
                                <textarea
                                    id="report-details"
                                    name="details"
                                    rows="3"
                                    maxlength="2000"
                                    class="field"
                                    placeholder="What you saw, and where"
                                ></textarea>
                            </div>
                            <ActionError value=move || action.value().get() />
                            <button type="submit" class="btn-secondary" disabled=move || action.pending().get()>
                                "Send report"
                            </button>
                            <p class="hint">
                                "A reviewer reads every report. You don't need an account, and the developer won't see who reported."
                            </p>
                        </ActionForm>
                    }
                }
            >
                <p role="status" class="mt-3 text-good">
                    "Thanks. A reviewer will take a look."
                </p>
            </Show>
        </details>
    }
}

#[component]
pub fn DeveloperPage() -> impl IntoView {
    let params = use_params_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let profile = Resource::new(slug, get_developer);

    view! {
        {move || Suspend::new(async move {
            match profile.await {
                Ok(Some(p)) => view! { <DeveloperView profile=p /> }.into_any(),
                Ok(None) => view! { <NotFound /> }.into_any(),
                Err(e) => {
                    view! {
                        <PageShell>
                            <ErrorNote message=server_error_text(&e) />
                        </PageShell>
                    }
                        .into_any()
                }
            }
        })}
    }
}

#[component]
fn DeveloperView(profile: DeveloperProfile) -> impl IntoView {
    let DeveloperProfile { developer, games } = profile;
    let count = games.len();
    let mut links = developer.support_links.clone();
    if let Some(url) = &developer.website_url {
        links.push(SupportLink {
            kind: SupportKind::Website,
            url: url.clone(),
            label: None,
        });
    }

    view! {
        <Title text=format!("{} · Foyer", developer.display_name) />
        <Meta name="description" content=developer.bio.clone() />
        <PageShell>
            <header class="flex flex-col gap-6 border-b border-line pb-10 sm:flex-row sm:items-center">
                <Cover
                    seed=developer.slug.clone()
                    title=""
                    class="size-24 shrink-0 rounded-full"
                />
                <div class="flex-1">
                    <h1 class="font-display text-3xl font-semibold">{developer.display_name.clone()}</h1>
                    <p class="mt-2 max-w-prose text-muted">{developer.bio.clone()}</p>
                    <p class="mt-2 text-sm text-faint">
                        {format!("{count} game{} on Foyer", if count == 1 { "" } else { "s" })}
                    </p>
                    <div class="mt-4">
                        <PlayerActions game=None developer=developer.slug.clone() />
                    </div>
                </div>
                <SupportLinks links />
            </header>
            <section class="mt-10">
                <h2 class="mb-6 font-display text-2xl font-semibold">"Games"</h2>
                {if games.is_empty() {
                    view! { <p class="text-muted">"No public games yet."</p> }.into_any()
                } else {
                    view! { <GameGrid games /> }.into_any()
                }}
            </section>
        </PageShell>
    }
    .into_any()
}

/// Wishlist and follow buttons for the signed-in visitor, or a sign-in link.
/// Loaded separately from the page, which is the same for everyone.
#[component]
fn PlayerActions(game: Option<String>, developer: String) -> impl IntoView {
    let wish = ServerAction::<SetWishlisted>::new();
    let follow = ServerAction::<SetFollowing>::new();
    let state = {
        let game = game.clone();
        let developer = developer.clone();
        Resource::new(
            move || (wish.version().get(), follow.version().get()),
            move |_| get_player_state(game.clone(), developer.clone()),
        )
    };
    let next = match &game {
        Some(slug) => format!("/games/{slug}"),
        None => format!("/developers/{developer}"),
    };

    view! {
        {move || {
            let game = game.clone();
            let developer = developer.clone();
            let next = next.clone();
            Suspend::new(async move {
                let state = state.await.unwrap_or_default();
                if !state.signed_in {
                    let text = if game.is_some() {
                        "Sign in to add it to your wishlist or follow the developer."
                    } else {
                        "Sign in to follow them and hear about new releases."
                    };
                    return view! {
                        <p class="text-sm text-faint">
                            <A href=format!("/signin?next={next}") attr:class="link">
                                {text}
                            </A>
                        </p>
                    }
                        .into_any();
                }
                let wish_button = game
                    .map(|slug| {
                        let on = !state.wishlisted;
                        view! {
                            <ActionForm action=wish>
                                <input type="hidden" name="slug" value=slug />
                                <input type="hidden" name="on" value=on.to_string() />
                                <button
                                    type="submit"
                                    class=if on { "btn-secondary" } else { "btn-secondary text-good" }
                                    aria-pressed=(!on).to_string()
                                    disabled=move || wish.pending().get()
                                >
                                    {if on { "Add to wishlist" } else { "On your wishlist ✓" }}
                                </button>
                            </ActionForm>
                        }
                    });
                let follow_button = (!state.own_profile)
                    .then(|| {
                        let on = !state.following;
                        view! {
                            <ActionForm action=follow>
                                <input type="hidden" name="slug" value=developer />
                                <input type="hidden" name="on" value=on.to_string() />
                                <button
                                    type="submit"
                                    class=if on { "btn-secondary" } else { "btn-secondary text-good" }
                                    aria-pressed=(!on).to_string()
                                    disabled=move || follow.pending().get()
                                >
                                    {if on { "Follow developer" } else { "Following ✓" }}
                                </button>
                            </ActionForm>
                        }
                    });
                view! {
                    <div class="flex flex-wrap gap-2">{wish_button} {follow_button}</div>
                    <ActionError value=move || wish.value().get() />
                    <ActionError value=move || follow.value().get() />
                }
                    .into_any()
            })
        }}
    }
}

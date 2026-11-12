//! Public pages: discover, browse, game page and developer profile.

use leptos::prelude::*;
use leptos_meta::{Meta, Title};
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
        <Title text="Foyer: hand-picked indie games" />
        <Meta
            name="description"
            content="A curated showcase of indie games. Every game is reviewed, free to download, and links straight to the people who made it."
        />
        <section class="border-b border-line">
            <div class="mx-auto max-w-6xl px-6 py-16">
                <h1 class="max-w-2xl font-display text-4xl font-semibold tracking-tight sm:text-5xl">
                    "Hand-picked indie games, free to play."
                </h1>
                <p class="mt-4 max-w-2xl text-lg text-muted">
                    "Every game on Foyer was played and approved by a person. No asset flips, no spam, no adult content. "
                    "Download for free, and support the developers directly if you like what they made."
                </p>
                <div class="mt-8 flex gap-3">
                    <a href="/games" class="btn-primary">
                        "Browse all games"
                    </a>
                    <a href="/signin?next=/dashboard" class="btn-secondary">
                        "Submit your game"
                    </a>
                </div>
            </div>
        </section>
        <PageShell>
            <Suspense fallback=Loading>
                {move || Suspend::new(async move {
                    match data.await {
                        Ok(d) => {
                            view! {
                                <section aria-labelledby="new">
                                    <div class="mb-6 flex items-baseline justify-between">
                                        <h2 id="new" class="font-display text-2xl font-semibold">
                                            "New and approved"
                                        </h2>
                                        <a href="/games" class="link text-sm">
                                            "See all"
                                        </a>
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
            </Suspense>
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
            <Suspense fallback=|| ()>
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
            </Suspense>
            <div class="mt-10">
                <Suspense fallback=Loading>
                    {move || Suspend::new(async move {
                        match games.await {
                            Ok(page) if page.items.is_empty() => {
                                view! {
                                    <div class="card p-10 text-center">
                                        <p class="text-muted">"No games match that yet."</p>
                                        <a href="/games" class="link mt-2 inline-block text-sm">
                                            "Clear filters"
                                        </a>
                                    </div>
                                }
                                    .into_any()
                            }
                            Ok(page) => view! { <GameGrid games=page.items /> }.into_any(),
                            Err(e) => view! { <ErrorNote message=server_error_text(&e) /> }.into_any(),
                        }
                    })}
                </Suspense>
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
        <Suspense fallback=Loading>
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
        </Suspense>
    }
}

/// A game's public page. `owner_preview` is set on the developer's preview
/// in the dashboard, which shows unpublished games too.
#[component]
pub fn GameView(page: GamePage, #[prop(optional)] owner_preview: bool) -> impl IntoView {
    let GamePage {
        game,
        developer,
        downloads,
        more_from_developer,
    } = page;
    let title = format!("{} · Foyer", game.title);
    let screenshots: Vec<Media> = screenshots(&game).cloned().collect();
    let dev_href = format!("/developers/{}", developer.slug);
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
    let preview = !owner_preview && game.status != GameStatus::Published;
    // The developer's page design, turned into colours that stay readable.
    let palette = Palette::for_style(&game.page);
    let page_style =
        (game.page.background.is_some() || game.page.accent.is_some()).then(|| palette.css_vars());
    let hero = game
        .page
        .hero
        .and_then(|id| media_by_id(&game, id).cloned());
    let logo = game
        .page
        .logo
        .and_then(|id| media_by_id(&game, id).cloned())
        .filter(has_image);
    let title_class = match game.page.title_style {
        TitleStyle::Standard => "font-display text-3xl font-semibold tracking-tight",
        TitleStyle::Uppercase => {
            "font-display text-4xl font-bold uppercase leading-none tracking-wide"
        }
    };
    let blocks = game
        .page
        .layout()
        .into_iter()
        .map(|block| view! { <PageBlockView block game=game.clone() screenshots=screenshots.clone() /> })
        .collect_view();

    view! {
        <Title text=title />
        <Meta name="description" content=game.tagline.clone() />
        <div class="bg-canvas text-ink" style=page_style>
        {hero
            .map(|h| {
                view! {
                    <div class="relative h-56 overflow-hidden sm:h-80 lg:h-[26rem]">
                        <GameImage media=Some(h) seed=format!("{}-hero", game.slug) alt="" class="absolute inset-0 h-full w-full" />
                        <div class="absolute inset-0 bg-linear-to-b from-transparent via-canvas/30 to-canvas"></div>
                    </div>
                }
            })}
        <PageShell>
            {owner_preview
                .then(|| {
                    view! {
                        <p
                            role="status"
                            class="mb-6 rounded-control border border-info/40 bg-info/10 px-4 py-3 text-sm text-info"
                        >
                            "Preview: this is how players will see your page. Downloads and support links work as they will once it's public."
                        </p>
                    }
                })}
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
                <div class="flex flex-col gap-10">
                    {blocks}
                    {game
                        .ai_disclosure
                        .clone()
                        .map(|d| {
                            view! {
                                <section>
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
                        <h1 class=title_class>
                            {match logo {
                                Some(l) => view! {
                                    <img src=l.url alt=game.title.clone() class="max-h-32 w-auto max-w-full object-contain" />
                                }
                                    .into_any(),
                                None => game.title.clone().into_any(),
                            }}
                        </h1>
                        <p class="mt-2 text-muted">{game.tagline.clone()}</p>
                        <p class="mt-3 text-sm text-faint">
                            "by "
                            <a href=dev_href.clone() class="link">
                                {developer.display_name.clone()}
                            </a>
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
                                "A zip of the game. The Foyer app installs it and keeps it up to date."
                            } else {
                                "No downloads yet."
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
        </div>
    }
    .into_any()
}

/// Escapes text for use inside HTML attributes and elements.
fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Splits plain text into paragraphs at blank lines.
fn paragraphs(text: &str) -> Vec<String> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// One block of a game page's main column.
#[component]
fn PageBlockView(block: PageBlock, game: Game, screenshots: Vec<Media>) -> impl IntoView {
    match block {
        PageBlock::Screenshots => {
            let mut shots = screenshots.into_iter();
            let first = shots.next();
            let rest: Vec<Media> = shots.collect();
            view! {
                <section aria-label="Screenshots">
                    <GameImage
                        media=first
                        seed=game.slug.clone()
                        alt=format!("Screenshot of {}", game.title)
                        class="aspect-[16/9] w-full rounded-card"
                    />
                    <div class="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-4">
                        {rest
                            .into_iter()
                            .map(|m| {
                                view! {
                                    <GameImage
                                        seed=format!("{}-{}", game.slug, m.position)
                                        alt=format!("Screenshot of {}", game.title)
                                        class="aspect-video w-full rounded-control"
                                        media=Some(m)
                                    />
                                }
                            })
                            .collect_view()}
                    </div>
                </section>
            }
            .into_any()
        }
        PageBlock::About => view! {
            <section>
                <h2 class="mb-3 font-display text-xl font-semibold">"About this game"</h2>
                <div class="flex max-w-prose flex-col gap-4 text-muted">
                    {paragraphs(&game.description).into_iter().map(|p| view! { <p>{p}</p> }).collect_view()}
                </div>
            </section>
        }
        .into_any(),
        PageBlock::Text { heading, body } => view! {
            <section>
                {(!heading.is_empty())
                    .then(|| view! { <h2 class="mb-3 font-display text-xl font-semibold">{heading}</h2> })}
                <div class="flex max-w-prose flex-col gap-4 text-muted">
                    {paragraphs(&body).into_iter().map(|p| view! { <p>{p}</p> }).collect_view()}
                </div>
            </section>
        }
        .into_any(),
        PageBlock::Features { heading, items } => view! {
            <section>
                {(!heading.is_empty())
                    .then(|| view! { <h2 class="mb-3 font-display text-xl font-semibold">{heading}</h2> })}
                <ul class="flex max-w-prose list-disc flex-col gap-2 pl-5 text-muted marker:text-accent">
                    {items.into_iter().map(|i| view! { <li>{i}</li> }).collect_view()}
                </ul>
            </section>
        }
        .into_any(),
        PageBlock::Quote { text, source } => view! {
            <figure class="max-w-prose border-l-4 border-accent pl-5">
                <blockquote class="font-display text-2xl font-semibold leading-snug">
                    <p>{format!("“{text}”")}</p>
                </blockquote>
                {(!source.is_empty())
                    .then(|| view! { <figcaption class="mt-2 text-sm text-muted">{format!("— {source}")}</figcaption> })}
            </figure>
        }
        .into_any(),
        PageBlock::Image { media, caption } => {
            let image = media_by_id(&game, media).cloned();
            view! {
                <figure>
                    <GameImage
                        media=image
                        seed=format!("{}-{media}", game.slug)
                        alt=if caption.is_empty() { format!("Art from {}", game.title) } else { caption.clone() }
                        class="w-full rounded-card"
                    />
                    {(!caption.is_empty())
                        .then(|| view! { <figcaption class="mt-2 text-sm text-muted">{caption}</figcaption> })}
                </figure>
            }
            .into_any()
        }
        PageBlock::Trailer { url } => match trailer_embed(&url) {
            Some(src) => {
                // Written as markup because Leptos has no typed `loading`
                // attribute for iframes. `src` is built by `trailer_embed`
                // from a fixed host and a checked id; the title is escaped.
                let iframe = format!(
                    r#"<iframe src="{src}" title="{}" class="aspect-video w-full rounded-card bg-surface" loading="lazy" referrerpolicy="strict-origin-when-cross-origin" allow="encrypted-media; picture-in-picture; fullscreen" allowfullscreen></iframe>"#,
                    escape_html(&format!("Trailer for {}", game.title))
                );
                view! { <section aria-label="Trailer" inner_html=iframe></section> }
            }
            .into_any(),
            None => ().into_any(),
        },
    }
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
        <Suspense fallback=Loading>
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
        </Suspense>
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
        <Suspense fallback=|| ()>
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
                                <a href=format!("/signin?next={next}") class="link">
                                    {text}
                                </a>
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
        </Suspense>
    }
}

#[cfg(all(test, feature = "ssr"))]
mod tests {
    use leptos::prelude::*;
    use leptos_router::components::Router;
    use leptos_router::location::RequestUrl;

    use super::*;

    fn render(page: GamePage) -> String {
        // Resources need an executor, as they have in the server.
        let _ = any_spawner::Executor::init_tokio();
        let owner = Owner::new();
        owner.set();
        provide_context(RequestUrl::new("/games/brine"));
        leptos_meta::provide_meta_context();
        view! {
            <Router>
                <GameView page />
            </Router>
        }
        .to_html()
    }

    fn sample(page_style: PageStyle) -> GamePage {
        let store = crate::mock::store();
        let mut game = store.game("brine", None).expect("sample game");
        game.page = page_style;
        let developer = store.developer(&game.developer.slug).expect("developer");
        GamePage {
            game,
            developer,
            downloads: Vec::new(),
            more_from_developer: Vec::new(),
        }
    }

    #[tokio::test]
    async fn renders_the_developers_design_on_the_server() {
        let mut page = sample(PageStyle::default());
        let art = Media {
            id: uuid::Uuid::from_u128(42),
            kind: MediaKind::Artwork,
            url: "https://media.example/logo.png".into(),
            position: 0,
        };
        page.game.media.push(art.clone());
        page.game.page = PageStyle {
            background: Some("#f4ecd8".into()),
            accent: Some("#b0302a".into()),
            logo: Some(art.id),
            blocks: vec![
                PageBlock::Quote {
                    text: "A small gem".into(),
                    source: "A player".into(),
                },
                PageBlock::Trailer {
                    url: "https://youtu.be/dQw4w9WgXcQ".into(),
                },
                PageBlock::About,
            ],
            ..Default::default()
        };
        let html = render(page);
        // Colours arrive as CSS variables on the page wrapper, already
        // adjusted for readability (dark text on the light background).
        assert!(html.contains("--color-canvas:#f4ecd8;"), "{html}");
        assert!(html.contains("--color-accent:#b0302a;"));
        assert!(html.contains("--color-ink:#141416;"));
        // The logo replaces the title text, which stays as its alt text.
        assert!(html.contains(r#"src="https://media.example/logo.png" alt="Brine""#));
        // Blocks in the developer's order; the screenshots block was left out.
        let quote = html.find("A small gem").expect("quote");
        let trailer = html
            .find("youtube-nocookie.com/embed/dQw4w9WgXcQ")
            .expect("trailer");
        let about = html.find("About this game").expect("about");
        assert!(quote < trailer && trailer < about);
        assert!(!html.contains(r#"aria-label="Screenshots""#));
    }

    #[tokio::test]
    async fn standard_pages_keep_the_store_look() {
        let html = render(sample(PageStyle::default()));
        assert!(!html.contains("--color-canvas"));
        let shots = html.find(r#"aria-label="Screenshots""#).expect("gallery");
        let about = html.find("About this game").expect("about");
        assert!(shots < about);
    }

    #[tokio::test]
    async fn escapes_titles_in_trailer_markup() {
        let mut page = sample(PageStyle {
            blocks: vec![PageBlock::Trailer {
                url: "https://youtu.be/dQw4w9WgXcQ".into(),
            }],
            ..Default::default()
        });
        page.game.title = r#"Brine" onload="alert(1)"#.into();
        let html = render(page);
        assert!(!html.contains(r#"onload="alert(1)""#));
        assert!(html.contains("Brine&quot; onload=&quot;alert(1)"));
    }
}

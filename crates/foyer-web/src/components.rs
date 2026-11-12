//! Small building blocks shared by the pages.

use leptos::prelude::*;
use leptos_router::components::A;
use leptos_router::hooks::use_location;

use crate::api::{SignOut, get_session};
use crate::model::*;

/// The signed-in account, refetched on every navigation so sign-in and
/// sign-out show up in the header straight away. Signing out also
/// refetches it, since its redirect to `/` is not a navigation when the
/// visitor is already there.
pub type SessionResource = Resource<Result<Option<Session>, ServerFnError>>;

pub fn provide_session() {
    let location = use_location();
    let sign_out = ServerAction::<SignOut>::new();
    let session: SessionResource = Resource::new(
        move || (location.pathname.get(), sign_out.version().get()),
        |_| async { get_session().await },
    );
    provide_context(session);
    provide_context(sign_out);
}

pub fn use_session() -> SessionResource {
    expect_context::<SessionResource>()
}

#[component]
pub fn Header() -> impl IntoView {
    let session = use_session();
    let sign_out = expect_context::<ServerAction<SignOut>>();

    view! {
        <header class="bg-chrome">
            <nav class="mx-auto flex max-w-6xl items-center gap-6 px-6 py-4">
                <A href="/" attr:class="font-display text-[22px] font-bold tracking-wide uppercase">
                    "Foyer"
                </A>
                <A href="/games" attr:class="text-sm text-muted hover:text-ink">
                    "Browse"
                </A>
                <A href="/app" attr:class="hidden text-sm text-muted hover:text-ink sm:inline">
                    "Get the app"
                </A>
                <form action="/games" method="get" class="ml-auto hidden sm:block">
                    <label class="sr-only" for="header-search">
                        "Search games"
                    </label>
                    <input
                        id="header-search"
                        name="q"
                        type="search"
                        placeholder="Search games"
                        class="field w-56 py-1.5 text-sm"
                    />
                </form>
                <Transition fallback=|| view! { <span class="ml-auto w-20 sm:ml-0"></span> }>
                    {move || Suspend::new(async move {
                        match session.await {
                            Ok(Some(s)) => {
                                let staff = s.user.role.is_staff();
                                let has_developer = s.developer.is_some();
                                view! {
                                    <div class="ml-auto flex items-center gap-4 sm:ml-0">
                                        <Show when=move || staff>
                                            <A href="/review" attr:class="text-sm text-muted hover:text-ink">
                                                "Review"
                                            </A>
                                        </Show>
                                        <Show when=move || has_developer>
                                            <A href="/dashboard" attr:class="text-sm text-muted hover:text-ink">
                                                "Dashboard"
                                            </A>
                                        </Show>
                                        <A href="/account" attr:class="text-sm text-muted hover:text-ink">
                                            "Account"
                                        </A>
                                        <span class="hidden text-sm text-faint md:inline">
                                            {s.user.username}
                                        </span>
                                        <ActionForm action=sign_out>
                                            <button type="submit" class="text-sm text-muted hover:text-ink">
                                                "Sign out"
                                            </button>
                                        </ActionForm>
                                    </div>
                                }
                                    .into_any()
                            }
                            _ => {
                                view! {
                                    <A href="/signin" attr:class="btn-secondary ml-auto py-1.5 sm:ml-0">
                                        "Sign in"
                                    </A>
                                }
                                    .into_any()
                            }
                        }
                    })}
                </Transition>
            </nav>
        </header>
    }
}

#[component]
pub fn Footer() -> impl IntoView {
    view! {
        <footer class="mt-24 border-t border-line">
            <div class="mx-auto flex max-w-6xl flex-col gap-2 px-6 py-8 text-sm text-faint sm:flex-row sm:justify-between">
                <p>"Foyer. A store for indie games."</p>
                <p class="flex gap-4">
                    <A href="/app" attr:class="hover:text-ink">
                        "Desktop app"
                    </A>
                    <A href="/signin?next=/dashboard" attr:class="hover:text-ink">
                        "For developers"
                    </A>
                </p>
            </div>
        </footer>
    }
}

/// Placeholder cover art: a gradient seeded from the game's slug.
#[component]
pub fn Cover(
    #[prop(into)] seed: String,
    #[prop(into)] title: String,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    let hue = placeholder_hue(&seed);
    view! {
        <div
            class=format!("cover flex items-end p-4 {class}")
            style=format!("--hue: {hue}")
            role="img"
            aria-label=format!("Cover art for {title}")
        >
            <span class="font-display text-lg font-semibold text-white/90 drop-shadow">
                {title.clone()}
            </span>
        </div>
    }
}

/// An uploaded image, or placeholder art while it has no loadable URL.
/// `alt` empty marks it decorative.
#[component]
pub fn GameImage(
    media: Option<Media>,
    #[prop(into)] seed: String,
    #[prop(into)] alt: String,
    #[prop(optional, into)] class: String,
) -> impl IntoView {
    match media.filter(has_image) {
        Some(m) => view! {
            <img src=m.url alt=alt class=format!("object-cover {class}") loading="lazy" decoding="async" />
        }
        .into_any(),
        None => view! { <Cover seed title="" class /> }.into_any(),
    }
}

#[component]
pub fn GameCard(game: Game) -> impl IntoView {
    let href = format!("/games/{}", game.slug);
    view! {
        <A href=href attr:class="card group block overflow-hidden border border-transparent transition-colors hover:border-accent">
            <Cover seed=game.slug.clone() title=game.title.clone() class="aspect-[16/9]" />
            <div class="flex flex-col gap-2 p-4">
                <p class="text-sm text-muted">{game.tagline}</p>
                <p class="text-xs text-faint">{game.developer.display_name}</p>
                <div class="flex flex-wrap gap-1.5">
                    {game.tags.into_iter().take(3).map(|t| view! { <TagChip tag=t /> }).collect_view()}
                </div>
            </div>
        </A>
    }
}

#[component]
pub fn GameGrid(games: Vec<Game>) -> impl IntoView {
    view! {
        <div class="grid gap-6 sm:grid-cols-2 lg:grid-cols-3">
            {games.into_iter().map(|game| view! { <GameCard game /> }).collect_view()}
        </div>
    }
}

/// A tag as plain text, for use inside cards that are already links.
#[component]
pub fn TagChip(#[prop(into)] tag: String) -> impl IntoView {
    view! { <span class="rounded-full bg-raised px-2.5 py-0.5 text-xs text-muted">{tag}</span> }
}

/// A tag as a link, for places that aren't already inside a link.
#[component]
pub fn TagLink(#[prop(into)] tag: String, #[prop(optional)] active: bool) -> impl IntoView {
    let href = if active {
        "/games".to_string()
    } else {
        format!("/games?tag={tag}")
    };
    let class = if active {
        "rounded-full bg-accent px-3 py-1 text-sm font-medium text-accent-ink"
    } else {
        "rounded-full bg-raised px-3 py-1 text-sm text-muted hover:text-ink"
    };
    view! {
        <A href=href attr:class=class>
            {tag}
        </A>
    }
}

#[component]
pub fn StatusBadge(status: GameStatus) -> impl IntoView {
    let tone = match status {
        GameStatus::Draft => "bg-raised text-muted",
        GameStatus::InReview => "bg-info/15 text-info",
        GameStatus::ChangesRequested => "bg-warn/15 text-warn",
        GameStatus::Published => "bg-good/15 text-good",
        GameStatus::Rejected | GameStatus::Delisted => "bg-bad/15 text-bad",
    };
    view! {
        <span class=format!("rounded-full px-2.5 py-0.5 text-xs font-medium {tone}")>
            {status.label()}
        </span>
    }
}

#[component]
pub fn SupportLinks(links: Vec<SupportLink>) -> impl IntoView {
    view! {
        <div class="flex flex-wrap gap-2">
            {links
                .into_iter()
                .map(|l| {
                    let text = support_link_text(&l);
                    view! {
                        <a
                            href=l.url
                            target="_blank"
                            rel="noopener noreferrer"
                            class="btn-support"
                        >
                            {text}
                        </a>
                    }
                })
                .collect_view()}
        </div>
    }
}

#[component]
pub fn PageShell(children: Children) -> impl IntoView {
    view! { <main class="mx-auto max-w-6xl px-6 py-10">{children()}</main> }
}

#[component]
pub fn Loading() -> impl IntoView {
    view! { <p class="py-12 text-center text-faint">"Loading…"</p> }
}

#[component]
pub fn ErrorNote(#[prop(into)] message: String) -> impl IntoView {
    view! {
        <p role="alert" class="rounded-control border border-bad/40 bg-bad/10 px-4 py-3 text-sm text-bad">
            {message}
        </p>
    }
}

/// Shows a server action's error, if its last run failed.
#[component]
pub fn ActionError(
    #[prop(into)] value: Signal<Option<Result<(), ServerFnError>>>,
) -> impl IntoView {
    move || match value.get() {
        Some(Err(e)) => Some(view! { <ErrorNote message=server_error_text(&e) /> }),
        _ => None,
    }
}

/// The human-readable part of a server function error.
pub fn server_error_text(e: &ServerFnError) -> String {
    match e {
        ServerFnError::ServerError(msg) => msg.clone(),
        other => other.to_string(),
    }
}

#[component]
pub fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    {
        if let Some(resp) = use_context::<leptos_axum::ResponseOptions>() {
            resp.set_status(axum::http::StatusCode::NOT_FOUND);
        }
    }
    view! {
        <PageShell>
            <div class="py-16 text-center">
                <h1 class="font-display text-3xl font-semibold">"Page not found"</h1>
                <p class="mt-3 text-muted">"That page doesn't exist, or isn't public yet."</p>
                <a class="btn-secondary mt-6" href="/games">
                    "Browse games"
                </a>
            </div>
        </PageShell>
    }
}

//! Developer dashboard: developer profile setup, game list, game page editor,
//! submitting for review, appeals and strikes.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::{use_location, use_params_map, use_query_map};

use crate::api::{
    CreateDeveloper, FileAppeal, GameForm, SaveGame, SubmitForReview, my_game, my_games, my_strikes,
};
use crate::components::*;
use crate::model::Submission;
use crate::model::*;
use crate::pages::auth::VerifyEmailBanner;

/// Renders `children` with the signed-in session, or a sign-in prompt.
#[component]
pub fn RequireSession<F, V>(children: F) -> impl IntoView
where
    F: Fn(Session) -> V + Send + Sync + 'static,
    V: IntoView + 'static,
{
    let session = use_session();
    let location = use_location();
    let children = std::sync::Arc::new(children);

    view! {
        <Suspense fallback=Loading>
            {move || {
                let children = children.clone();
                Suspend::new(async move {
                    match session.await {
                        Ok(Some(s)) => children(s).into_any(),
                        Ok(None) => {
                            let next = location.pathname.get_untracked();
                            view! {
                                <PageShell>
                                    <div class="card mx-auto max-w-md p-8 text-center">
                                        <h1 class="font-display text-2xl font-semibold">
                                            "Sign in to continue"
                                        </h1>
                                        <p class="mt-2 text-muted">
                                            "The dashboard is where developers manage their games."
                                        </p>
                                        <a href=format!("/signin?next={next}") class="btn-primary mt-6">
                                            "Sign in"
                                        </a>
                                    </div>
                                </PageShell>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! {
                                <PageShell>
                                    <ErrorNote message=server_error_text(&e) />
                                </PageShell>
                            }
                                .into_any()
                        }
                    }
                })
            }}
        </Suspense>
    }
}

#[component]
pub fn Dashboard() -> impl IntoView {
    view! {
        <Title text="Dashboard · Foyer" />
        <RequireSession children=|session: Session| {
            let banner = (!session.user.email_verified)
                .then(|| {
                    view! {
                        <div class="mx-auto max-w-6xl px-6 pt-10 [&>div]:mb-0">
                            <VerifyEmailBanner email=session.user.email.clone() />
                        </div>
                    }
                });
            let page = match session.developer {
                Some(dev) => view! { <GameList developer=dev /> }.into_any(),
                None => view! { <CreateDeveloperForm /> }.into_any(),
            };
            view! {
                {banner}
                {page}
            }
        } />
    }
}

#[component]
fn CreateDeveloperForm() -> impl IntoView {
    let action = ServerAction::<CreateDeveloper>::new();
    view! {
        <PageShell>
            <div class="mx-auto max-w-xl">
                <h1 class="font-display text-3xl font-semibold">"Set up your developer profile"</h1>
                <p class="mt-2 text-muted">
                    "This is the public page players see next to your games. You can be a studio or just you."
                </p>
                <ActionForm action attr:class="card mt-8 flex flex-col gap-5 p-6">
                    <div>
                        <label class="label" for="dev-name">
                            "Studio or developer name"
                        </label>
                        <input id="dev-name" class="field" name="display_name" required />
                    </div>
                    <div>
                        <label class="label" for="dev-slug">
                            "Profile address"
                        </label>
                        <div class="flex items-center gap-1">
                            <span class="text-sm text-faint">"/developers/"</span>
                            <input
                                id="dev-slug"
                                class="field"
                                name="slug"
                                pattern="[a-z0-9-]{2,40}"
                                placeholder="made from the name if empty"
                            />
                        </div>
                        <p class="hint">"Lowercase letters, digits and dashes. It can't be changed later."</p>
                    </div>
                    <div>
                        <label class="label" for="dev-bio">
                            "Short bio"
                        </label>
                        <textarea id="dev-bio" class="field" name="bio" rows="3"></textarea>
                    </div>
                    <div>
                        <label class="label" for="dev-site">
                            "Website"
                        </label>
                        <input
                            id="dev-site"
                            class="field"
                            type="url"
                            name="website_url"
                            placeholder="https://"
                        />
                    </div>
                    <ActionError value=move || action.value().get() />
                    <button type="submit" class="btn-primary self-start">
                        "Create profile"
                    </button>
                </ActionForm>
            </div>
        </PageShell>
    }
    .into_any()
}

#[component]
fn GameList(developer: Developer) -> impl IntoView {
    let location = use_location();
    let games = Resource::new(move || location.search.get(), |_| my_games());
    let profile_href = format!("/developers/{}", developer.slug);
    let developer_slug = developer.slug.clone();

    view! {
        <PageShell>
            <div class="flex flex-wrap items-end justify-between gap-4">
                <div>
                    <p class="text-sm text-faint">"Developer dashboard"</p>
                    <h1 class="font-display text-3xl font-semibold">{developer.display_name}</h1>
                    <a href=profile_href class="link text-sm">
                        "View public profile"
                    </a>
                </div>
                <a href="/dashboard/games/new" class="btn-primary">
                    "New game page"
                </a>
            </div>
            <Suspense fallback=Loading>
                {move || Suspend::new(async move {
                    match games.await {
                        Ok(list) if list.is_empty() => {
                            view! {
                                <div class="card mt-8 p-10 text-center">
                                    <p class="text-muted">"No games yet."</p>
                                    <p class="hint">
                                        "Create a game page, then send it for review. It goes public once a reviewer approves it."
                                    </p>
                                </div>
                            }
                                .into_any()
                        }
                        Ok(list) => {
                            view! {
                                <ul class="card mt-8 divide-y divide-line">
                                    {list.into_iter().map(|g| view! { <GameRow item=g /> }).collect_view()}
                                </ul>
                            }
                                .into_any()
                        }
                        Err(e) => {
                            view! {
                                <div class="mt-8">
                                    <ErrorNote message=server_error_text(&e) />
                                </div>
                            }
                                .into_any()
                        }
                    }
                })}
            </Suspense>
            <StrikesCard developer=developer_slug />
        </PageShell>
    }
    .into_any()
}

#[component]
fn GameRow(item: DashboardGame) -> impl IntoView {
    let DashboardGame {
        game, submissions, ..
    } = item;
    let href = format!("/dashboard/games/{}", game.slug);
    let latest = submissions.first().map(|s| match &s.decided_at {
        Some(d) => format!("{} on {}", submission_label(s), date_only(d)),
        None => format!("Submitted {}", date_only(&s.created_at)),
    });
    view! {
        <li>
            <a href=href class="flex items-center gap-4 p-4 hover:bg-raised">
                <Cover seed=game.slug.clone() title="" class="h-12 w-20 shrink-0 rounded-control" />
                <div class="min-w-0 flex-1">
                    <p class="font-medium">{game.title}</p>
                    <p class="truncate text-sm text-faint">
                        {latest.unwrap_or_else(|| "Not submitted yet".into())}
                    </p>
                </div>
                <StatusBadge status=game.status />
            </a>
        </li>
    }
}

#[component]
pub fn NewGame() -> impl IntoView {
    view! {
        <Title text="New game page · Foyer" />
        <RequireSession children=|session: Session| {
            if session.developer.is_none() {
                return view! { <CreateDeveloperForm /> }.into_any();
            }
            view! {
                <PageShell>
                    <a href="/dashboard" class="link text-sm">
                        "← Dashboard"
                    </a>
                    <h1 class="mt-2 font-display text-3xl font-semibold">"New game page"</h1>
                    <p class="mt-2 max-w-prose text-muted">
                        "Start with the basics. It stays a private draft until you send it for review."
                    </p>
                    <div class="mt-8 max-w-3xl">
                        <GameEditor form=GameForm::default() />
                    </div>
                </PageShell>
            }
                .into_any()
        } />
    }
}

#[component]
pub fn EditGame() -> impl IntoView {
    view! {
        <Title text="Edit game · Foyer" />
        <RequireSession children=|_| view! { <EditGameInner /> } />
    }
}

#[component]
fn EditGameInner() -> impl IntoView {
    let params = use_params_map();
    let query = use_query_map();
    let slug = move || params.read().get("slug").unwrap_or_default();
    // Keyed on the query string too, so it refetches after a save or submit
    // redirects back here.
    let game = Resource::new(
        move || {
            (
                slug(),
                query.read().get("saved"),
                query.read().get("submitted"),
                query.read().get("appealed"),
            )
        },
        |(slug, _, _, _)| my_game(slug),
    );
    let flash = move || {
        let q = query.read();
        if q.get("appealed").is_some() {
            Some(
                "Appeal sent. A reviewer who wasn't part of the decision will look at it, and we'll email you the outcome.",
            )
        } else if q.get("submitted").is_some() {
            Some("Sent for review. We'll email you when a reviewer has looked at it.")
        } else if q.get("saved").is_some() {
            Some("Saved.")
        } else {
            None
        }
    };

    view! {
        <PageShell>
            <a href="/dashboard" class="link text-sm">
                "← Dashboard"
            </a>
            {move || {
                flash()
                    .map(|m| {
                        view! {
                            <p
                                role="status"
                                class="mt-4 rounded-control border border-good/40 bg-good/10 px-4 py-3 text-sm text-good"
                            >
                                {m}
                            </p>
                        }
                    })
            }}
            <Suspense fallback=Loading>
                {move || Suspend::new(async move {
                    match game.await {
                        Ok(item) => view! { <EditGameView item /> }.into_any(),
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
            </Suspense>
        </PageShell>
    }
}

#[component]
fn EditGameView(item: DashboardGame) -> impl IntoView {
    let DashboardGame {
        game,
        submissions,
        appeals,
    } = item;
    let editable = is_editable(game.status);
    let public_href = format!("/games/{}", game.slug);
    let is_public = game.status == GameStatus::Published;

    view! {
        <div class="mt-2 flex flex-wrap items-center gap-3">
            <h1 class="font-display text-3xl font-semibold">{game.title.clone()}</h1>
            <StatusBadge status=game.status />
            <Show when=move || is_public>
                <a href=public_href.clone() class="link text-sm">
                    "View public page"
                </a>
            </Show>
        </div>
        <div class="mt-8 grid gap-10 lg:grid-cols-[1fr_22rem]">
            <div>
                {if editable {
                    view! { <GameEditor form=GameForm::from_game(&game) /> }.into_any()
                } else {
                    let text = match game.status {
                        GameStatus::Rejected => {
                            "This game was rejected, so the page is locked. If you think the decision is wrong, you can appeal it once."
                        }
                        GameStatus::Delisted => {
                            "This game was taken off the catalog, so the page is locked. If you think the decision is wrong, you can appeal it once."
                        }
                        _ => {
                            "The page is locked while it's in review. You can edit it again once a reviewer has made a decision."
                        }
                    };
                    view! { <div class="card p-6 text-muted">{text}</div> }.into_any()
                }}
            </div>
            <ReviewPanel game=game.clone() submissions appeals />
        </div>
    }
    .into_any()
}

/// The game page form, for both new and existing games.
#[component]
fn GameEditor(form: GameForm) -> impl IntoView {
    let action = ServerAction::<SaveGame>::new();
    let is_new = form.slug.is_none();
    let checkbox = |name: &'static str, label: &'static str, on: bool| {
        view! {
            <label class="flex items-center gap-2 text-sm">
                <input type="checkbox" name=name checked=on class="size-4 accent-accent" />
                {label}
            </label>
        }
    };
    let descriptors: Vec<(&'static str, ContentDescriptor, bool)> = CONTENT_DESCRIPTORS
        .into_iter()
        .map(|c| (descriptor_field(c), c, form.is_checked(c)))
        .collect();

    view! {
        <ActionForm action attr:class="card flex flex-col gap-6 p-6">
            {form.slug.clone().map(|s| view! { <input type="hidden" name="slug" value=s /> })}
            <div>
                <label class="label" for="g-title">
                    "Title"
                </label>
                <input
                    id="g-title"
                    class="field"
                    name="title"
                    value=form.title.clone()
                    maxlength="80"
                    required
                />
            </div>
            <div>
                <label class="label" for="g-tagline">
                    "Tagline"
                </label>
                <input
                    id="g-tagline"
                    class="field"
                    name="tagline"
                    value=form.tagline.clone()
                    maxlength="140"
                    placeholder="One sentence that makes someone want to play"
                />
            </div>
            <div>
                <label class="label" for="g-desc">
                    "Description"
                </label>
                <textarea id="g-desc" class="field" name="description" rows="8">
                    {form.description.clone()}
                </textarea>
                <p class="hint">"Plain text. Leave a blank line between paragraphs."</p>
            </div>
            <div>
                <label class="label" for="g-tags">
                    "Tags"
                </label>
                <input
                    id="g-tags"
                    class="field"
                    name="tags"
                    value=form.tags.clone()
                    placeholder="puzzle, cozy, exploration"
                />
                <p class="hint">"Comma separated, up to 10. Accurate tags are part of review."</p>
            </div>
            {is_new
                .then(|| {
                    view! {
                        <div>
                            <label class="label" for="g-slug">
                                "Page address"
                            </label>
                            <div class="flex items-center gap-1">
                                <span class="text-sm text-faint">"/games/"</span>
                                <input
                                    id="g-slug"
                                    class="field"
                                    name="new_slug"
                                    pattern="[a-z0-9-]{2,40}"
                                    placeholder="made from the title if empty"
                                />
                            </div>
                            <p class="hint">"Lowercase letters, digits and dashes. It can't be changed later."</p>
                        </div>
                    }
                })}
            <fieldset>
                <legend class="label">"Content descriptors"</legend>
                <p class="hint mb-2 mt-0">"Tick everything that applies. Honest metadata is part of review."</p>
                <div class="grid gap-2 sm:grid-cols-3">
                    {descriptors.into_iter().map(|(n, c, on)| checkbox(n, c.label(), on)).collect_view()}
                </div>
            </fieldset>
            <div>
                <label class="label" for="g-ai">
                    "Generative AI disclosure"
                </label>
                <textarea id="g-ai" class="field" name="ai_disclosure" rows="2">
                    {form.ai_disclosure.clone()}
                </textarea>
                <p class="hint">"Say what, if anything, was made with generative AI. Leave empty if nothing was."</p>
            </div>
            <div>
                <label class="label" for="g-support">
                    "Support links"
                </label>
                <textarea
                    id="g-support"
                    class="field"
                    name="support_links"
                    rows="3"
                    placeholder="https://ko-fi.com/yourname"
                >
                    {form.support_links.clone()}
                </textarea>
                <p class="hint">
                    "One per line: Patreon, Ko-fi, GitHub Sponsors, itch.io or your own site. Your profile's links are shown too."
                </p>
            </div>
            <div class="rounded-control border border-dashed border-line p-4 text-sm text-faint">
                "Platforms come from the builds you push with the "
                <code class="text-ink">"foyer"</code>
                " CLI. Screenshot and capsule uploads come next."
            </div>
            <ActionError value=move || action.value().get() />
            <div class="flex gap-3">
                <button type="submit" class="btn-primary" disabled=move || action.pending().get()>
                    {if is_new { "Create draft" } else { "Save changes" }}
                </button>
            </div>
        </ActionForm>
    }
    .into_any()
}

/// The form field name for a content descriptor checkbox.
fn descriptor_field(c: ContentDescriptor) -> &'static str {
    match c {
        ContentDescriptor::Violence => "violence",
        ContentDescriptor::Blood => "blood",
        ContentDescriptor::Horror => "horror",
        ContentDescriptor::StrongLanguage => "strong_language",
        ContentDescriptor::DrugsAlcohol => "drugs_alcohol",
        ContentDescriptor::MatureThemes => "mature_themes",
    }
}

/// What a game needs before it can be submitted, from the quality bar.
fn checklist(game: &Game) -> Vec<(&'static str, bool)> {
    vec![
        (
            "A title and tagline",
            !game.title.is_empty() && !game.tagline.is_empty(),
        ),
        ("A description", !game.description.trim().is_empty()),
        (
            "A ready build for at least one platform",
            !game.platforms.is_empty(),
        ),
        ("At least 3 screenshots", screenshots(game).count() >= 3),
        ("Accurate tags", !game.tags.is_empty()),
    ]
}

#[component]
fn ReviewPanel(game: Game, submissions: Vec<Submission>, appeals: Vec<Appeal>) -> impl IntoView {
    let action = ServerAction::<SubmitForReview>::new();
    let appeal_target = appealable(&game, &submissions, &appeals).cloned();
    let has_appeals = !appeals.is_empty();
    let can_submit = can_submit(game.status);
    let checks = checklist(&game);
    let has_history = !submissions.is_empty();
    let resubmit = game.status == GameStatus::ChangesRequested;
    let slug = game.slug.clone();

    view! {
        <aside class="flex flex-col gap-6">
            <section class="card p-5">
                <h2 class="font-display text-lg font-semibold">"Review"</h2>
                <p class="mt-1 text-sm text-faint">
                    "A person plays every game before it's listed. Aim for 5 business days."
                </p>
                <ul class="mt-4 flex flex-col gap-2 text-sm">
                    {checks
                        .into_iter()
                        .map(|(label, ok)| {
                            view! {
                                <li class="flex items-center gap-2">
                                    <span
                                        aria-hidden="true"
                                        class=if ok { "text-good" } else { "text-faint" }
                                    >
                                        {if ok { "✓" } else { "○" }}
                                    </span>
                                    <span class=if ok { "" } else { "text-muted" }>{label}</span>
                                    <span class="sr-only">{if ok { "(done)" } else { "(missing)" }}</span>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                <a href="/guidelines" class="link mt-3 inline-block text-sm">
                    "Content guidelines"
                </a>
                {can_submit.then(|| view! {
                    <ActionForm action attr:class="mt-5 flex flex-col gap-3 border-t border-line pt-5">
                        <input type="hidden" name="slug" value=slug.clone() />
                        <div>
                            <label class="label" for="r-notes">
                                "Notes for the reviewer"
                            </label>
                            <textarea
                                id="r-notes"
                                class="field"
                                name="notes"
                                rows="3"
                                placeholder="Anything they should know: controls, known issues, what changed"
                            ></textarea>
                        </div>
                        <ActionError value=move || action.value().get() />
                        <button type="submit" class="btn-primary" disabled=move || action.pending().get()>
                            {if resubmit { "Resubmit for review" } else { "Submit for review" }}
                        </button>
                    </ActionForm>
                })}
            </section>
            {appeal_target.map(|s| view! { <AppealForm slug=game.slug.clone() decision=s /> })}
            <Show when=move || has_appeals>
                <section class="card p-5">
                    <h2 class="font-display text-lg font-semibold">"Appeals"</h2>
                    <ol class="mt-4 flex flex-col gap-5">
                        {appeals
                            .clone()
                            .into_iter()
                            .map(|a| view! { <AppealEntry appeal=a /> })
                            .collect_view()}
                    </ol>
                </section>
            </Show>
            <Show when=move || has_history>
                <section class="card p-5">
                    <h2 class="font-display text-lg font-semibold">"History"</h2>
                    <ol class="mt-4 flex flex-col gap-5">
                        {submissions
                            .clone()
                            .into_iter()
                            .map(|s| view! { <SubmissionItem submission=s /> })
                            .collect_view()}
                    </ol>
                </section>
            </Show>
        </aside>
    }
    .into_any()
}

#[component]
fn AppealForm(slug: String, decision: Submission) -> impl IntoView {
    let action = ServerAction::<FileAppeal>::new();
    let what = if decision.kind == SubmissionKind::Delisting {
        "delisting"
    } else {
        "rejection"
    };
    view! {
        <section class="card p-5">
            <h2 class="font-display text-lg font-semibold">"Appeal"</h2>
            <p class="mt-1 text-sm text-faint">
                {format!(
                    "Think the {what} is wrong? Explain why. A different reviewer decides, and you can appeal each decision once."
                )}
            </p>
            <ActionForm action attr:class="mt-4 flex flex-col gap-3">
                <input type="hidden" name="slug" value=slug />
                <div>
                    <label class="label" for="appeal-statement">
                        "Your case"
                    </label>
                    <textarea
                        id="appeal-statement"
                        class="field"
                        name="statement"
                        rows="5"
                        required
                        placeholder="What the reviewer got wrong, and anything that shows it: source files, licences, a fixed build"
                    ></textarea>
                </div>
                <ActionError value=move || action.value().get() />
                <button type="submit" class="btn-primary" disabled=move || action.pending().get()>
                    "Send appeal"
                </button>
            </ActionForm>
        </section>
    }
}

#[component]
pub fn AppealEntry(
    appeal: Appeal,
    /// Shown to a reviewer rather than the game's developer.
    #[prop(optional)]
    for_reviewer: bool,
) -> impl IntoView {
    let tone = match appeal.status {
        AppealStatus::Pending => "text-info",
        AppealStatus::Upheld => "text-bad",
        AppealStatus::Overturned | AppealStatus::Reopened => "text-good",
    };
    let statement_label = if for_reviewer {
        "Developer: "
    } else {
        "You wrote: "
    };
    view! {
        <li class="text-sm">
            <p class=format!("font-medium {tone}")>{appeal.status.label()}</p>
            <p class="text-faint">
                {format!(
                    "Appealed {} the {} of {}",
                    date_only(&appeal.created_at),
                    submission_label(&appeal.submission).to_lowercase(),
                    appeal.submission.decided_at.as_ref().map(date_only).unwrap_or_default(),
                )}
                {appeal.decided_at.map(|d| format!(" · decided {}", date_only(&d)))}
                {appeal.reviewer.map(|r| format!(" by {}", r.username))}
            </p>
            <p class="mt-2 text-muted">{statement_label} {appeal.statement}</p>
            {appeal
                .decision_notes
                .map(|n| {
                    view! {
                        <blockquote class="mt-2 border-l-2 border-line pl-3 text-ink">{n}</blockquote>
                    }
                })}
        </li>
    }
}

/// The developer's strikes, if they have any.
#[component]
fn StrikesCard(developer: String) -> impl IntoView {
    let strikes = Resource::new(move || developer.clone(), my_strikes);
    view! {
        <Suspense>
            {move || Suspend::new(async move {
                let strikes = strikes.await.unwrap_or_default();
                if strikes.is_empty() {
                    return ().into_any();
                }
                let active = strikes.iter().filter(|s| s.is_active()).count();
                view! {
                    <section class="card mt-8 p-5">
                        <h2 class="font-display text-lg font-semibold">"Strikes"</h2>
                        <p class="mt-1 text-sm text-faint">
                            {format!(
                                "{active} active. A strike records a guideline violation; reviewers see them when they look at your games."
                            )}
                        </p>
                        <ul class="mt-4 flex flex-col gap-4">
                            {strikes.into_iter().map(|s| view! { <li><StrikeEntry strike=s /></li> }).collect_view()}
                        </ul>
                    </section>
                }
                    .into_any()
            })}
        </Suspense>
    }
}

#[component]
pub fn StrikeEntry(strike: Strike) -> impl IntoView {
    let active = strike.is_active();
    view! {
        <div class="text-sm">
            <p class=if active { "font-medium text-bad" } else { "font-medium text-faint line-through" }>
                {strike.rule.label()}
            </p>
            <p class="text-faint">
                {date_only(&strike.created_at)}
                {strike.game.clone().map(|g| format!(" · {g}"))}
                {strike.issued_by.clone().map(|u| format!(" · by {}", u.username))}
            </p>
            <p class="mt-1 text-muted">{strike.notes.clone()}</p>
            {strike
                .revoked_at
                .map(|d| {
                    view! {
                        <p class="mt-1 text-good">
                            {format!(
                                "Revoked {}: {}",
                                date_only(&d),
                                strike.revoke_reason.clone().unwrap_or_default(),
                            )}
                        </p>
                    }
                })}
        </div>
    }
}

#[component]
pub fn SubmissionItem(
    submission: Submission,
    /// Shown to a reviewer rather than the game's developer.
    #[prop(optional)]
    for_reviewer: bool,
) -> impl IntoView {
    let notes_label = if for_reviewer {
        "Developer's notes: "
    } else {
        "Your notes: "
    };
    let label = submission_label(&submission);
    let Submission {
        status,
        developer_notes,
        reviewer,
        decision_notes,
        rule,
        created_at,
        decided_at,
        ..
    } = submission;
    let tone = match status {
        SubmissionStatus::Approved => "text-good",
        SubmissionStatus::ChangesRequested => "text-warn",
        SubmissionStatus::Rejected => "text-bad",
        _ => "text-info",
    };
    view! {
        <li class="text-sm">
            <p class=format!("font-medium {tone}")>{label}</p>
            <p class="text-faint">
                {if label == "Delisted" {
                    format!("On {}", date_only(&created_at))
                } else {
                    let decided = decided_at.map(|d| format!(" · decided {}", date_only(&d)));
                    format!("Submitted {}{}", date_only(&created_at), decided.unwrap_or_default())
                }}
                {reviewer.map(|r| format!(" by {}", r.username))}
            </p>
            {(!developer_notes.is_empty())
                .then(|| view! { <p class="mt-2 text-muted">{notes_label} {developer_notes}</p> })}
            {rule.map(|r| view! { <p class="mt-2 text-xs uppercase tracking-wide text-faint">{r.label()}</p> })}
            {decision_notes
                .map(|n| {
                    view! {
                        <blockquote class="mt-2 border-l-2 border-line pl-3 text-ink">{n}</blockquote>
                    }
                })}
        </li>
    }
    .into_any()
}

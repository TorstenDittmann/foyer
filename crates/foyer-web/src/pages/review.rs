//! Staff review: the queue of submissions and the screen for judging one
//! game, with its builds, malware scan results and downloads.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::hooks::{use_location, use_params_map, use_query_map};

use crate::api::{
    ClaimSubmission, DecideSubmission, ReleaseSubmission, get_review_game, review_queue,
};
use crate::components::*;
use crate::model::Submission;
use crate::model::*;
use crate::pages::dashboard::{RequireSession, SubmissionItem};

/// Renders `children` for reviewers and admins only.
#[component]
fn RequireStaff<F, V>(children: F) -> impl IntoView
where
    F: Fn() -> V + Send + Sync + 'static,
    V: IntoView + 'static,
{
    let children = std::sync::Arc::new(children);
    view! {
        <RequireSession children=move |session: Session| {
            if session.user.role.is_staff() {
                children().into_any()
            } else {
                view! {
                    <PageShell>
                        <ErrorNote message="Only reviewers can see the review queue." />
                    </PageShell>
                }
                    .into_any()
            }
        } />
    }
}

#[component]
pub fn ReviewQueue() -> impl IntoView {
    view! {
        <Title text="Review queue · Foyer" />
        <RequireStaff children=|| view! { <QueueList /> } />
    }
}

#[component]
fn QueueList() -> impl IntoView {
    let location = use_location();
    let query = use_query_map();
    let queue = Resource::new(move || location.search.get(), |_| review_queue());
    let decided = move || {
        query.read().get("decided").map(|slug| {
            view! {
                <p role="status" class="card mt-6 px-4 py-3 text-sm text-good">
                    {format!("Decision recorded for {slug}.")}
                </p>
            }
        })
    };

    view! {
        <PageShell>
            <p class="text-sm text-faint">"Staff"</p>
            <h1 class="font-display text-3xl font-semibold">"Review queue"</h1>
            <p class="mt-2 max-w-prose text-muted">
                "Open submissions, oldest first. Claim one before you look at it so nobody else does the same work."
            </p>
            {decided}
            <Suspense fallback=Loading>
                {move || Suspend::new(async move {
                    match queue.await {
                        Ok(items) if items.is_empty() => {
                            view! {
                                <div class="card mt-8 p-10 text-center text-muted">
                                    "Nothing waiting. Nice."
                                </div>
                            }
                                .into_any()
                        }
                        Ok(items) => {
                            view! {
                                <ul class="mt-8 flex flex-col gap-3">
                                    {items
                                        .into_iter()
                                        .map(|item| view! { <QueueRow item /> })
                                        .collect_view()}
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
        </PageShell>
    }
}

#[component]
fn QueueRow(item: QueueItem) -> impl IntoView {
    let QueueItem {
        submission,
        game,
        flagged_builds,
    } = item;
    let href = format!("/review/{}", game.slug);
    let who = match (&submission.status, &submission.reviewer) {
        (SubmissionStatus::InReview, Some(r)) => format!("Claimed by {}", r.username),
        _ => "Unclaimed".to_string(),
    };
    let flagged = flagged_builds.len();
    view! {
        <li>
            <a
                href=href
                class="card flex flex-col gap-2 p-4 transition-colors hover:border-accent sm:flex-row sm:items-center"
            >
                <div class="flex-1">
                    <p class="font-semibold">{game.title}</p>
                    <p class="text-sm text-faint">
                        {format!(
                            "{} · submitted {}",
                            game.developer.display_name,
                            date_only(&submission.created_at),
                        )}
                    </p>
                </div>
                {(flagged > 0)
                    .then(|| {
                        view! {
                            <span class="rounded-full bg-bad/15 px-2.5 py-0.5 text-xs font-medium text-bad">
                                {if flagged == 1 {
                                    "1 flagged build".to_string()
                                } else {
                                    format!("{flagged} flagged builds")
                                }}
                            </span>
                        }
                    })}
                <span class="text-sm text-muted">{who}</span>
            </a>
        </li>
    }
}

#[component]
pub fn ReviewGamePage() -> impl IntoView {
    view! {
        <Title text="Review · Foyer" />
        <RequireStaff children=|| view! { <ReviewGameLoader /> } />
    }
}

#[component]
fn ReviewGameLoader() -> impl IntoView {
    let params = use_params_map();
    let location = use_location();
    let slug = move || params.read().get("slug").unwrap_or_default();
    let page = Resource::new(
        move || (slug(), location.search.get()),
        |(slug, _)| get_review_game(slug),
    );
    view! {
        <Suspense fallback=Loading>
            {move || Suspend::new(async move {
                match page.await {
                    Ok(page) => view! { <ReviewGameView page /> }.into_any(),
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
fn ReviewGameView(page: ReviewGame) -> impl IntoView {
    let ReviewGame {
        game,
        developer,
        submissions,
        builds,
        downloads,
    } = page;
    let slug = game.slug.clone();
    let open = submissions.iter().find(|s| !s.status.is_decided()).cloned();
    let history: Vec<Submission> = submissions
        .into_iter()
        .filter(|s| s.status.is_decided())
        .collect();
    let paragraphs: Vec<String> = game
        .description
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    view! {
        <Title text=format!("Review {} · Foyer", game.title) />
        <PageShell>
            <a href="/review" class="link text-sm">
                "Back to the queue"
            </a>
            <div class="mt-4 flex flex-wrap items-center gap-3">
                <h1 class="font-display text-3xl font-semibold">{game.title.clone()}</h1>
                <StatusBadge status=game.status />
            </div>
            <p class="mt-2 text-muted">{game.tagline.clone()}</p>
            <p class="mt-1 text-sm text-faint">
                {format!("by {}", game.developer.display_name)}
                {developer
                    .as_ref()
                    .and_then(|d| d.website_url.clone())
                    .map(|url| {
                        let href = url.clone();
                        view! {
                            " · "
                            <a href=href class="link" target="_blank" rel="noopener noreferrer">
                                {url}
                            </a>
                        }
                    })}
            </p>

            <div class="mt-8 grid gap-8 lg:grid-cols-[1fr_22rem]">
                <div class="flex flex-col gap-8">
                    <section>
                        <h2 class="mb-3 font-display text-xl font-semibold">"Builds"</h2>
                        <BuildList builds />
                    </section>
                    <section>
                        <h2 class="mb-3 font-display text-xl font-semibold">"Downloads"</h2>
                        <DownloadList slug=slug.clone() downloads />
                    </section>
                    <section>
                        <h2 class="mb-3 font-display text-xl font-semibold">"Store page"</h2>
                        <div class="card flex flex-col gap-4 p-4 text-sm">
                            <div class="flex flex-col gap-3 text-muted">
                                {paragraphs.into_iter().map(|p| view! { <p>{p}</p> }).collect_view()}
                            </div>
                            <p>
                                <span class="text-faint">"Tags: "</span>
                                {game.tags.join(", ")}
                            </p>
                            <p>
                                <span class="text-faint">"Content: "</span>
                                {if game.content_descriptors.is_empty() {
                                    "none declared".to_string()
                                } else {
                                    game.content_descriptors
                                        .iter()
                                        .map(|c| c.label())
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }}
                            </p>
                            <p>
                                <span class="text-faint">"Generative AI: "</span>
                                {game.ai_disclosure.clone().unwrap_or_else(|| "none declared".into())}
                            </p>
                            <p>
                                <span class="text-faint">"Media: "</span>
                                {format!("{} files", game.media.len())}
                            </p>
                        </div>
                    </section>
                </div>
                <aside class="flex flex-col gap-6">
                    {match open {
                        Some(s) => view! { <OpenSubmission slug=slug.clone() submission=s /> }.into_any(),
                        None => {
                            view! {
                                <div class="card p-4 text-sm text-muted">
                                    "No open submission for this game."
                                </div>
                            }
                                .into_any()
                        }
                    }}
                    <Show when={
                        let empty = history.is_empty();
                        move || !empty
                    }>
                        <section class="card p-4">
                            <h2 class="mb-3 text-sm font-semibold">"Earlier decisions"</h2>
                            <ul class="flex flex-col gap-4">
                                {history
                                    .clone()
                                    .into_iter()
                                    .map(|s| view! { <SubmissionItem submission=s for_reviewer=true /> })
                                    .collect_view()}
                            </ul>
                        </section>
                    </Show>
                </aside>
            </div>
        </PageShell>
    }
}

#[component]
fn BuildList(builds: Vec<Build>) -> impl IntoView {
    if builds.is_empty() {
        return view! { <p class="card p-4 text-sm text-muted">"No builds pushed yet."</p> }
            .into_any();
    }
    view! {
        <ul class="flex flex-col gap-3">
            {builds.into_iter().map(|build| view! { <BuildRow item=build /> }).collect_view()}
        </ul>
    }
    .into_any()
}

#[component]
fn BuildRow(item: Build) -> impl IntoView {
    let build = item;
    let flagged = build.scan.as_ref().is_some_and(|s| s.status.is_flagged());
    let border = if flagged { "border-bad/50" } else { "" };
    let scan = build.scan.clone();
    let scan_text = match &scan {
        Some(s) => format!("{} on {}", s.status.label(), date_only(&s.scanned_at)),
        None => "Not scanned yet".to_string(),
    };
    let scan_tone = match scan.as_ref().map(|s| s.status) {
        Some(ScanStatus::Clean) => "text-good",
        Some(ScanStatus::Infected) => "text-bad",
        Some(ScanStatus::Incomplete) => "text-warn",
        _ => "text-faint",
    };
    let findings = scan.map(|s| s.findings).unwrap_or_default();
    view! {
        <li class=format!("card p-4 text-sm {border}")>
            <div class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
                <span class="font-semibold">
                    {format!("{} · {}", build.platform.label(), build.channel)}
                </span>
                <span class="text-muted">{build.version.clone().unwrap_or_else(|| "no version".into())}</span>
                <span class="text-faint">{build.state.label()}</span>
                <span class="ml-auto text-faint">
                    {format!("{} · {}", format_size(build.total_size), date_only(&build.created_at))}
                </span>
            </div>
            <p class=format!("mt-1 {scan_tone}")>{scan_text}</p>
            {(!findings.is_empty())
                .then(|| {
                    view! {
                        <ul class="mt-2 flex flex-col gap-1 font-mono text-xs">
                            {findings
                                .into_iter()
                                .map(|f| view! { <li>{format!("{}: {}", f.path, f.detail)}</li> })
                                .collect_view()}
                        </ul>
                    }
                })}
            {build.launch.clone().map(|l| {
                view! {
                    <p class="mt-1 text-faint">
                        "Launches "
                        <code class="text-ink">{l.executable}</code>
                        {(!l.args.is_empty()).then(|| format!(" {}", l.args.join(" ")))}
                    </p>
                }
            })}
            {build.failure.clone().map(|f| view! { <p class="mt-1 text-bad">{f}</p> })}
        </li>
    }
}

#[component]
fn DownloadList(slug: String, downloads: Vec<GameDownload>) -> impl IntoView {
    if downloads.is_empty() {
        return view! {
            <p class="card p-4 text-sm text-muted">
                "No zips yet. They appear once a build is scanned and packed."
            </p>
        }
        .into_any();
    }
    view! {
        <ul class="flex flex-col gap-2">
            {downloads
                .into_iter()
                .map(|d| {
                    let href = format!("/download/{slug}/{}", d.channel);
                    view! {
                        <li>
                            // A server redirect, not a page.
                            <a href=href rel="external nofollow" class="btn-secondary w-full justify-between">
                                <span>{format!("{} · {}", d.platform.label(), d.channel)}</span>
                                <span class="text-xs font-normal opacity-80">
                                    {format!(
                                        "{} · {}",
                                        d.build.version.clone().unwrap_or_default(),
                                        format_size(d.archive_size),
                                    )}
                                </span>
                            </a>
                        </li>
                    }
                })
                .collect_view()}
        </ul>
        <p class="hint">"Downloads by staff count in the stats like any other."</p>
    }
    .into_any()
}

#[component]
fn OpenSubmission(slug: String, submission: Submission) -> impl IntoView {
    let claim = ServerAction::<ClaimSubmission>::new();
    let release = ServerAction::<ReleaseSubmission>::new();
    let decide = ServerAction::<DecideSubmission>::new();
    let id = submission.id.to_string();
    let claimed_by = submission.reviewer.clone().map(|r| r.username);
    let in_review = submission.status == SubmissionStatus::InReview;
    let (form_slug, form_id) = (slug.clone(), id.clone());
    let claim_or_release = if in_review {
        view! {
            <ActionForm action=release>
                <input type="hidden" name="submission" value=form_id />
                <button type="submit" class="btn-secondary w-full py-1.5 text-sm">
                    "Release back to the queue"
                </button>
            </ActionForm>
            <ActionError value=move || release.value().get() />
        }
        .into_any()
    } else {
        view! {
            <ActionForm action=claim>
                <input type="hidden" name="slug" value=form_slug />
                <input type="hidden" name="submission" value=form_id />
                <button type="submit" class="btn-primary w-full">
                    "Claim"
                </button>
            </ActionForm>
            <ActionError value=move || claim.value().get() />
        }
        .into_any()
    };

    view! {
        <section class="card flex flex-col gap-4 p-4">
            <div>
                <h2 class="text-sm font-semibold">"Open submission"</h2>
                <p class="text-sm text-faint">
                    {format!("Submitted {}", date_only(&submission.created_at))}
                    {claimed_by.map(|u| format!(" · claimed by {u}"))}
                </p>
            </div>
            {(!submission.developer_notes.is_empty())
                .then(|| {
                    view! {
                        <blockquote class="border-l-2 border-line pl-3 text-sm text-muted">
                            {submission.developer_notes.clone()}
                        </blockquote>
                    }
                })}
            {claim_or_release}
            <ActionForm action=decide attr:class="flex flex-col gap-3 border-t border-line pt-4">
                <input type="hidden" name="slug" value=slug.clone() />
                <input type="hidden" name="submission" value=id.clone() />
                <fieldset class="flex flex-col gap-1 text-sm">
                    <legend class="label">"Decision"</legend>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="approved" required />
                        "Approve and publish"
                    </label>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="changes_requested" />
                        "Ask for changes"
                    </label>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="rejected" />
                        "Reject"
                    </label>
                </fieldset>
                <div>
                    <label class="label" for="decision-rule">
                        "Guideline"
                    </label>
                    <select id="decision-rule" name="rule" class="field">
                        <option value="">"None"</option>
                        {POLICY_RULES
                            .into_iter()
                            .map(|r| view! { <option value=r.as_str()>{r.label()}</option> })
                            .collect_view()}
                    </select>
                    <p class="hint">"Required for a rejection."</p>
                </div>
                <div>
                    <label class="label" for="decision-notes">
                        "Notes for the developer"
                    </label>
                    <textarea id="decision-notes" name="notes" rows="5" class="field"></textarea>
                    <p class="hint">"Required unless you approve. The developer sees these."</p>
                </div>
                <ActionError value=move || decide.value().get() />
                <button type="submit" class="btn-primary" disabled=move || decide.pending().get()>
                    "Record decision"
                </button>
            </ActionForm>
        </section>
    }
}

//! Staff review: the queues of submissions, player reports and appeals, and
//! the screen for judging one game, with its builds, malware scan results,
//! downloads, reports, appeals and the developer's strikes.

use leptos::prelude::*;
use leptos_meta::Title;
use leptos_router::components::A;
use leptos_router::hooks::{use_location, use_params_map, use_query_map};

use crate::api::{
    ClaimSubmission, DecideSubmission, DelistGame, DismissGameReports, JudgeAppeal,
    ReleaseSubmission, RevokeDeveloperStrike, appeal_queue, get_review_game, report_queue,
    review_queue,
};
use crate::components::*;
use crate::model::Submission;
use crate::model::*;
use crate::pages::dashboard::{AppealEntry, RequireSession, StrikeEntry, SubmissionItem};

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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Submissions,
    Reports,
    Appeals,
}

impl Tab {
    fn from_query(tab: Option<String>) -> Self {
        match tab.as_deref() {
            Some("reports") => Tab::Reports,
            Some("appeals") => Tab::Appeals,
            _ => Tab::Submissions,
        }
    }

    fn href(self) -> &'static str {
        match self {
            Tab::Submissions => "/review",
            Tab::Reports => "/review?tab=reports",
            Tab::Appeals => "/review?tab=appeals",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Submissions => "Submissions",
            Tab::Reports => "Reports",
            Tab::Appeals => "Appeals",
        }
    }
}

#[component]
fn QueueList() -> impl IntoView {
    let query = use_query_map();
    let tab = move || Tab::from_query(query.read().get("tab"));
    let note = move || {
        let q = query.read();
        let text = if let Some(slug) = q.get("decided") {
            format!("Decision recorded for {slug}.")
        } else {
            let slug = q.get("done")?;
            match tab() {
                Tab::Reports => format!("Reports on {slug} dismissed."),
                _ => format!("Appeal decided for {slug}. The developer gets an email."),
            }
        };
        Some(view! { <p role="status" class="card mt-6 px-4 py-3 text-sm text-good">{text}</p> })
    };

    view! {
        <PageShell>
            <p class="text-sm text-faint">"Staff"</p>
            <h1 class="font-display text-3xl font-semibold">"Review"</h1>
            <nav class="mt-4 flex gap-2 border-b border-line" aria-label="Review queues">
                {[Tab::Submissions, Tab::Reports, Tab::Appeals]
                    .into_iter()
                    .map(|t| {
                        view! {
                            <a
                                href=t.href()
                                aria-current=move || (tab() == t).then_some("page")
                                class=move || {
                                    if tab() == t {
                                        "-mb-px border-b-2 border-accent px-3 py-2 text-sm font-medium"
                                    } else {
                                        "px-3 py-2 text-sm text-muted hover:text-ink"
                                    }
                                }
                            >
                                {t.label()}
                            </a>
                        }
                    })
                    .collect_view()}
            </nav>
            {note}
            {move || match tab() {
                Tab::Submissions => view! { <SubmissionQueue /> }.into_any(),
                Tab::Reports => view! { <ReportQueue /> }.into_any(),
                Tab::Appeals => view! { <AppealQueue /> }.into_any(),
            }}
        </PageShell>
    }
}

/// An empty queue.
#[component]
fn Empty(text: &'static str) -> impl IntoView {
    view! { <div class="card mt-8 p-10 text-center text-muted">{text}</div> }
}

#[component]
fn SubmissionQueue() -> impl IntoView {
    let location = use_location();
    let queue = Resource::new(move || location.search.get(), |_| review_queue());
    view! {
        <p class="mt-4 max-w-prose text-muted">
            "Open submissions, oldest first. Claim one before you look at it so nobody else does the same work."
        </p>
        {move || Suspend::new(async move {
            match queue.await {
                Ok(items) if items.is_empty() => view! { <Empty text="Nothing waiting. Nice." /> }.into_any(),
                Ok(items) => {
                    view! {
                        <ul class="mt-8 flex flex-col gap-3">
                            {items.into_iter().map(|item| view! { <QueueRow item /> }).collect_view()}
                        </ul>
                    }
                        .into_any()
                }
                Err(e) => view! { <div class="mt-8"><ErrorNote message=server_error_text(&e) /></div> }.into_any(),
            }
        })}
    }
}

#[component]
fn ReportQueue() -> impl IntoView {
    let location = use_location();
    let queue = Resource::new(move || location.search.get(), |_| report_queue());
    view! {
        <p class="mt-4 max-w-prose text-muted">
            "Published games players reported. Malware reports first, then the most reported. Nothing changes until a reviewer acts: open the game, play it, then dismiss the reports or delist it."
        </p>
        {move || Suspend::new(async move {
            match queue.await {
                Ok(items) if items.is_empty() => view! { <Empty text="No open reports." /> }.into_any(),
                Ok(items) => {
                    view! {
                        <ul class="mt-8 flex flex-col gap-3">
                            {items.into_iter().map(|item| view! { <ReportRow item /> }).collect_view()}
                        </ul>
                    }
                        .into_any()
                }
                Err(e) => view! { <div class="mt-8"><ErrorNote message=server_error_text(&e) /></div> }.into_any(),
            }
        })}
    }
}

/// "1 strike", "3 strikes".
fn strikes_text(n: u64) -> String {
    if n == 1 {
        "1 strike".into()
    } else {
        format!("{n} strikes")
    }
}

#[component]
fn StrikeBadge(count: u64) -> impl IntoView {
    (count > 0).then(|| {
        view! {
            <span class="rounded-full bg-warn/15 px-2.5 py-0.5 text-xs font-medium text-warn">
                {strikes_text(count)}
            </span>
        }
    })
}

#[component]
fn ReportRow(item: ReportedGame) -> impl IntoView {
    let href = format!("/review/{}", item.game.slug);
    let malware = item
        .reasons
        .iter()
        .any(|r| r.reason == ReportReason::Malware);
    let reasons = item
        .reasons
        .iter()
        .map(|r| format!("{} ({})", r.reason.label(), r.count))
        .collect::<Vec<_>>()
        .join(" · ");
    view! {
        <li>
            <a
                href=href
                class=format!(
                    "card flex flex-col gap-2 border border-transparent p-4 transition-colors hover:border-accent sm:flex-row sm:items-center {}",
                    if malware { "border-bad/50!" } else { "" },
                )
            >
                <div class="flex-1">
                    <p class="font-semibold">{item.game.title.clone()}</p>
                    <p class="text-sm text-faint">
                        {format!(
                            "{} · first reported {} · latest {}",
                            item.game.developer.display_name,
                            date_only(&item.first_reported_at),
                            date_only(&item.last_reported_at),
                        )}
                    </p>
                    <p class="mt-1 text-sm text-muted">{reasons}</p>
                </div>
                <StrikeBadge count=item.developer_strikes />
                <span class="text-sm font-medium">
                    {if item.open_reports == 1 {
                        "1 report".to_string()
                    } else {
                        format!("{} reports", item.open_reports)
                    }}
                </span>
            </a>
        </li>
    }
}

#[component]
fn AppealQueue() -> impl IntoView {
    let location = use_location();
    let queue = Resource::new(move || location.search.get(), |_| appeal_queue());
    view! {
        <p class="mt-4 max-w-prose text-muted">
            "Developers asking for a second look at a rejection or a delisting, oldest first. The reviewer who made the decision can't decide its appeal."
        </p>
        {move || Suspend::new(async move {
            match queue.await {
                Ok(items) if items.is_empty() => view! { <Empty text="No appeals waiting." /> }.into_any(),
                Ok(items) => {
                    view! {
                        <ul class="mt-8 flex flex-col gap-4">
                            {items.into_iter().map(|item| view! { <AppealCard item /> }).collect_view()}
                        </ul>
                    }
                        .into_any()
                }
                Err(e) => view! { <div class="mt-8"><ErrorNote message=server_error_text(&e) /></div> }.into_any(),
            }
        })}
    }
}

#[component]
fn AppealCard(item: AppealItem) -> impl IntoView {
    let AppealItem {
        appeal,
        game,
        developer_strikes,
    } = item;
    let action = ServerAction::<JudgeAppeal>::new();
    let decision = appeal.submission.clone();
    let href = format!("/review/{}", game.slug);
    let field = format!("appeal-notes-{}", appeal.id);
    view! {
        <li class="card flex flex-col gap-4 p-4">
            <div class="flex flex-wrap items-center gap-3">
                <A href=href attr:class="font-semibold link">{game.title.clone()}</A>
                <StatusBadge status=game.status />
                <StrikeBadge count=developer_strikes />
                <span class="ml-auto text-sm text-faint">
                    {format!("{} · appealed {}", game.developer.display_name, date_only(&appeal.created_at))}
                </span>
            </div>
            <div class="grid gap-4 text-sm md:grid-cols-2">
                <div>
                    <p class="label">
                        {format!(
                            "{} by {}",
                            submission_label(&decision),
                            decision.reviewer.as_ref().map(|r| r.username.clone()).unwrap_or_default(),
                        )}
                    </p>
                    {decision.rule.map(|r| view! { <p class="text-xs uppercase tracking-wide text-faint">{r.label()}</p> })}
                    <blockquote class="mt-1 border-l-2 border-line pl-3 text-muted">
                        {decision.decision_notes.clone().unwrap_or_default()}
                    </blockquote>
                </div>
                <div>
                    <p class="label">"The developer's case"</p>
                    <blockquote class="border-l-2 border-accent pl-3 text-ink">{appeal.statement.clone()}</blockquote>
                </div>
            </div>
            <ActionForm action attr:class="flex flex-col gap-3 border-t border-line pt-4">
                <input type="hidden" name="slug" value=game.slug.clone() />
                <input type="hidden" name="appeal" value=appeal.id.to_string() />
                <fieldset class="flex flex-wrap gap-x-5 gap-y-1 text-sm">
                    <legend class="label">"Outcome"</legend>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="upheld" required />
                        "Uphold the decision"
                    </label>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="overturned" />
                        "Overturn and publish"
                    </label>
                    <label class="flex items-center gap-2">
                        <input type="radio" name="outcome" value="reopened" />
                        "Reopen for changes"
                    </label>
                </fieldset>
                <div>
                    <label class="label" for=field.clone()>"Notes for the developer"</label>
                    <textarea id=field name="notes" rows="3" class="field" required></textarea>
                    <p class="hint">"Overturning or reopening also revokes strikes that came with the decision."</p>
                </div>
                <ActionError value=move || action.value().get() />
                <button type="submit" class="btn-primary self-start" disabled=move || action.pending().get()>
                    "Decide appeal"
                </button>
            </ActionForm>
        </li>
    }
}

#[component]
fn QueueRow(item: QueueItem) -> impl IntoView {
    let QueueItem {
        submission,
        game,
        flagged_builds,
        developer_strikes,
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
                class="card flex flex-col gap-2 border border-transparent p-4 transition-colors hover:border-accent sm:flex-row sm:items-center"
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
                <StrikeBadge count=developer_strikes />
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
        reports,
        appeals,
        strikes,
    } = page;
    let open_reports: Vec<Report> = reports
        .iter()
        .filter(|r| r.status == ReportStatus::Open)
        .cloned()
        .collect();
    let closed_reports = reports.len() - open_reports.len();
    let published = game.status == GameStatus::Published;
    let has_appeals = !appeals.is_empty();
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
            <A href="/review" attr:class="link text-sm">
                "Back to the queue"
            </A>
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
                    <ReportsSection slug=slug.clone() open=open_reports closed=closed_reports />
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
                    {published.then(|| view! { <DelistForm slug=slug.clone() /> })}
                    <StrikesSection slug=slug.clone() strikes />
                    <Show when=move || has_appeals>
                        <section class="card p-4">
                            <h2 class="mb-3 text-sm font-semibold">"Appeals"</h2>
                            <ul class="flex flex-col gap-4">
                                {appeals
                                    .clone()
                                    .into_iter()
                                    .map(|a| view! { <AppealEntry appeal=a for_reviewer=true /> })
                                    .collect_view()}
                            </ul>
                        </section>
                    </Show>
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
    let border = if flagged { "border border-bad/50" } else { "" };
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
                <label class="flex items-center gap-2 text-sm">
                    <input type="checkbox" name="strike" />
                    "Record a strike (rejections only)"
                </label>
                <ActionError value=move || decide.value().get() />
                <button type="submit" class="btn-primary" disabled=move || decide.pending().get()>
                    "Record decision"
                </button>
            </ActionForm>
        </section>
    }
}

#[component]
fn ReportsSection(slug: String, open: Vec<Report>, closed: usize) -> impl IntoView {
    if open.is_empty() {
        return (closed > 0)
            .then(|| {
                view! {
                    <p class="text-sm text-faint">
                        {format!("No open reports. {closed} earlier reports were closed.")}
                    </p>
                }
            })
            .into_any();
    }
    let dismiss = ServerAction::<DismissGameReports>::new();
    view! {
        <section>
            <h2 class="mb-3 font-display text-xl font-semibold">
                {format!("Open reports ({})", open.len())}
            </h2>
            <ul class="flex flex-col gap-2">
                {open
                    .into_iter()
                    .map(|r| {
                        let tone = if r.reason == ReportReason::Malware { "border border-bad/50" } else { "" };
                        view! {
                            <li class=format!("card p-3 text-sm {tone}")>
                                <div class="flex flex-wrap gap-x-3">
                                    <span class="font-medium">{r.reason.label()}</span>
                                    <span class="ml-auto text-faint">
                                        {format!(
                                            "{} · {}",
                                            r.reporter.map(|u| u.username).unwrap_or_else(|| "anonymous".into()),
                                            date_only(&r.created_at),
                                        )}
                                    </span>
                                </div>
                                {(!r.details.is_empty()).then(|| view! { <p class="mt-1 text-muted">{r.details.clone()}</p> })}
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
            <ActionForm action=dismiss attr:class="mt-3 flex flex-col gap-2 sm:flex-row sm:items-end">
                <input type="hidden" name="slug" value=slug />
                <div class="flex-1">
                    <label class="label" for="dismiss-notes">"Why no action (staff only)"</label>
                    <input id="dismiss-notes" name="notes" class="field" placeholder="Played it; the reports don't hold up" />
                </div>
                <button type="submit" class="btn-secondary" disabled=move || dismiss.pending().get()>
                    "Dismiss reports"
                </button>
            </ActionForm>
            <ActionError value=move || dismiss.value().get() />
            <p class="hint">"To act on them, delist the game. That closes them too."</p>
        </section>
    }
    .into_any()
}

#[component]
fn DelistForm(slug: String) -> impl IntoView {
    let delist = ServerAction::<DelistGame>::new();
    view! {
        <section class="card p-4">
            <h2 class="text-sm font-semibold">"Delist"</h2>
            <p class="mt-1 text-sm text-faint">
                "Takes the game off the catalog. The developer gets an email with your notes and can appeal once."
            </p>
            <ActionForm action=delist attr:class="mt-3 flex flex-col gap-3">
                <input type="hidden" name="slug" value=slug />
                <div>
                    <label class="label" for="delist-rule">"Guideline"</label>
                    <select id="delist-rule" name="rule" class="field" required>
                        <option value="">"Pick one"</option>
                        {POLICY_RULES
                            .into_iter()
                            .map(|r| view! { <option value=r.as_str()>{r.label()}</option> })
                            .collect_view()}
                    </select>
                </div>
                <div>
                    <label class="label" for="delist-notes">"Notes for the developer"</label>
                    <textarea id="delist-notes" name="notes" rows="3" class="field" required></textarea>
                </div>
                <label class="flex items-center gap-2 text-sm">
                    <input type="checkbox" name="strike" />
                    "Record a strike"
                </label>
                <ActionError value=move || delist.value().get() />
                <button type="submit" class="btn-danger" disabled=move || delist.pending().get()>
                    "Delist game"
                </button>
            </ActionForm>
        </section>
    }
}

#[component]
fn StrikesSection(slug: String, strikes: Vec<Strike>) -> impl IntoView {
    if strikes.is_empty() {
        return view! {
            <p class="card p-4 text-sm text-muted">"The developer has no strikes."</p>
        }
        .into_any();
    }
    let active = strikes.iter().filter(|s| s.is_active()).count();
    view! {
        <section class="card p-4">
            <h2 class="mb-3 text-sm font-semibold">
                {format!("Developer's strikes ({active} active)")}
            </h2>
            <ul class="flex flex-col gap-4">
                {strikes
                    .into_iter()
                    .map(|strike| {
                        let slug = slug.clone();
                        let revoke = strike.is_active().then(move || {
                            let action = ServerAction::<RevokeDeveloperStrike>::new();
                            let id = strike.id.to_string();
                            let field = format!("revoke-{id}");
                            view! {
                                <ActionForm action attr:class="mt-2 flex gap-2">
                                    <input type="hidden" name="slug" value=slug />
                                    <input type="hidden" name="strike" value=id />
                                    <label class="sr-only" for=field.clone()>"Reason"</label>
                                    <input id=field name="reason" class="field py-1 text-sm" placeholder="Why revoke it" required />
                                    <button type="submit" class="btn-secondary py-1 text-sm">"Revoke"</button>
                                </ActionForm>
                                <ActionError value=move || action.value().get() />
                            }
                        });
                        view! {
                            <li>
                                <StrikeEntry strike />
                                {revoke}
                            </li>
                        }
                    })
                    .collect_view()}
            </ul>
        </section>
    }
    .into_any()
}

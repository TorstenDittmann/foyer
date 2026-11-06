//! The public content guidelines that review decisions cite.

use leptos::prelude::*;
use leptos_meta::Title;

use crate::components::PageShell;

const EXCLUSIONS: &[(&str, &str)] = &[
    (
        "Sexual content",
        "No nudity, sexual content or fetish themes, including suggestive thumbnails and adult patches hosted elsewhere.",
    ),
    (
        "No-gameplay games",
        "Games that are mostly clicking through text and images with no meaningful mechanics or player decisions. Story-heavy games are welcome when choices or mechanics matter.",
    ),
    (
        "Hate and harassment",
        "No content promoting hate against protected groups.",
    ),
    (
        "Real-money gambling",
        "No cash-out gambling or loot boxes sold for money.",
    ),
    (
        "Asset flips and AI slop",
        "No unmodified store assets, template reskins, or games that are mostly unedited generated assets. Generative AI use must be disclosed.",
    ),
    (
        "Spam",
        "No near-duplicate listings, keyword-stuffed pages, or bundles of other people's work.",
    ),
];

const QUALITY: &[(&str, &str)] = &[
    (
        "Runs",
        "Launches and is playable on every platform it claims, with no crash in the first 15 minutes.",
    ),
    (
        "Complete experience",
        "A finished game, or an Early Access build with a stated roadmap and about 30 minutes of content.",
    ),
    (
        "Presentable page",
        "At least 3 real screenshots, a short description, accurate tags and cover art that matches the game.",
    ),
    (
        "Original work",
        "You own or have licensed the assets, code and music.",
    ),
    (
        "Honest metadata",
        "Content descriptors are set and accurate.",
    ),
];

fn rules(items: &'static [(&'static str, &'static str)], ordered: bool) -> impl IntoView {
    let list = items
        .iter()
        .map(|(name, text)| {
            view! {
                <li class="card p-4">
                    <p class="font-medium">{*name}</p>
                    <p class="mt-1 text-sm text-muted">{*text}</p>
                </li>
            }
        })
        .collect_view();
    if ordered {
        view! { <ol class="mt-4 grid gap-3 sm:grid-cols-2">{list}</ol> }.into_any()
    } else {
        view! { <ul class="mt-4 grid gap-3 sm:grid-cols-2">{list}</ul> }.into_any()
    }
}

#[component]
pub fn Guidelines() -> impl IntoView {
    view! {
        <Title text="Content guidelines · Foyer" />
        <PageShell>
            <div class="max-w-4xl">
                <h1 class="font-display text-3xl font-semibold">"Content guidelines"</h1>
                <p class="mt-3 max-w-prose text-muted">
                    "Every game on Foyer is played by a person before it's listed. Decisions cite one of the rules below, "
                    "and you can resubmit after changes or appeal once."
                </p>
                <section class="mt-10">
                    <h2 class="font-display text-xl font-semibold">"Never allowed"</h2>
                    {rules(EXCLUSIONS, false)}
                </section>
                <section class="mt-10">
                    <h2 class="font-display text-xl font-semibold">"Quality bar"</h2>
                    {rules(QUALITY, true)}
                </section>
            </div>
        </PageShell>
    }
}

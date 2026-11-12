//! `/app`: download page for the desktop client.
//!
//! The buttons point at `/app/download/{file}` (see `routes.rs`), which
//! redirects to the newest GitHub release, so this page never needs a new
//! deploy when the client is released.

use leptos::prelude::*;
use leptos_meta::{Meta, Title};

use crate::components::PageShell;

/// Release assets the download routes may redirect to: (route name, file).
pub const FILES: &[(&str, &str)] = &[
    ("windows", "Foyer-Setup-x64.exe"),
    ("macos", "Foyer-macos.dmg"),
    ("appimage", "Foyer-x86_64.AppImage"),
    ("deb", "Foyer-amd64.deb"),
];

/// Every release, with notes and checksums.
pub const RELEASES_PAGE: &str = "https://github.com/TorstenDittmann/foyer/releases";

const WINDOWS_ICON: &str = r#"<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path d="M3 5.5l7.5-1v7H3zM11.5 4.3L21 3v8.5h-9.5zM3 12.5h7.5v7L3 18.5zM11.5 12.5H21V21l-9.5-1.3z" fill="currentColor"/></svg>"#;
const APPLE_ICON: &str = r#"<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true"><path d="M16.4 12.6c0-2.4 2-3.5 2-3.6-1.1-1.6-2.8-1.8-3.4-1.8-1.4-.1-2.8.9-3.5.9s-1.8-.8-3-.8C6.9 7.3 5.4 8.2 4.6 9.7c-1.7 2.9-.4 7.2 1.2 9.6.8 1.2 1.7 2.4 2.9 2.4 1.2 0 1.6-.8 3-.8s1.8.8 3 .8c1.3 0 2.1-1.2 2.8-2.3.9-1.3 1.3-2.6 1.3-2.7 0 0-2.4-.9-2.4-3.9zM14.1 5.5c.6-.8 1.1-1.9 1-3-.9 0-2.1.6-2.7 1.4-.6.7-1.1 1.8-1 2.9 1 .1 2.1-.5 2.7-1.3z" fill="currentColor"/></svg>"#;
const LINUX_ICON: &str = r#"<svg viewBox="0 0 24 24" width="22" height="22" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"><path d="M12 2.5c-2.5 0-3.6 2-3.6 4.6 0 1.6-.2 2.6-1.3 4.2-1.3 1.9-2.7 4.3-2.4 6.8.9-.4 1.8-.2 2.3.6.5.8.2 1.8-.5 2.4h11c-.7-.6-1-1.6-.5-2.4.5-.8 1.4-1 2.3-.6.3-2.5-1.1-4.9-2.4-6.8-1.1-1.6-1.3-2.6-1.3-4.2 0-2.6-1.1-4.6-3.6-4.6z"/><circle cx="10.4" cy="7.2" r=".6" fill="currentColor"/><circle cx="13.6" cy="7.2" r=".6" fill="currentColor"/><path d="M10.8 9.2h2.4l-1.2 1z" fill="currentColor"/></svg>"#;

const FEATURES: &[(&str, &str)] = &[
    (
        "Small updates",
        "Only the parts of a game that changed are downloaded, often a few MB instead of the whole game.",
    ),
    (
        "Pause and resume",
        "Stop a download and pick it up later, even after a restart. Broken files are found and repaired.",
    ),
    (
        "No account",
        "Browse, install and play without signing up. Your library stays on your computer.",
    ),
];

#[component]
fn PlatformCard(
    icon: &'static str,
    name: &'static str,
    requirement: &'static str,
    href: &'static str,
    button: &'static str,
    children: Children,
) -> impl IntoView {
    view! {
        <li class="card flex flex-col gap-4 p-5">
            <div class="flex items-center gap-3">
                <span class="text-ink" inner_html=icon></span>
                <h2 class="font-display text-lg font-semibold">{name}</h2>
            </div>
            <p class="text-sm text-muted">{requirement}</p>
            // A plain link: the server redirects it to the release file.
            <a href=href rel="external" class="btn-primary w-full">
                {button}
            </a>
            <div class="mt-auto text-sm text-faint">{children()}</div>
        </li>
    }
}

#[component]
pub fn GetTheApp() -> impl IntoView {
    let features = FEATURES
        .iter()
        .map(|(title, text)| {
            view! {
                <li>
                    <p class="font-medium">{*title}</p>
                    <p class="mt-1 text-sm text-muted">{*text}</p>
                </li>
            }
        })
        .collect_view();

    view! {
        <Title text="Get the Foyer app · Foyer" />
        <Meta
            name="description"
            content="The Foyer desktop app installs indie games from Foyer and keeps them up to date with small patches. Free, no account needed."
        />
        <PageShell>
            <div class="max-w-4xl">
                <h1 class="font-display text-3xl font-semibold">"Get the Foyer app"</h1>
                <p class="mt-3 max-w-prose text-muted">
                    "Install any game on Foyer with one click and keep it up to date. "
                    "The app is free and needs no account."
                </p>
                <ul class="mt-8 grid gap-4 md:grid-cols-3">
                    <PlatformCard
                        icon=WINDOWS_ICON
                        name="Windows"
                        requirement="Windows 10 or 11, 64-bit"
                        href="/app/download/windows"
                        button="Download for Windows"
                    >
                        "Installs for your user only, no administrator needed."
                    </PlatformCard>
                    <PlatformCard
                        icon=APPLE_ICON
                        name="macOS"
                        requirement="macOS 11 or later, Apple Silicon and Intel"
                        href="/app/download/macos"
                        button="Download for macOS"
                    >
                        "Open the disk image and drag Foyer into Applications."
                    </PlatformCard>
                    <PlatformCard
                        icon=LINUX_ICON
                        name="Linux"
                        requirement="64-bit, most distributions"
                        href="/app/download/appimage"
                        button="Download AppImage"
                    >
                        "Make it executable and run it. On Debian or Ubuntu you can use the "
                        <a href="/app/download/deb" rel="external" class="link">
                            ".deb package"
                        </a>
                        " instead."
                    </PlatformCard>
                </ul>
                <ul class="mt-12 grid gap-6 sm:grid-cols-3">{features}</ul>
                <section class="mt-12 max-w-prose text-sm text-muted">
                    <h2 class="font-display text-xl font-semibold text-ink">"Updates"</h2>
                    <p class="mt-3">
                        "The app checks for new versions of itself and installs them when you restart it. "
                        "The .deb package is the exception: download the new one from this page."
                    </p>
                    <p class="mt-3">
                        "Older versions, release notes and checksums are on "
                        <a href=RELEASES_PAGE rel="external" class="link">
                            "GitHub"
                        </a>
                        "."
                    </p>
                </section>
            </div>
        </PageShell>
    }
}

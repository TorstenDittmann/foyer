//! Keeping Foyer itself up to date.
//!
//! The engine lives in `foyer_install::client_update`; this file wires it
//! into the app: check at startup and with every game update check, download
//! in the background when this install can replace itself, then offer
//! "Restart to update". Installs that can't (a `.deb`, a portable copy, a dev
//! build, or a build made without the release key) get a link to the
//! download page instead.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use foyer_install::client_update::{self, InstallKind, Release, Updater};
use gpui::{ClickEvent, Context, Div, FontWeight, Stateful, div, prelude::*, px};

use crate::app::Foyer;
use crate::runtime;
use crate::theme;
use crate::ui::{self, Icon, button, icon};

/// The version shown in Settings and compared against releases.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The minisign public key releases are signed with, set when building a
/// release (`FOYER_UPDATE_PUBLIC_KEY`, the second line of the `.pub` file).
const PUBLIC_KEY: Option<&str> = option_env!("FOYER_UPDATE_PUBLIC_KEY");

pub enum SelfUpdate {
    Idle,
    Checking,
    UpToDate,
    /// Newer version out; `can_install` says whether we can install it here.
    Available {
        release: Release,
        can_install: bool,
    },
    Downloading {
        release: Release,
        done: Arc<AtomicU64>,
        total: u64,
    },
    /// Downloaded and verified; restarting installs it.
    Ready {
        release: Release,
        file: PathBuf,
    },
    Failed {
        release: Option<Release>,
        error: String,
    },
}

impl SelfUpdate {
    /// The release on offer, whatever state it is in.
    pub fn release(&self) -> Option<&Release> {
        match self {
            Self::Available { release, .. }
            | Self::Downloading { release, .. }
            | Self::Ready { release, .. } => Some(release),
            Self::Failed { release, .. } => release.as_ref(),
            _ => None,
        }
    }

    /// Download progress from 0 to 1.
    pub fn fraction(&self) -> Option<f32> {
        match self {
            Self::Downloading { done, total, .. } if *total > 0 => {
                Some(done.load(Ordering::Relaxed) as f32 / *total as f32)
            }
            _ => None,
        }
    }
}

pub struct ClientUpdates {
    pub state: SelfUpdate,
    pub kind: InstallKind,
    updater: Option<Updater>,
    dir: PathBuf,
}

impl ClientUpdates {
    pub fn new(data_dir: &std::path::Path) -> Self {
        let url = std::env::var("FOYER_UPDATE_URL")
            .unwrap_or_else(|_| client_update::DEFAULT_MANIFEST_URL.to_string());
        let updater = Updater::new(&url, VERSION, PUBLIC_KEY)
            .inspect_err(|err| tracing::warn!("client updates are off: {err}"))
            .ok();
        let dir = data_dir.join("updates");
        // Whatever is in here was installed (or abandoned) last time.
        client_update::clean_downloads(&dir);
        Self {
            state: SelfUpdate::Idle,
            kind: InstallKind::detect(),
            updater,
            dir,
        }
    }

    /// Whether a new version can be downloaded and installed in place.
    pub fn can_install(&self) -> bool {
        self.kind.can_self_update()
            && self
                .updater
                .as_ref()
                .is_some_and(|u| u.verifies_signatures())
    }
}

impl Foyer {
    /// Looks for a newer Foyer. `manual` checks report "up to date" and
    /// errors; background checks stay quiet unless they find something.
    pub fn check_client_update(&mut self, manual: bool, cx: &mut Context<Self>) {
        let updates = &mut self.client_updates;
        if matches!(
            updates.state,
            SelfUpdate::Checking | SelfUpdate::Downloading { .. } | SelfUpdate::Ready { .. }
        ) {
            return;
        }
        let Some(updater) = updates.updater.clone() else {
            return;
        };
        updates.state = SelfUpdate::Checking;
        cx.notify();
        let request = runtime::spawn(async move { updater.check().await });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                let can_install = this.client_updates.can_install();
                this.client_updates.state = match result {
                    Ok(Ok(Some(release))) => SelfUpdate::Available {
                        // Platforms the release has no file for (e.g. ARM)
                        // get the download page instead.
                        can_install: can_install
                            && release.asset(&client_update::target()).is_some(),
                        release,
                    },
                    Ok(Ok(None)) => SelfUpdate::UpToDate,
                    Ok(Err(err)) => {
                        tracing::warn!("client update check failed: {err}");
                        if manual {
                            SelfUpdate::Failed {
                                release: None,
                                error: err.to_string(),
                            }
                        } else {
                            SelfUpdate::Idle
                        }
                    }
                    Err(_) => SelfUpdate::Idle,
                };
                if matches!(
                    this.client_updates.state,
                    SelfUpdate::Available {
                        can_install: true,
                        ..
                    }
                ) {
                    this.download_client_update(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Downloads and verifies the release on offer.
    pub fn download_client_update(&mut self, cx: &mut Context<Self>) {
        let updates = &mut self.client_updates;
        let (Some(updater), Some(release)) =
            (updates.updater.clone(), updates.state.release().cloned())
        else {
            return;
        };
        let total = release
            .asset(&client_update::target())
            .map(|a| a.size)
            .unwrap_or(0);
        let done = Arc::new(AtomicU64::new(0));
        updates.state = SelfUpdate::Downloading {
            release: release.clone(),
            done: done.clone(),
            total,
        };
        let dir = updates.dir.clone();
        let for_download = release.clone();
        let request = runtime::spawn(async move {
            updater
                .download(&for_download, &dir, |n, _| done.store(n, Ordering::Relaxed))
                .await
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.client_updates.state = match result {
                    Ok(Ok(file)) => SelfUpdate::Ready { release, file },
                    Ok(Err(client_update::UpdateError::NoAsset(_))) => SelfUpdate::Available {
                        release,
                        can_install: false,
                    },
                    Ok(Err(err)) => {
                        tracing::warn!("client update download failed: {err}");
                        SelfUpdate::Failed {
                            release: Some(release),
                            error: err.to_string(),
                        }
                    }
                    Err(err) => SelfUpdate::Failed {
                        release: Some(release),
                        error: err.to_string(),
                    },
                };
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Installs the downloaded update and quits; the new version starts on
    /// its own.
    pub fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        let SelfUpdate::Ready { file, .. } = &self.client_updates.state else {
            return;
        };
        if !self.jobs.is_empty() {
            self.notify_user(
                "Pause your downloads first. They pick up where they left off after the restart.",
                true,
                cx,
            );
            return;
        }
        let (kind, file) = (self.client_updates.kind.clone(), file.clone());
        match client_update::apply(&kind, &file) {
            Ok(()) => cx.quit(),
            Err(err) => self.notify_user(format!("Could not install the update: {err}"), true, cx),
        }
    }

    /// Opens the website's download page.
    pub fn open_download_page(&mut self, cx: &mut Context<Self>) {
        cx.open_url(client_update::DOWNLOAD_PAGE);
    }
}

// ---- views -------------------------------------------------------------------

impl Foyer {
    /// The sidebar card once a new version is out.
    pub fn client_update_card(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let state = &self.client_updates.state;
        let release = state.release()?;
        let title = format!("Foyer {} is out", release.version);
        let (detail, action): (String, Option<Stateful<Div>>) = match state {
            SelfUpdate::Downloading { .. } => (
                format!(
                    "Downloading… {}%",
                    (state.fraction().unwrap_or(0.) * 100.) as u32
                ),
                None,
            ),
            SelfUpdate::Ready { .. } => (
                "Ready to install.".into(),
                Some(
                    button("client-restart", "Restart to update")
                        .primary()
                        .full_width()
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.restart_to_update(cx)),
                        ),
                ),
            ),
            SelfUpdate::Available {
                can_install: false, ..
            } => (
                "Get it from foyer.lol.".into(),
                Some(
                    button("client-download", "Download")
                        .icon(Icon::External)
                        .full_width()
                        .on_click(
                            cx.listener(|this, _: &ClickEvent, _, cx| this.open_download_page(cx)),
                        ),
                ),
            ),
            SelfUpdate::Failed { .. } => (
                "The download failed.".into(),
                Some(
                    button("client-retry", "Try again")
                        .icon(Icon::Refresh)
                        .full_width()
                        .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                            if this.client_updates.can_install() {
                                this.download_client_update(cx)
                            } else {
                                this.open_download_page(cx)
                            }
                        })),
                ),
            ),
            _ => ("Preparing the update…".into(), None),
        };
        let fraction = state.fraction();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .mx(px(4.))
                .mb(px(6.))
                .p(px(10.))
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .text_xs()
                        .child(icon(Icon::Download, 14., theme::info()))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(title),
                        ),
                )
                .when_some(fraction, |d, f| d.child(ui::progress_bar(f, 4.)))
                .child(div().text_xs().text_color(theme::faint()).child(detail))
                .children(action),
        )
    }

    /// The "Foyer updates" row in Settings → About.
    pub fn client_update_row(&mut self, cx: &mut Context<Self>) -> Div {
        let updates = &self.client_updates;
        let status: String = match &updates.state {
            SelfUpdate::Idle => "Not checked yet".into(),
            SelfUpdate::Checking => "Checking…".into(),
            SelfUpdate::UpToDate => "Up to date".into(),
            SelfUpdate::Available { release, .. } => format!("{} available", release.version),
            SelfUpdate::Downloading { release, .. } => format!(
                "Downloading {} ({}%)",
                release.version,
                (updates.state.fraction().unwrap_or(0.) * 100.) as u32
            ),
            SelfUpdate::Ready { release, .. } => {
                format!("{} ready, restart to install", release.version)
            }
            SelfUpdate::Failed { error, .. } => format!("Failed: {error}"),
        };
        let how = match &updates.kind {
            _ if !updates.can_install() => "Updates come from foyer.lol/app",
            InstallKind::WindowsInstaller => "Installs updates by itself",
            InstallKind::AppImage(_) => "Replaces the AppImage by itself",
            InstallKind::MacBundle(_) => "Replaces the app by itself",
            InstallKind::Unmanaged => "Updates come from foyer.lol/app",
        };
        let busy = matches!(
            updates.state,
            SelfUpdate::Checking | SelfUpdate::Downloading { .. }
        );
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .py(px(7.))
            .border_b_1()
            .border_color(theme::line())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .min_w_0()
                    .child(div().text_color(theme::muted()).child("Foyer updates"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::faint())
                            .truncate()
                            .child(format!("{status} · {how}")),
                    ),
            )
            .child(
                button("client-check", "Check now")
                    .icon(Icon::Refresh)
                    .disabled(busy)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        if !busy {
                            this.check_client_update(true, cx)
                        }
                    })),
            )
    }
}

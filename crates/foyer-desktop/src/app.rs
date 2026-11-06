//! Application state and everything the views can ask it to do.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Child;
use std::time::Duration;

use chrono::Utc;
use foyer_install::api::InstalledBuild;
use foyer_install::install::{self, InstallJob, JobHandle};
use foyer_install::launch::{self, current_platform};
use foyer_install::{
    Api, BuildRef, DownloadEvent, GameQuery, InstallError, InstalledGame, Library,
};
use foyer_types::{ChannelStatus, Game, Platform};
use gpui::{Context, FocusHandle, PathPromptOptions, SharedString, Task, Window};

use crate::images::{self, Images};
use crate::runtime;
use crate::self_update::{ClientUpdates, SelfUpdate};
use crate::tester_keys::TesterKeys;

const UPDATE_INTERVAL: Duration = Duration::from_secs(30 * 60);
const TICK: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Discover,
    Game(String),
    Library,
    Downloads,
    Settings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobKind {
    Install,
    Update,
    Repair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AfterStop {
    /// Keep what was downloaded so the job can resume.
    Pause,
    /// Throw away what was downloaded.
    Cancel,
    /// Delete the game.
    Uninstall,
}

pub struct ActiveJob {
    pub kind: JobKind,
    pub handle: JobHandle,
    /// What to do once the job has stopped.
    pub after: AfterStop,
    _task: Task<()>,
}

/// The catalog grid and its filters.
#[derive(Default)]
pub struct Catalog {
    pub query: String,
    pub tag: Option<String>,
    pub games: Vec<Game>,
    pub next_cursor: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
    /// Increments with every new search so stale answers are dropped.
    generation: u64,
}

/// A game page's data.
#[derive(Default)]
pub struct GamePage {
    pub game: Option<Game>,
    pub channels: Option<Vec<ChannelStatus>>,
    pub error: Option<String>,
    /// The channel picked for installing.
    pub channel: Option<String>,
    /// Index of the screenshot shown large in the gallery.
    pub shot: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryFilter {
    #[default]
    All,
    Updates,
    Downloading,
}

pub struct Notice {
    pub text: SharedString,
    pub error: bool,
    id: u64,
}

pub struct Foyer {
    pub api: Api,
    pub data_dir: PathBuf,
    pub library: Library,
    pub screen: Screen,
    /// Where a game page's back button goes.
    pub back: Screen,
    pub catalog: Catalog,
    pub pages: HashMap<String, GamePage>,
    pub jobs: HashMap<String, ActiveJob>,
    /// Last error per game, shown on its page and in Downloads.
    pub failures: HashMap<String, String>,
    pub running: HashMap<String, Child>,
    pub notice: Option<Notice>,
    pub images: Images,
    pub search_focus: FocusHandle,
    pub checking_updates: bool,
    pub platform: Platform,
    /// Games whose "more" menu is open in the library.
    pub open_menus: HashSet<String>,
    pub library_filter: LibraryFilter,
    /// Updates for Foyer itself.
    pub client_updates: ClientUpdates,
    pub tester_keys: TesterKeys,
    notices: u64,
    _ticker: Option<Task<()>>,
}

impl Foyer {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let (data_dir, default_root) = foyer_install::library::default_dirs();
        let (library, load_error) = match Library::load(&data_dir, default_root.clone()) {
            Ok(library) => (library, None),
            Err(err) => {
                // Keep the damaged file for inspection and start fresh.
                let backup = data_dir.join("library.damaged.json");
                let _ = std::fs::rename(data_dir.join("library.json"), &backup);
                (
                    Library::load(&data_dir, default_root).expect("fresh library"),
                    Some(format!(
                        "Your library file was damaged and has been reset ({err})"
                    )),
                )
            }
        };
        let api_url = std::env::var("FOYER_API_URL")
            .ok()
            .or_else(|| library.data.settings.api_url.clone())
            .unwrap_or_else(|| foyer_install::api::DEFAULT_API_URL.to_string());
        let api = Api::new(&api_url).expect("HTTP client");
        api.set_tester_keys(&library.data.settings.tester_keys);

        let mut this = Self {
            api,
            images: Images::new(data_dir.join("cache").join("images")),
            client_updates: ClientUpdates::new(&data_dir),
            data_dir,
            library,
            screen: Screen::Discover,
            back: Screen::Discover,
            catalog: Catalog::default(),
            pages: HashMap::new(),
            jobs: HashMap::new(),
            failures: HashMap::new(),
            running: HashMap::new(),
            notice: load_error.map(|text| Notice {
                text: text.into(),
                error: true,
                id: 0,
            }),
            search_focus: cx.focus_handle(),
            tester_keys: TesterKeys::new(cx),
            checking_updates: false,
            platform: current_platform(),
            open_menus: HashSet::new(),
            library_filter: LibraryFilter::default(),
            notices: 0,
            _ticker: None,
        };
        this.search(cx);
        this.check_tester_keys(cx);
        this.start_ticker(cx);
        if this.library.data.settings.check_updates {
            this.check_updates(cx);
            this.check_client_update(false, cx);
        }
        this
    }

    /// Redraws while something is moving and runs periodic work.
    fn start_ticker(&mut self, cx: &mut Context<Self>) {
        self._ticker = Some(cx.spawn(async move |this, cx| {
            let mut since_check = Duration::ZERO;
            loop {
                cx.background_executor().timer(TICK).await;
                since_check += TICK;
                let check = since_check >= UPDATE_INTERVAL;
                if check {
                    since_check = Duration::ZERO;
                }
                let alive = this.update(cx, |this, cx| {
                    let finished: Vec<String> = this
                        .running
                        .iter_mut()
                        .filter_map(|(slug, child)| {
                            (!matches!(child.try_wait(), Ok(None))).then(|| slug.clone())
                        })
                        .collect();
                    for slug in &finished {
                        this.running.remove(slug);
                    }
                    if check && this.library.data.settings.check_updates {
                        this.check_updates(cx);
                        this.check_client_update(false, cx);
                    }
                    let updating_client =
                        matches!(this.client_updates.state, SelfUpdate::Downloading { .. });
                    if !this.jobs.is_empty() || !finished.is_empty() || updating_client {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        }));
    }

    pub fn navigate(&mut self, screen: Screen, cx: &mut Context<Self>) {
        if let Screen::Game(slug) = &screen {
            self.load_game(slug.clone(), cx);
            if !matches!(self.screen, Screen::Game(_)) {
                self.back = self.screen.clone();
            }
        }
        self.screen = screen;
        self.open_menus.clear();
        cx.notify();
    }

    pub fn notify_user(
        &mut self,
        text: impl Into<SharedString>,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        self.notices += 1;
        let id = self.notices;
        self.notice = Some(Notice {
            text: text.into(),
            error,
            id,
        });
        // Confirmations fade on their own; errors stay until dismissed.
        if !error {
            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(Duration::from_secs(5)).await;
                this.update(cx, |this, cx| {
                    if this.notice.as_ref().is_some_and(|n| n.id == id) {
                        this.notice = None;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        if let Err(err) = self.library.save() {
            self.notify_user(format!("Could not save your library: {err}"), true, cx);
        }
    }

    // ---- catalog ----------------------------------------------------------

    pub fn search(&mut self, cx: &mut Context<Self>) {
        self.catalog.games.clear();
        self.catalog.next_cursor = None;
        self.load_catalog_page(cx);
    }

    pub fn filter_tag(&mut self, tag: Option<String>, cx: &mut Context<Self>) {
        self.catalog.tag = tag;
        self.catalog.query.clear();
        self.screen = Screen::Discover;
        self.search(cx);
    }

    pub fn load_catalog_page(&mut self, cx: &mut Context<Self>) {
        self.catalog.generation += 1;
        let generation = self.catalog.generation;
        self.catalog.loading = true;
        self.catalog.error = None;
        let query = GameQuery {
            q: Some(self.catalog.query.clone()),
            tag: self.catalog.tag.clone(),
            cursor: self.catalog.next_cursor.clone(),
            limit: Some(48),
        };
        let api = self.api.clone();
        let request = runtime::spawn(async move { api.games(&query).await });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                if this.catalog.generation != generation {
                    return;
                }
                this.catalog.loading = false;
                match result {
                    Ok(Ok(page)) => {
                        this.catalog.games.extend(page.items);
                        this.catalog.next_cursor = page.next_cursor;
                    }
                    Ok(Err(err)) => this.catalog.error = Some(err.to_string()),
                    Err(err) => this.catalog.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    // ---- game pages -------------------------------------------------------

    pub fn load_game(&mut self, slug: String, cx: &mut Context<Self>) {
        let page = self.pages.entry(slug.clone()).or_default();
        page.error = None;
        if page.game.is_none()
            && let Some(game) = self.catalog.games.iter().find(|g| g.slug == slug)
        {
            page.game = Some(game.clone());
        }
        let api = self.api.clone();
        let request = {
            let slug = slug.clone();
            runtime::spawn(async move {
                let game = api.game(&slug).await;
                let channels = api.channels(&slug).await;
                (game, channels)
            })
        };
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                let platform = this.platform;
                let page = this.pages.entry(slug).or_default();
                match result {
                    Ok((game, channels)) => {
                        match game {
                            Ok(game) => page.game = Some(game),
                            Err(err) => page.error = Some(err.to_string()),
                        }
                        if let Ok(channels) = channels {
                            if page.channel.is_none() {
                                page.channel = foyer_install::default_channel(&channels, platform)
                                    .map(|c| c.channel.clone());
                            }
                            page.channels = Some(channels);
                        }
                    }
                    Err(err) => page.error = Some(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ---- images -----------------------------------------------------------

    /// The image for `url` if it is cached; starts a download otherwise.
    pub fn image(&mut self, url: &str, cx: &mut Context<Self>) -> Option<PathBuf> {
        match self.images.get(url) {
            Ok(path) => Some(path),
            Err(false) => None,
            Err(true) => {
                let path = self.images.path_for(url);
                let request =
                    runtime::spawn(images::download(self.api.clone(), url.to_string(), path));
                let url = url.to_string();
                cx.spawn(async move |this, cx| {
                    let result = request.await.ok().flatten();
                    this.update(cx, |this, cx| {
                        this.images.finish(&url, result);
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
                None
            }
        }
    }

    // ---- installs and updates ----------------------------------------------

    /// Add `game` to the library on `channel` and start installing it.
    pub fn install(&mut self, game: &Game, channel: &ChannelStatus, cx: &mut Context<Self>) {
        let Some(build) = &channel.latest else {
            return;
        };
        if self.library.get(&game.slug).is_some() {
            return;
        }
        let dir = self.library.new_install_dir(&game.slug);
        let mut entry = InstalledGame::new(game, &channel.channel, channel.platform, dir);
        entry.pending = Some(BuildRef::from(build));
        self.library.upsert(entry);
        self.save(cx);
        self.start_job(&game.slug, JobKind::Install, cx);
    }

    /// Start (or resume) the job that brings a game to its pending build.
    pub fn start_job(&mut self, slug: &str, kind: JobKind, cx: &mut Context<Self>) {
        if self.jobs.contains_key(slug) {
            return;
        }
        let Some(game) = self.library.get_mut(slug) else {
            return;
        };
        let target = match kind {
            JobKind::Repair => game.installed.clone(),
            _ => game.pending.clone().or_else(|| game.update.clone()),
        };
        let Some(target) = target else {
            return;
        };
        if kind != JobKind::Repair {
            game.pending = Some(target.clone());
        }
        let event = match kind {
            JobKind::Install if game.installed.is_none() => Some(DownloadEvent::Install),
            JobKind::Install | JobKind::Update => Some(DownloadEvent::Update),
            JobKind::Repair => None,
        };
        let job = InstallJob {
            install_dir: game.install_dir.clone(),
            build_id: target.id,
            event,
            repair: kind == JobKind::Repair,
        };
        self.failures.remove(slug);
        self.save(cx);

        let handle = JobHandle::new();
        let api = self.api.clone();
        let request = {
            let handle = handle.clone();
            runtime::spawn(async move { install::install(&api, &job, &handle).await })
        };
        let slug_owned = slug.to_string();
        let task = cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                let result = result.unwrap_or_else(|err| {
                    Err(InstallError::Download(format!(
                        "the install task failed: {err}"
                    )))
                });
                this.finish_job(&slug_owned, target, result, cx);
            })
            .ok();
        });
        self.jobs.insert(
            slug.to_string(),
            ActiveJob {
                kind,
                handle,
                after: AfterStop::Pause,
                _task: task,
            },
        );
        cx.notify();
    }

    fn finish_job(
        &mut self,
        slug: &str,
        target: BuildRef,
        result: Result<foyer_install::Installed, InstallError>,
        cx: &mut Context<Self>,
    ) {
        let Some(job) = self.jobs.remove(slug) else {
            return;
        };
        let Some(game) = self.library.get_mut(slug) else {
            return;
        };
        match result {
            // Stopping is not possible once files are being swapped in, so
            // the job can finish even though the player asked to uninstall.
            Ok(_) if job.after == AfterStop::Uninstall => {
                self.uninstall(slug, cx);
            }
            // A cancelled first install that slipped past the last stop
            // check is removed; a cancelled update that already swapped in
            // is kept, since the old files are gone.
            Ok(_) if job.after == AfterStop::Cancel && game.installed.is_none() => {
                self.uninstall(slug, cx);
            }
            Ok(done) => {
                let first = game.installed.is_none();
                game.installed = Some(BuildRef {
                    id: done.build_id,
                    ..target.clone()
                });
                // A repair leaves a paused update's resume point alone.
                if game.pending.as_ref().is_some_and(|p| p.id == done.build_id) {
                    game.pending = None;
                }
                if game.update.as_ref().is_some_and(|u| u.id == done.build_id) {
                    game.update = None;
                    game.update_download = None;
                }
                game.updated_at = Some(Utc::now());
                let launch_ok = game
                    .launch
                    .as_ref()
                    .is_some_and(|p| done.manifest.files.contains_key(p));
                if !launch_ok {
                    game.launch_chosen = false;
                }
                // The developer's launch target wins over guessing, but not
                // over the player's own choice.
                let declared = target
                    .launch
                    .as_ref()
                    .filter(|t| done.manifest.files.contains_key(&t.executable));
                if let Some(t) = declared.filter(|_| !game.launch_chosen) {
                    game.launch = Some(t.executable.clone());
                    game.launch_args = t.args.clone();
                } else if !launch_ok {
                    let dir = game.install_dir.clone();
                    game.launch_args = Vec::new();
                    game.launch = launch::executable_candidates(
                        &done.manifest,
                        game.platform,
                        &game.slug,
                        |p| launch::has_exec_bit(&launch::resolve(&dir, p)),
                    )
                    .into_iter()
                    .next();
                }
                let title = game.title.clone();
                let text = match job.kind {
                    JobKind::Install if first => format!("Installed: {title}."),
                    JobKind::Repair => format!("Repaired: {title}."),
                    _ => format!("Updated: {title}."),
                };
                self.save(cx);
                self.notify_user(text, false, cx);
            }
            Err(InstallError::Stopped) if job.after == AfterStop::Uninstall => {
                self.uninstall(slug, cx);
            }
            // Cancelling a repair just stops it; there is nothing to throw away.
            Err(InstallError::Stopped)
                if job.after == AfterStop::Cancel && job.kind != JobKind::Repair =>
            {
                let dir = game.install_dir.clone();
                let never_installed = game.installed.is_none();
                game.pending = None;
                if never_installed {
                    self.library.remove(slug);
                }
                self.save(cx);
                runtime::spawn_blocking(move || {
                    if let Err(err) = install::discard(&dir) {
                        tracing::warn!("could not clean up {}: {err}", dir.display());
                    }
                });
            }
            Err(InstallError::Stopped) => {
                // Paused; the pending build stays so it can resume.
                self.save(cx);
            }
            Err(err) => {
                let title = game.title.clone();
                self.failures.insert(slug.to_string(), err.to_string());
                self.save(cx);
                self.notify_user(format!("{title} did not finish: {err}"), true, cx);
            }
        }
        cx.notify();
    }

    pub fn pause(&mut self, slug: &str, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.get(slug) {
            job.handle.stop();
        }
        cx.notify();
    }

    /// Stop and throw away a download. A game that was never fully
    /// installed leaves the library.
    pub fn cancel(&mut self, slug: &str, cx: &mut Context<Self>) {
        if let Some(job) = self.jobs.get_mut(slug) {
            job.after = AfterStop::Cancel;
            job.handle.stop();
            return;
        }
        let Some(game) = self.library.get_mut(slug) else {
            return;
        };
        let dir = game.install_dir.clone();
        game.pending = None;
        if game.installed.is_none() {
            self.library.remove(slug);
        }
        self.failures.remove(slug);
        self.save(cx);
        runtime::spawn_blocking(move || install::discard(&dir));
        cx.notify();
    }

    pub fn uninstall(&mut self, slug: &str, cx: &mut Context<Self>) {
        // A running job lets go of its files first, then comes back here.
        if let Some(job) = self.jobs.get_mut(slug) {
            job.after = AfterStop::Uninstall;
            job.handle.stop();
            return;
        }
        let Some(game) = self.library.remove(slug) else {
            return;
        };
        self.failures.remove(slug);
        self.save(cx);
        let dir = game.install_dir.clone();
        let request = runtime::spawn_blocking(move || install::uninstall(&dir));
        let title = game.title.clone();
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| match result {
                Ok(Ok(())) => this.notify_user(format!("Uninstalled: {title}."), false, cx),
                Ok(Err(err)) => {
                    this.notify_user(format!("Could not uninstall {title}: {err}"), true, cx)
                }
                Err(err) => {
                    this.notify_user(format!("Could not uninstall {title}: {err}"), true, cx)
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    pub fn play(&mut self, slug: &str, cx: &mut Context<Self>) {
        if self.running.contains_key(slug) {
            return;
        }
        let Some(game) = self.library.get_mut(slug) else {
            return;
        };
        let Some(exe) = game.launch.clone() else {
            let title = game.title.clone();
            self.notify_user(
                format!(
                    "Could not find how to start {title}. Use \"Choose executable\" in its menu."
                ),
                true,
                cx,
            );
            return;
        };
        match launch::launch(&game.install_dir, &exe, &game.launch_args) {
            Ok(child) => {
                game.last_played = Some(Utc::now());
                self.running.insert(slug.to_string(), child);
                self.save(cx);
            }
            Err(err) => {
                let title = game.title.clone();
                self.notify_user(format!("Could not start {title}: {err}"), true, cx);
            }
        }
        cx.notify();
    }

    pub fn choose_executable(&mut self, slug: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(game) = self.library.get(slug) else {
            return;
        };
        let dir = game.install_dir.clone();
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose the game's executable".into()),
        });
        let slug = slug.to_string();
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                let Ok(relative) = path.strip_prefix(&dir) else {
                    this.notify_user("Pick a file inside the game's folder.", true, cx);
                    return;
                };
                let relative = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                if let Some(game) = this.library.get_mut(&slug) {
                    game.launch = Some(relative);
                    game.launch_args = Vec::new();
                    game.launch_chosen = true;
                }
                this.save(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn open_folder(&self, slug: &str, cx: &mut Context<Self>) {
        if let Some(game) = self.library.get(slug) {
            cx.open_with_system(&game.install_dir);
        }
    }

    // ---- updates ----------------------------------------------------------

    pub fn check_updates(&mut self, cx: &mut Context<Self>) {
        if self.checking_updates {
            return;
        }
        let installed: Vec<(InstalledBuild, PathBuf)> = self
            .library
            .data
            .games
            .values()
            .filter_map(|g| {
                Some((
                    InstalledBuild {
                        game: g.slug.clone(),
                        channel: g.channel.clone(),
                        build_id: g.installed.as_ref()?.id,
                    },
                    g.install_dir.clone(),
                ))
            })
            .collect();
        if installed.is_empty() {
            return;
        }
        self.checking_updates = true;
        let api = self.api.clone();
        let request = runtime::spawn(async move {
            let builds: Vec<InstalledBuild> = installed.iter().map(|(b, _)| b.clone()).collect();
            let res = api.check_updates(&builds).await?;
            // How much each update downloads, from the two manifests.
            let mut sizes = HashMap::new();
            for update in &res.updates {
                if let Some((_, dir)) = installed.iter().find(|(b, _)| b.game == update.game)
                    && let Ok(manifest) = api.manifest(update.latest.id).await
                {
                    let dir = dir.clone();
                    let size = tokio::task::spawn_blocking(move || {
                        install::update_download_size(&dir, &manifest)
                    })
                    .await
                    .unwrap_or(0);
                    sizes.insert(update.game.clone(), size);
                }
            }
            Ok::<_, foyer_install::ApiFailure>((res, sizes))
        });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.checking_updates = false;
                let (res, sizes) = match result {
                    Ok(Ok(found)) => found,
                    Ok(Err(err)) => {
                        tracing::warn!("update check failed: {err}");
                        return;
                    }
                    Err(_) => return,
                };
                for game in this.library.data.games.values_mut() {
                    game.unavailable = res.unavailable.iter().any(|u| u.game == game.slug);
                    match res.updates.iter().find(|u| u.game == game.slug) {
                        Some(update) => {
                            game.update = Some(BuildRef::from(&update.latest));
                            game.update_download = sizes.get(&game.slug).copied();
                        }
                        None => {
                            game.update = None;
                            game.update_download = None;
                        }
                    }
                }
                this.save(cx);
                if this.library.data.settings.auto_update {
                    this.update_all(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn update_all(&mut self, cx: &mut Context<Self>) {
        let slugs: Vec<String> = self
            .library
            .data
            .games
            .values()
            .filter(|g| g.update.is_some() && !self.running.contains_key(&g.slug))
            .map(|g| g.slug.clone())
            .collect();
        for slug in slugs {
            self.start_job(&slug, JobKind::Update, cx);
        }
    }

    // ---- settings ---------------------------------------------------------

    pub fn choose_install_root(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Install new games here".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| {
                this.library.data.settings.install_root = path;
                this.save(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    pub fn toggle_check_updates(&mut self, cx: &mut Context<Self>) {
        let settings = &mut self.library.data.settings;
        settings.check_updates = !settings.check_updates;
        if !settings.check_updates {
            settings.auto_update = false;
        }
        self.save(cx);
        cx.notify();
    }

    pub fn toggle_auto_update(&mut self, cx: &mut Context<Self>) {
        let settings = &mut self.library.data.settings;
        settings.auto_update = !settings.auto_update;
        if settings.auto_update {
            settings.check_updates = true;
        }
        self.save(cx);
        cx.notify();
    }
}

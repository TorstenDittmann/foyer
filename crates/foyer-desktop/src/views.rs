//! The screens: discover, game page, library, downloads and settings.

use std::collections::HashMap;

use foyer_install::launch::current_platform;
use foyer_install::{InstalledGame, Phase, Progress};
use foyer_types::{
    ContentDescriptor, Game, MediaKind, PageBlock, PageStyle, Palette, Platform, SupportKind,
    TitleStyle, trailer_embed,
};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, KeyDownEvent, ObjectFit, Rgba, SharedString,
    Stateful, Window, div, img, prelude::*, px, rgb,
};

use crate::app::{Foyer, JobKind, LibraryFilter, Screen};
use crate::theme;
use crate::ui::{self, ButtonStyle, Icon, button, chip, icon, icon_button};

const SIDEBAR_W: f32 = 232.;
const PAGE_PAD: f32 = 32.;
const GAP: f32 = 16.;
/// Capsule art is 460 × 215.
const CAPSULE_RATIO: f32 = 215. / 460.;
const PANEL_W: f32 = 320.;
const PAGE_MAX: f32 = 1120.;

/// Width available to a screen's content.
fn content_width(window: &Window) -> f32 {
    let w = f32::from(window.viewport_size().width) - SIDEBAR_W - PAGE_PAD * 2. - 4.;
    w.clamp(320., PAGE_MAX)
}

/// Columns and card width for a grid of cards at least `min` wide.
fn grid(window: &Window, min: f32) -> (usize, f32) {
    let w = content_width(window);
    let cols = ((w + GAP) / (min + GAP)).floor().max(1.) as usize;
    let card = (w - GAP * (cols as f32 - 1.)) / cols as f32;
    (cols, card.floor())
}

/// Where a game in the library stands, for badges and controls.
#[derive(Debug, Clone, PartialEq)]
enum GameState {
    Working {
        kind: JobKind,
        progress: Progress,
        stopping: bool,
    },
    Paused {
        failed: bool,
    },
    Running,
    UpdateReady,
    Ready,
}

impl GameState {
    fn badge(&self) -> Option<(String, Rgba)> {
        Some(match self {
            GameState::Working { kind, progress, .. } => (
                format!(
                    "{} {}%",
                    match kind {
                        JobKind::Repair => "Repairing",
                        JobKind::Update => "Updating",
                        JobKind::Install => "Downloading",
                    },
                    (progress.fraction() * 100.) as u32
                ),
                theme::accent(),
            ),
            GameState::Paused { failed: true } => ("Stopped".into(), theme::bad()),
            GameState::Paused { failed: false } => ("Paused".into(), theme::warn()),
            GameState::Running => ("Playing".into(), theme::good()),
            GameState::UpdateReady => ("Update".into(), theme::info()),
            GameState::Ready => return None,
        })
    }

    fn fraction(&self) -> Option<f32> {
        match self {
            GameState::Working { progress, .. } => Some(progress.fraction()),
            _ => None,
        }
    }
}

impl Render for Foyer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (key, content): (&str, AnyElement) = match self.screen.clone() {
            Screen::Discover => ("discover", self.discover(window, cx).into_any_element()),
            Screen::Game(slug) => ("game", self.game_page(&slug, window, cx).into_any_element()),
            Screen::Library => (
                "library",
                self.library_screen(window, cx).into_any_element(),
            ),
            Screen::Downloads => ("downloads", self.downloads(cx).into_any_element()),
            Screen::Settings => ("settings", self.settings(cx).into_any_element()),
        };
        let scroll_id = match &self.screen {
            Screen::Game(slug) => SharedString::from(format!("content-game-{slug}")),
            _ => SharedString::from(format!("content-{key}")),
        };

        div()
            .relative()
            .flex()
            .size_full()
            .bg(theme::canvas())
            .text_color(theme::ink())
            .font_family(theme::FONT_SANS)
            .text_sm()
            .child(self.sidebar(cx))
            .child(
                div()
                    .id(scroll_id)
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_y_scroll()
                    .child(
                        div()
                            .w_full()
                            .px(px(PAGE_PAD))
                            .pt(px(28.))
                            .pb(px(48.))
                            .child(div().w_full().max_w(px(PAGE_MAX)).child(content)),
                    ),
            )
            .children(self.toast(cx))
    }
}

impl Foyer {
    fn state_of(&self, slug: &str) -> Option<GameState> {
        let game = self.library.get(slug)?;
        if let Some(job) = self.jobs.get(slug) {
            return Some(GameState::Working {
                kind: job.kind,
                progress: job.handle.progress(),
                stopping: job.handle.is_stopped(),
            });
        }
        if game.pending.is_some() {
            return Some(GameState::Paused {
                failed: self.failures.contains_key(slug),
            });
        }
        if !game.is_installed() {
            return None;
        }
        Some(if self.running.contains_key(slug) {
            GameState::Running
        } else if game.update.is_some() {
            GameState::UpdateReady
        } else {
            GameState::Ready
        })
    }

    // ---- frame ------------------------------------------------------------

    fn sidebar(&mut self, cx: &mut Context<Self>) -> Div {
        let downloads = self.jobs.len()
            + self
                .library
                .data
                .games
                .values()
                .filter(|g| g.pending.is_some() && !self.jobs.contains_key(&g.slug))
                .count();
        let updates = self
            .library
            .data
            .games
            .values()
            .filter(|g| g.update.is_some())
            .count();

        let selected = |screen: &Screen| match &self.screen {
            // Installed games are highlighted in the list below instead.
            Screen::Game(slug) => self.library.get(slug).is_none() && &self.back == screen,
            current => current == screen,
        };
        let nav = [
            (
                "nav-discover",
                Icon::Compass,
                "Discover",
                Screen::Discover,
                None,
            ),
            (
                "nav-library",
                Icon::Library,
                "Library",
                Screen::Library,
                (updates > 0).then_some((updates, theme::info())),
            ),
            (
                "nav-downloads",
                Icon::Download,
                "Downloads",
                Screen::Downloads,
                (downloads > 0).then_some((downloads, theme::accent())),
            ),
        ];
        let nav: Vec<AnyElement> = nav
            .into_iter()
            .map(|(id, i, label, screen, count)| {
                let on = selected(&screen);
                nav_item(id, i, label, screen, on, count, cx).into_any_element()
            })
            .collect();
        let settings_on = selected(&Screen::Settings);
        let settings = nav_item(
            "nav-settings",
            Icon::Settings,
            "Settings",
            Screen::Settings,
            settings_on,
            None,
            cx,
        );

        // Installed games, most recently played first.
        let mut games: Vec<InstalledGame> = self.library.data.games.values().cloned().collect();
        games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        let total = games.len();
        let shortcuts: Vec<AnyElement> = games
            .into_iter()
            .take(8)
            .map(|game| {
                let image = game.capsule_url.clone().and_then(|u| self.image(&u, cx));
                let state = self.state_of(&game.slug);
                let on = self.screen == Screen::Game(game.slug.clone());
                let slug = game.slug.clone();
                let dot = match state {
                    Some(GameState::Running) => Some(theme::good()),
                    Some(GameState::UpdateReady) => Some(theme::info()),
                    Some(GameState::Working { .. }) => Some(theme::accent()),
                    Some(GameState::Paused { .. }) => Some(theme::warn()),
                    _ => None,
                };
                div()
                    .id(SharedString::from(format!("side-{slug}")))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(theme::RADIUS_CONTROL)
                    .cursor_pointer()
                    .text_color(if on { theme::ink() } else { theme::muted() })
                    .when(on, |d| d.bg(theme::raised()))
                    .hover(|s| s.bg(theme::raised()).text_color(theme::ink()))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(Screen::Game(slug.clone()), cx)
                    }))
                    .child(div().flex_none().size(px(24.)).child(ui::cover(
                        SharedString::from(format!("side-img-{}", game.slug)),
                        image,
                        px(5.),
                    )))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(game.title.clone()),
                    )
                    .children(dot.map(|c| div().flex_none().size(px(6.)).rounded_full().bg(c)))
                    .into_any_element()
            })
            .collect();

        // The download in progress, if any.
        let active = self.jobs.iter().next().and_then(|(slug, job)| {
            let game = self.library.get(slug)?;
            Some((game.title.clone(), job.kind, job.handle.progress()))
        });
        let active = active.map(|(title, kind, p)| {
            div()
                .id("side-download")
                .flex()
                .flex_col()
                .gap(px(6.))
                .mx(px(4.))
                .p(px(10.))
                .rounded(theme::RADIUS_CARD)
                .bg(theme::raised())
                .cursor_pointer()
                .hover(|s| s.bg(theme::line()))
                .on_click(
                    cx.listener(|this, _: &ClickEvent, _, cx| this.navigate(Screen::Downloads, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .text_xs()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_weight(FontWeight::MEDIUM)
                                .child(title),
                        )
                        .child(
                            ui::mono(format!("{}%", (p.fraction() * 100.) as u32))
                                .flex_none()
                                .text_color(theme::muted()),
                        ),
                )
                .child(ui::progress_bar(p.fraction(), 4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::faint())
                        .child(phase_label(kind, &p, false)),
                )
        });

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR_W))
            .h_full()
            .px(px(12.))
            .pt(px(18.))
            .pb(px(12.))
            .gap(px(2.))
            .bg(theme::chrome())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(8.))
                    .pb(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .rounded(px(7.))
                            .bg(theme::accent())
                            .child(icon(Icon::Logo, 18., theme::accent_ink())),
                    )
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Foyer"),
                    ),
            )
            .children(nav)
            .when(!shortcuts.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(8.))
                        .pt(px(22.))
                        .pb(px(6.))
                        .child(ui::eyebrow("Installed"))
                        .child(
                            ui::mono(total.to_string())
                                .text_xs()
                                .text_color(theme::faint()),
                        ),
                )
                .child(
                    div()
                        .id("side-games")
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .children(shortcuts),
                )
            })
            .when(total == 0, |d| d.child(div().flex_1()))
            .children(active)
            .child(div().h(px(8.)))
            .child(settings)
    }

    fn toast(&mut self, cx: &mut Context<Self>) -> Option<Div> {
        let notice = self.notice.as_ref()?;
        let color = if notice.error {
            theme::bad()
        } else {
            theme::good()
        };
        Some(
            div()
                .absolute()
                .bottom(px(20.))
                .right(px(20.))
                .flex()
                .items_center()
                .gap(px(10.))
                .max_w(px(460.))
                .pl(px(14.))
                .pr(px(6.))
                .py(px(6.))
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::line())
                .bg(theme::raised())
                .child(icon(
                    if notice.error {
                        Icon::Alert
                    } else {
                        Icon::Check
                    },
                    16.,
                    color,
                ))
                .child(div().flex_1().min_w_0().child(notice.text.clone()))
                .child(icon_button("dismiss-notice", Icon::X).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    },
                ))),
        )
    }

    // ---- discover ---------------------------------------------------------

    fn search_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let focused = self.search_focus.is_focused(window);
        let empty = self.catalog.query.is_empty();
        div()
            .id("search")
            .track_focus(&self.search_focus)
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.))
            .w(px(300.))
            .h(px(36.))
            .px(px(12.))
            .rounded(theme::RADIUS_CONTROL)
            .border_1()
            .border_color(if focused {
                theme::muted()
            } else {
                theme::line()
            })
            .bg(theme::surface())
            .cursor_text()
            .on_click(cx.listener(|this, _, window, _| window.focus(&this.search_focus)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = &event.keystroke;
                match key.key.as_str() {
                    "enter" => this.search(cx),
                    "escape" => {
                        this.catalog.query.clear();
                        this.search(cx);
                    }
                    "backspace" => {
                        this.catalog.query.pop();
                    }
                    _ => {
                        let m = &key.modifiers;
                        if m.control || m.platform || m.alt {
                            return;
                        }
                        if let Some(text) = &key.key_char
                            && this.catalog.query.chars().count() < 100
                        {
                            this.catalog.query.push_str(text);
                        }
                    }
                }
                cx.notify();
            }))
            .child(icon(Icon::Search, 15., theme::faint()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .text_color(if empty { theme::faint() } else { theme::ink() })
                    .child(div().truncate().child(if empty && !focused {
                        SharedString::from("Search games")
                    } else {
                        SharedString::from(self.catalog.query.clone())
                    }))
                    .when(focused, |d| {
                        d.child(div().flex_none().w(px(1.)).h(px(15.)).bg(theme::ink()))
                    }),
            )
            .when(!empty, |d| {
                d.child(
                    div()
                        .id("clear-search")
                        .flex_none()
                        .p(px(3.))
                        .rounded(theme::RADIUS_CONTROL)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::raised()))
                        .child(icon(Icon::X, 13., theme::muted()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.catalog.query.clear();
                            this.search(cx);
                        })),
                )
            })
    }

    fn discover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let games: Vec<Game> = self.catalog.games.clone();
        let filtered = !self.catalog.query.is_empty() || self.catalog.tag.is_some();
        let (_, card_w) = grid(window, 228.);

        // The banner rotates through the first few games.
        let pool: Vec<Game> = games.iter().take(5).cloned().collect();
        let hero = (!filtered && !pool.is_empty()).then(|| {
            let i = self.featured % pool.len();
            self.hero(&pool[i], i, pool.len(), window, cx)
        });
        let shelf = (!filtered).then(|| self.jump_back_in(window, cx)).flatten();

        // Popular tags in the loaded catalog.
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for tag in games.iter().flat_map(|g| g.tags.iter()) {
            *counts.entry(tag.as_str()).or_default() += 1;
        }
        let mut tags: Vec<(&str, usize)> = counts.into_iter().collect();
        tags.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        let mut tags: Vec<String> = tags.into_iter().take(10).map(|t| t.0.to_string()).collect();
        if let Some(tag) = &self.catalog.tag
            && !tags.contains(tag)
        {
            tags.insert(0, tag.clone());
        }
        let current_tag = self.catalog.tag.clone();
        let chips: Vec<AnyElement> = std::iter::once(None)
            .chain(tags.into_iter().map(Some))
            .map(|tag| {
                let active = tag == current_tag;
                let label = tag.clone().unwrap_or_else(|| "All".into());
                div()
                    .id(SharedString::from(format!(
                        "tag-chip-{}",
                        tag.as_deref().unwrap_or("")
                    )))
                    .cursor_pointer()
                    .child(chip(label, active).when(!active, |c| {
                        c.hover(|s| s.bg(theme::line()).text_color(theme::ink()))
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.filter_tag(tag.clone(), cx)))
                    .into_any_element()
            })
            .collect();

        let title = if !self.catalog.query.is_empty() {
            format!("Results for “{}”", self.catalog.query)
        } else if let Some(tag) = &self.catalog.tag {
            format!("Tagged {tag}")
        } else {
            "All games".to_string()
        };

        let body: AnyElement = if let Some(err) = self.catalog.error.clone() {
            empty_state(
                Icon::Alert,
                "Could not load games",
                format!("{} answered: {err}", self.api.base_url()),
            )
            .child(
                button("retry", "Try again")
                    .icon(Icon::Refresh)
                    .on_click(cx.listener(|this, _, _, cx| this.load_catalog_page(cx))),
            )
            .into_any_element()
        } else if self.catalog.loading && games.is_empty() {
            div()
                .flex()
                .flex_wrap()
                .gap(px(GAP))
                .children((0..8).map(|i| skeleton_card(i, card_w)))
                .into_any_element()
        } else if games.is_empty() {
            empty_state(Icon::Search, "No games match", "Try another search or tag.")
                .child(
                    button("clear-filters", "Show all games").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.catalog.query.clear();
                            this.filter_tag(None, cx);
                        },
                    )),
                )
                .into_any_element()
        } else {
            let cards: Vec<AnyElement> = games
                .iter()
                .map(|game| self.game_card(game, card_w, cx).into_any_element())
                .collect();
            div()
                .flex()
                .flex_wrap()
                .gap(px(GAP))
                .children(cards)
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(ui::heading("Discover"))
                            .child(ui::muted(
                                "Free games from independent developers. No account needed.",
                            )),
                    )
                    .child(self.search_box(window, cx)),
            )
            .children(hero)
            .children(shelf)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(ui::section_title(title))
                            .when(!games.is_empty(), |d| {
                                d.child(
                                    ui::mono(format!(
                                        "{}{} game{}",
                                        games.len(),
                                        if self.catalog.next_cursor.is_some() {
                                            "+"
                                        } else {
                                            ""
                                        },
                                        if games.len() == 1 { "" } else { "s" }
                                    ))
                                    .text_xs()
                                    .text_color(theme::faint()),
                                )
                            }),
                    )
                    .when(chips.len() > 1, |d| {
                        d.child(div().flex().flex_wrap().gap(px(6.)).children(chips))
                    })
                    .child(body),
            )
            .when(self.catalog.next_cursor.is_some(), |d| {
                let loading = self.catalog.loading;
                d.child(
                    div().flex().justify_center().pt_2().child(
                        button("more", if loading { "Loading…" } else { "Load more" })
                            .disabled(loading)
                            .on_click(cx.listener(|this, _, _, cx| this.load_catalog_page(cx))),
                    ),
                )
            })
    }

    fn hero(
        &mut self,
        game: &Game,
        index: usize,
        count: usize,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let art = hero_art(game).and_then(|u| self.image(&u, cx));
        let slug = game.slug.clone();
        let state = self.state_of(&game.slug);
        let height = (content_width(window) * 0.38).clamp(280., 400.);
        let dots: Vec<AnyElement> = (0..count)
            .map(|i| {
                let on = i == index;
                div()
                    .id(("hero-dot", i))
                    .h(px(6.))
                    .w(px(if on { 22. } else { 6. }))
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if on {
                        theme::accent()
                    } else {
                        ui::rgba_with(theme::accent(), 0.35)
                    })
                    .hover(|s| s.bg(theme::accent()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.show_featured(i, cx)
                    }))
                    .into_any_element()
            })
            .collect();
        let ready = matches!(state, Some(GameState::Ready | GameState::UpdateReady));

        let action = if ready {
            let s = slug.clone();
            button("hero-play", "Play")
                .primary()
                .large()
                .icon(Icon::Play)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.play(&s, cx)
                }))
        } else {
            let s = slug.clone();
            button("hero-view", "View game")
                .primary()
                .large()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.navigate(Screen::Game(s.clone()), cx)
                }))
        };

        div()
            .id("hero")
            .relative()
            .w_full()
            .h(px(height))
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::line())
            .bg(theme::surface())
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, _, cx| this.navigate(Screen::Game(slug.clone()), cx)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(height - 2.))
                    .child(ui::cover("hero-art", art, theme::RADIUS_CARD)),
            )
            .child(ui::scrim(90., 0.95).rounded(theme::RADIUS_CARD))
            .child(ui::scrim(0., 0.85).rounded(theme::RADIUS_CARD))
            .when(count > 1, |d| {
                d.child(
                    div()
                        .absolute()
                        .right(px(24.))
                        .bottom(px(24.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .children(dots),
                )
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(px(10.))
                    .max_w(px(520.))
                    .p(px(36.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().size(px(6.)).rounded_full().bg(theme::accent()))
                            .child(ui::eyebrow("Featured").text_color(theme::ink())),
                    )
                    .child(
                        div()
                            .text_size(px(40.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(px(46.))
                            .child(game.title.clone()),
                    )
                    .child(
                        div()
                            .text_base()
                            .text_color(theme::ink())
                            .opacity(0.86)
                            .child(game.tagline.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_color(theme::muted())
                            .child(format!("by {}", game.developer.display_name))
                            .child(div().size(px(3.)).rounded_full().bg(theme::faint()))
                            .child(platform_icons(&game.platforms, 14., theme::muted())),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .pt(px(8.))
                            .child(action)
                            .children(
                                state
                                    .as_ref()
                                    .and_then(|s| s.badge())
                                    .map(|(l, c)| ui::badge(l, c)),
                            )
                            .when(state.is_none(), |d| d.child(ui::muted("Free").text_xs())),
                    ),
            )
    }

    /// Installed games, most recently played first, for the Discover page.
    fn jump_back_in(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<Div> {
        let mut games: Vec<InstalledGame> = self
            .library
            .data
            .games
            .values()
            .filter(|g| g.is_installed())
            .cloned()
            .collect();
        if games.is_empty() {
            return None;
        }
        games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| b.added_at.cmp(&a.added_at))
        });
        let (cols, tile_w) = grid(window, 204.);
        let tiles: Vec<AnyElement> = games
            .into_iter()
            .take(cols)
            .map(|g| {
                let state = self.state_of(&g.slug);
                self.library_tile(&g, state, tile_w, cx).into_any_element()
            })
            .collect();
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(14.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(ui::section_title("Jump back in"))
                        .child(
                            button("see-library", "Library")
                                .ghost()
                                .icon(Icon::Library)
                                .on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.navigate(Screen::Library, cx)
                                    }),
                                ),
                        ),
                )
                .child(div().flex().gap(px(GAP)).children(tiles)),
        )
    }

    fn game_card(&mut self, game: &Game, width: f32, cx: &mut Context<Self>) -> Stateful<Div> {
        let image = foyer_install::library::capsule_url(game).and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let state = self.state_of(&game.slug);
        let badge = match &state {
            Some(GameState::Ready) => Some(("Installed".to_string(), theme::good())),
            Some(s) => s.badge(),
            None => None,
        };
        let fraction = state.as_ref().and_then(|s| s.fraction());
        let playable_here = game.platforms.contains(&self.platform);

        div()
            .id(SharedString::from(format!("card-{}", game.slug)))
            .group("card")
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(width))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, _, cx| this.navigate(Screen::Game(slug.clone()), cx)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px((width * CAPSULE_RATIO).floor()))
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::surface())
                    .group_hover("card", |s| s.border_color(theme::muted()))
                    .child(ui::cover(
                        SharedString::from(format!("card-img-{}", game.slug)),
                        image,
                        theme::RADIUS_CARD,
                    ))
                    .children(badge.map(|(label, color)| {
                        div()
                            .absolute()
                            .top(px(8.))
                            .left(px(8.))
                            .child(ui::overlay_badge(label, color))
                    }))
                    .children(fraction.map(|f| {
                        div()
                            .absolute()
                            .left(px(8.))
                            .right(px(8.))
                            .bottom(px(8.))
                            .child(ui::progress_bar(f, 4.))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .group_hover("card", |s| s.text_color(theme::ink()))
                                    .child(game.title.clone()),
                            )
                            .child(platform_icons(&game.platforms, 13., theme::faint())),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .line_clamp(2)
                            .child(game.tagline.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .text_xs()
                            .text_color(theme::faint())
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .child(game.developer.display_name.clone()),
                            )
                            .when(!playable_here, |d| {
                                d.child(
                                    div().flex_none().child(format!(
                                        "No {} build",
                                        platform_name(self.platform)
                                    )),
                                )
                            }),
                    ),
            )
    }

    // ---- game page --------------------------------------------------------

    fn game_page(&mut self, slug: &str, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (back_label, back_screen) = match &self.back {
            Screen::Library => ("Library", Screen::Library),
            Screen::Downloads => ("Downloads", Screen::Downloads),
            Screen::Settings => ("Settings", Screen::Settings),
            _ => ("Discover", Screen::Discover),
        };
        let back_target = back_screen.clone();
        let back = div().flex().child(
            button("back", back_label)
                .ghost()
                .icon(Icon::ArrowLeft)
                .on_click(
                    cx.listener(move |this, _, _, cx| this.navigate(back_target.clone(), cx)),
                ),
        );
        let page_error = self.pages.get(slug).and_then(|p| p.error.clone());
        let Some(game) = self.pages.get(slug).and_then(|p| p.game.clone()) else {
            let body = match page_error {
                Some(err) => {
                    empty_state(Icon::Alert, "Could not load this game", err).into_any_element()
                }
                None => div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(ui::skeleton(
                        "page-title-skel",
                        div().w(px(320.)).h(px(34.)).rounded(theme::RADIUS_CONTROL),
                    ))
                    .child(ui::skeleton(
                        "page-art-skel",
                        div().w_full().h(px(360.)).rounded(theme::RADIUS_CARD),
                    ))
                    .into_any_element(),
            };
            return div().flex().flex_col().gap_4().child(back).child(body);
        };

        let width = content_width(window);
        let two_cols = width >= 820.;
        let main_w = if two_cols {
            width - PANEL_W - 28.
        } else {
            width
        };

        let tags: Vec<AnyElement> = game
            .tags
            .iter()
            .map(|tag| {
                let t = tag.clone();
                div()
                    .id(SharedString::from(format!("tag-{tag}")))
                    .cursor_pointer()
                    .child(
                        chip(tag.clone(), false)
                            .hover(|s| s.bg(theme::line()).text_color(theme::ink())),
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.filter_tag(Some(t.clone()), cx)),
                    )
                    .into_any_element()
            })
            .collect();

        let art = hero_art(&game).and_then(|u| self.image(&u, cx));
        let logo = game
            .page
            .logo
            .and_then(|id| game.media.iter().find(|m| m.id == id))
            .map(|m| m.url.clone())
            .and_then(|u| self.image(&u, cx));
        let (title_text, title_weight) = match game.page.title_style {
            TitleStyle::Standard => (game.title.clone(), FontWeight::SEMIBOLD),
            TitleStyle::Uppercase => (game.title.to_uppercase(), FontWeight::BOLD),
        };
        let banner_h = (width * 0.3).clamp(240., 330.);
        let header =
            div()
                .relative()
                .w_full()
                .h(px(banner_h))
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::line())
                .bg(theme::surface())
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(px(banner_h - 2.))
                        .child(ui::cover("banner-art", art, theme::RADIUS_CARD)),
                )
                .child(ui::scrim(0., 0.97).rounded(theme::RADIUS_CARD))
                .child(ui::scrim(90., 0.6).rounded(theme::RADIUS_CARD))
                .child(
                    div().absolute().top(px(16.)).left(px(16.)).child(
                        div()
                            .id("back")
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .h(px(30.))
                            .pl(px(10.))
                            .pr(px(12.))
                            .rounded_full()
                            .bg(ui::rgba_with(theme::canvas(), 0.72))
                            .border_1()
                            .border_color(ui::rgba_with(theme::ink(), 0.12))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::canvas()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.navigate(back_screen.clone(), cx)
                            }))
                            .child(icon(Icon::ArrowLeft, 14., theme::ink()))
                            .child(back_label),
                    ),
                )
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .bottom_0()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .px(px(28.))
                        .pb(px(24.))
                        .child(match logo {
                            // The developer's logo instead of the title text.
                            Some(path) => div()
                                .h(px(96.))
                                .max_w(px(420.))
                                .child(img(path).h_full().object_fit(ObjectFit::Contain))
                                .into_any_element(),
                            None => div()
                                .text_size(px(40.))
                                .font_weight(title_weight)
                                .line_height(px(46.))
                                .child(title_text)
                                .into_any_element(),
                        })
                        .child(
                            div()
                                .text_base()
                                .text_color(theme::ink())
                                .opacity(0.86)
                                .child(game.tagline.clone()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .items_center()
                                .gap(px(8.))
                                .pt(px(4.))
                                .child(
                                    div()
                                        .text_color(theme::muted())
                                        .child(format!("by {}", game.developer.display_name)),
                                )
                                .child(platform_icons(&game.platforms, 14., theme::muted()))
                                .when(!tags.is_empty(), |d| {
                                    d.child(div().size(px(3.)).rounded_full().bg(theme::faint()))
                                })
                                .children(tags),
                        ),
                );

        // The developer's layout: the same blocks, in the same order, as on
        // the website.
        let accent = page_accent(&game.page);
        let mut blocks: Vec<AnyElement> = Vec::new();
        for (i, block) in game.page.layout().into_iter().enumerate() {
            blocks.push(match block {
                PageBlock::Screenshots => self.gallery(&game, main_w, cx).into_any_element(),
                PageBlock::About => div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(ui::section_title("About this game"))
                    .child(markdown(&game.description))
                    .into_any_element(),
                PageBlock::Text { heading, body } => div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(!heading.is_empty(), |d| d.child(ui::section_title(heading)))
                    .child(markdown(&body))
                    .into_any_element(),
                PageBlock::Features { heading, items } => div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .when(!heading.is_empty(), |d| d.child(ui::section_title(heading)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .max_w(px(720.))
                            .children(items.into_iter().map(|item| {
                                div()
                                    .flex()
                                    .items_start()
                                    .gap(px(10.))
                                    .line_height(px(22.))
                                    .child(
                                        div()
                                            .flex_none()
                                            .mt(px(9.))
                                            .size(px(5.))
                                            .rounded_full()
                                            .bg(accent.map_or(theme::accent(), |a| a.0)),
                                    )
                                    .child(div().text_color(theme::muted()).child(item))
                            })),
                    )
                    .into_any_element(),
                PageBlock::Quote { text, source } => div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .max_w(px(720.))
                    .pl(px(18.))
                    .border_l_4()
                    .border_color(accent.map_or(theme::accent(), |a| a.0))
                    .child(
                        div()
                            .font_family(theme::FONT_DISPLAY)
                            .text_size(px(22.))
                            .line_height(px(28.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(format!("“{text}”")),
                    )
                    .when(!source.is_empty(), |d| {
                        d.child(ui::muted(format!("— {source}")))
                    })
                    .into_any_element(),
                PageBlock::Image { media, caption } => {
                    let path = game
                        .media
                        .iter()
                        .find(|m| m.id == media)
                        .map(|m| m.url.clone())
                        .and_then(|u| self.image(&u, cx));
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(
                            div()
                                .w_full()
                                .h(px((main_w * 9. / 16.).floor()))
                                .child(ui::cover(
                                    SharedString::from(format!("block-img-{i}")),
                                    path,
                                    theme::RADIUS_CARD,
                                )),
                        )
                        .when(!caption.is_empty(), |d| d.child(ui::muted(caption)))
                        .into_any_element()
                }
                // Video can't play in the client; open it in the browser.
                PageBlock::Trailer { url } => match trailer_embed(&url) {
                    Some(_) => div()
                        .flex()
                        .child(
                            button(("trailer", i), "Watch the trailer")
                                .icon(Icon::Play)
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                        .into_any_element(),
                    None => div().into_any_element(),
                },
            });
        }
        let notes = notes_card(&game);
        let main = div()
            .flex()
            .flex_col()
            .gap(px(28.))
            .when(two_cols, |d| d.flex_1().min_w_0())
            .children(blocks)
            .children(notes);

        let side = div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(two_cols, |d| d.flex_none().w(px(PANEL_W)))
            .child(self.install_panel(&game, cx))
            .children(support_panel(&game))
            .child(self.details_panel(&game));

        div()
            .flex()
            .flex_col()
            .gap(px(28.))
            .child(header)
            .child(if two_cols {
                div()
                    .flex()
                    .items_start()
                    .gap(px(28.))
                    .child(main)
                    .child(side)
            } else {
                div().flex().flex_col().gap(px(24.)).child(side).child(main)
            })
    }

    fn gallery(&mut self, game: &Game, width: f32, cx: &mut Context<Self>) -> Div {
        let mut shots: Vec<_> = game
            .media
            .iter()
            .filter(|m| m.kind == MediaKind::Screenshot)
            .collect();
        shots.sort_by_key(|m| m.position);
        let mut urls: Vec<String> = shots.into_iter().map(|m| m.url.clone()).collect();
        if urls.is_empty()
            && let Some(url) = foyer_install::library::capsule_url(game)
        {
            urls.push(url);
        }
        let selected = self
            .pages
            .get(&game.slug)
            .map(|p| p.shot)
            .unwrap_or(0)
            .min(urls.len().saturating_sub(1));
        let images: Vec<_> = urls.iter().map(|u| self.image(u, cx)).collect();
        let main = images.get(selected).cloned().flatten();

        let thumbs: Vec<AnyElement> = images
            .iter()
            .enumerate()
            .map(|(i, image)| {
                let slug = game.slug.clone();
                let on = i == selected;
                div()
                    .id(("thumb", i))
                    .flex_none()
                    .w(px(104.))
                    .h(px(58.))
                    .p(px(2.))
                    .rounded(px(5.))
                    .border_2()
                    .border_color(if on {
                        theme::accent()
                    } else {
                        ui::rgba_with(theme::accent(), 0.)
                    })
                    .cursor_pointer()
                    .when(!on, |d| {
                        d.opacity(0.6)
                            .hover(|s| s.opacity(1.).border_color(theme::line()))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(page) = this.pages.get_mut(&slug) {
                            page.shot = i;
                        }
                        cx.notify();
                    }))
                    .child(ui::cover(("thumb-img", i), image.clone(), px(3.)))
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .w_full()
                    .h(px((width * 9. / 16.).floor()))
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::surface())
                    .child(ui::cover(
                        SharedString::from(format!("shot-{selected}")),
                        main,
                        theme::RADIUS_CARD,
                    )),
            )
            .when(thumbs.len() > 1, |d| {
                d.child(
                    div()
                        .id("thumbs")
                        .flex()
                        .gap(px(6.))
                        .overflow_x_scroll()
                        .children(thumbs),
                )
            })
    }

    /// Install, or the controls for a game already in the library.
    fn install_panel(&mut self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        if self.library.get(&game.slug).is_some() {
            return ui::card()
                .p(px(20.))
                .child(self.library_controls(&game.slug, cx))
                .into_any_element();
        }

        let platform = self.platform;
        let page = self.pages.get(&game.slug);
        let panel = ui::card().flex().flex_col().gap(px(14.)).p(px(20.));
        let Some(channels) = page.and_then(|p| p.channels.clone()) else {
            return panel
                .child(ui::skeleton(
                    "install-skel-a",
                    div().w(px(120.)).h(px(24.)).rounded(theme::RADIUS_CONTROL),
                ))
                .child(ui::skeleton(
                    "install-skel-b",
                    div().w_full().h(px(40.)).rounded(theme::RADIUS_CONTROL),
                ))
                .into_any_element();
        };
        let ours: Vec<_> = channels
            .iter()
            .filter(|c| c.platform == platform && c.latest.is_some())
            .cloned()
            .collect();
        if ours.is_empty() {
            let others: Vec<&str> = game
                .platforms
                .iter()
                .filter(|p| **p != platform)
                .map(|p| platform_name(*p))
                .collect();
            let listed_here = game.platforms.contains(&platform);
            let (title, text) = if listed_here || others.is_empty() {
                (
                    "No build ready yet".to_string(),
                    "The developer hasn't published a download for this computer yet.".to_string(),
                )
            } else {
                (
                    format!("Not available for {}", platform_name(platform)),
                    format!("Available for {}.", others.join(", ")),
                )
            };
            return panel
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon(Icon::Alert, 16., theme::warn()))
                        .child(div().font_weight(FontWeight::MEDIUM).child(title)),
                )
                .child(ui::muted(text))
                .into_any_element();
        }
        let selected = page
            .and_then(|p| p.channel.clone())
            .filter(|c| ours.iter().any(|o| &o.channel == c))
            .unwrap_or_else(|| ours[0].channel.clone());
        let channel = ours
            .iter()
            .find(|c| c.channel == selected)
            .cloned()
            .expect("selected channel exists");
        let build = channel.latest.clone().expect("filtered to ready builds");

        let picker: Option<Div> = (ours.len() > 1).then(|| {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(ui::eyebrow("Channel"))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap(px(6.))
                        .children(ours.iter().map(|c| {
                            let name = c.channel.clone();
                            let slug = game.slug.clone();
                            let chosen = name == selected;
                            div()
                                .id(SharedString::from(format!("channel-{name}")))
                                .cursor_pointer()
                                .child(chip(name.clone(), chosen))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(page) = this.pages.get_mut(&slug) {
                                        page.channel = Some(name.clone());
                                    }
                                    cx.notify();
                                }))
                        })),
                )
        });

        let game_owned = game.clone();
        panel
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Free"),
                    )
                    .child(
                        ui::mono(format!(
                            "{}{}",
                            build
                                .version
                                .as_deref()
                                .map(|v| format!("v{v} · "))
                                .unwrap_or_default(),
                            ui::size(build.total_size),
                        ))
                        .text_xs()
                        .text_color(theme::muted()),
                    ),
            )
            .children(picker)
            .child(
                button("install", "Install")
                    .primary()
                    .fill(page_accent(&game_owned.page))
                    .large()
                    .full_width()
                    .icon(Icon::Download)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.install(&game_owned, &channel, cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .min_w_0()
                    .text_color(theme::faint())
                    .child(icon(Icon::Folder, 13., theme::faint()))
                    .child(
                        ui::mono(pretty_path(&self.library.new_install_dir(&game.slug)))
                            .text_xs()
                            .min_w_0()
                            .truncate(),
                    ),
            )
            .into_any_element()
    }

    /// Play, update, progress and management actions for a library game.
    fn library_controls(&mut self, slug: &str, cx: &mut Context<Self>) -> Div {
        let Some(game) = self.library.get(slug).cloned() else {
            return div();
        };
        let id = |name: &str| SharedString::from(format!("{name}-{slug}"));
        let mut out = div().flex().flex_col().gap(px(12.));

        match self.state_of(slug) {
            Some(GameState::Working {
                kind,
                progress,
                stopping,
            }) => {
                out = out.child(job_progress(kind, &progress, stopping));
                // Swapping files in cannot be stopped; don't offer to.
                if progress.phase != Phase::Finishing {
                    let s = slug.to_string();
                    let c = slug.to_string();
                    out = out.child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                button(id("pause"), if stopping { "Pausing…" } else { "Pause" })
                                    .icon(Icon::Pause)
                                    .full_width()
                                    .disabled(stopping)
                                    .on_click(cx.listener(move |this, _, _, cx| this.pause(&s, cx)))
                                    .flex_1(),
                            )
                            .child(
                                button(id("cancel"), "Cancel").ghost().on_click(
                                    cx.listener(move |this, _, _, cx| this.cancel(&c, cx)),
                                ),
                            ),
                    );
                }
                return out;
            }
            Some(GameState::Paused { failed }) => {
                let what = if game.installed.is_some() {
                    "Update"
                } else {
                    "Download"
                };
                let size = game.pending.as_ref().map(|p| ui::size(p.size));
                out = out.child(match self.failures.get(slug) {
                    Some(err) if failed => div()
                        .flex()
                        .items_start()
                        .gap_2()
                        .child(icon(Icon::Alert, 16., theme::bad()))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(format!("{what} stopped")),
                                )
                                .child(ui::muted(err.clone()).text_xs()),
                        ),
                    _ => div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!("{what} paused")),
                        )
                        .children(size.map(|s| ui::mono(s).text_xs().text_color(theme::muted()))),
                });
                let s = slug.to_string();
                out = out.child(
                    button(id("resume"), if failed { "Try again" } else { "Resume" })
                        .primary()
                        .large()
                        .full_width()
                        .icon(if failed {
                            Icon::Refresh
                        } else {
                            Icon::Download
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let kind = if this.library.get(&s).is_some_and(|g| g.is_installed()) {
                                JobKind::Update
                            } else {
                                JobKind::Install
                            };
                            this.start_job(&s, kind, cx)
                        })),
                );
                let s = slug.to_string();
                out = out.child(
                    button(id("discard"), format!("Cancel {}", what.to_lowercase()))
                        .ghost()
                        .full_width()
                        .on_click(cx.listener(move |this, _, _, cx| this.cancel(&s, cx))),
                );
            }
            Some(state) => {
                if state == GameState::Running {
                    out = out.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .h(px(40.))
                            .rounded(theme::RADIUS_CONTROL)
                            .border_1()
                            .border_color(ui::rgba_with(theme::good(), 0.4))
                            .bg(ui::rgba_with(theme::good(), 0.08))
                            .text_color(theme::good())
                            .font_weight(FontWeight::MEDIUM)
                            .child(div().size(px(7.)).rounded_full().bg(theme::good()))
                            .child("Playing now"),
                    );
                } else {
                    let s = slug.to_string();
                    out = out.child(
                        button(id("play"), "Play")
                            .primary()
                            .large()
                            .full_width()
                            .icon(Icon::Play)
                            .on_click(cx.listener(move |this, _, _, cx| this.play(&s, cx))),
                    );
                }
                if let Some(update) = &game.update {
                    let s = slug.to_string();
                    let label = match &update.version {
                        Some(v) => format!("Update to v{v}"),
                        None => "Update".into(),
                    };
                    let label = match game.update_download {
                        Some(bytes) => format!("{label} · {}", ui::size(bytes)),
                        None => label,
                    };
                    out = out.child(
                        button(id("update"), label)
                            .full_width()
                            .icon(Icon::Download)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.start_job(&s, JobKind::Update, cx)
                            })),
                    );
                }
                if let Some(err) = self.failures.get(slug) {
                    out = out.child(
                        div()
                            .flex()
                            .items_start()
                            .gap_2()
                            .text_xs()
                            .text_color(theme::bad())
                            .child(icon(Icon::Alert, 14., theme::bad()))
                            .child(div().flex_1().min_w_0().child(err.clone())),
                    );
                }
            }
            None => {}
        }

        let action = |name: &str, label: &'static str, i: Icon, style: ButtonStyle| {
            button(id(name), label).style(style).icon(i).full_width()
        };
        let mut actions = div().flex().flex_col().gap(px(2.));
        if game.is_installed() {
            let s = slug.to_string();
            actions = actions.child(
                action("verify", "Verify files", Icon::Shield, ButtonStyle::Ghost)
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.start_job(&s, JobKind::Repair, cx)),
                    )
                    .justify_start(),
            );
            let s = slug.to_string();
            actions = actions.child(
                action(
                    "exe",
                    "Choose executable",
                    Icon::Terminal,
                    ButtonStyle::Ghost,
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.choose_executable(&s, window, cx)),
                )
                .justify_start(),
            );
        }
        let s = slug.to_string();
        actions = actions.child(
            action("folder", "Open folder", Icon::Folder, ButtonStyle::Ghost)
                .on_click(cx.listener(move |this, _, _, cx| this.open_folder(&s, cx)))
                .justify_start(),
        );
        let s = slug.to_string();
        actions = actions.child(
            action("uninstall", "Uninstall", Icon::Trash, ButtonStyle::Danger)
                .on_click(cx.listener(move |this, _, _, cx| this.uninstall(&s, cx)))
                .justify_start(),
        );

        out.child(div().h(px(1.)).bg(theme::line()).my(px(2.)))
            .child(actions)
    }

    fn details_panel(&self, game: &Game) -> Div {
        let installed = self.library.get(&game.slug);
        let mut rows: Vec<Div> = vec![
            ui::detail("Developer", game.developer.display_name.clone()),
            ui::detail(
                "Platforms",
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(platform_icons(&game.platforms, 13., theme::muted()))
                    .child(
                        game.platforms
                            .iter()
                            .map(|p| platform_name(*p))
                            .collect::<Vec<_>>()
                            .join(", "),
                    ),
            ),
        ];
        if let Some(at) = game.published_at {
            rows.push(ui::detail("Released", at.format("%-d %b %Y").to_string()));
        }
        if let Some(g) = installed {
            if let Some(b) = &g.installed {
                rows.push(ui::detail(
                    "Installed",
                    ui::mono(format!(
                        "{}{}",
                        b.version
                            .as_deref()
                            .map(|v| format!("v{v} · "))
                            .unwrap_or_default(),
                        ui::size(b.size)
                    ))
                    .text_xs(),
                ));
            }
            rows.push(ui::detail("Channel", g.channel.clone()));
            if let Some(exe) = &g.launch {
                rows.push(ui::detail("Starts", ui::mono(exe.clone()).text_xs()));
            }
            rows.push(ui::detail(
                "Last played",
                g.last_played
                    .map(ui::relative_day)
                    .unwrap_or_else(|| "never".into()),
            ));
            rows.push(ui::detail(
                "Folder",
                ui::mono(pretty_path(&g.install_dir)).text_xs(),
            ));
        }
        if let Some(last) = rows.pop() {
            rows.push(last.border_b_0());
        }
        ui::card()
            .flex()
            .flex_col()
            .px(px(20.))
            .py(px(8.))
            .children(rows)
    }

    // ---- library ----------------------------------------------------------

    fn library_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut games: Vec<InstalledGame> = self.library.data.games.values().cloned().collect();
        games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        let states: Vec<Option<GameState>> = games.iter().map(|g| self.state_of(&g.slug)).collect();
        let is_update = |s: &Option<GameState>| matches!(s, Some(GameState::UpdateReady));
        let is_downloading = |s: &Option<GameState>| {
            matches!(
                s,
                Some(GameState::Working { .. } | GameState::Paused { .. })
            )
        };
        let updates = states.iter().filter(|s| is_update(s)).count();
        let downloading = states.iter().filter(|s| is_downloading(s)).count();
        let filter = self.library_filter;
        let shown: Vec<(InstalledGame, Option<GameState>)> = games
            .into_iter()
            .zip(states)
            .filter(|(_, s)| match filter {
                LibraryFilter::All => true,
                LibraryFilter::Updates => is_update(s),
                LibraryFilter::Downloading => is_downloading(s),
            })
            .collect();
        let total = self.library.data.games.len();
        let (_, tile_w) = grid(window, 204.);

        let tabs: Vec<AnyElement> = [
            (LibraryFilter::All, "All", total),
            (LibraryFilter::Updates, "Updates", updates),
            (LibraryFilter::Downloading, "Downloading", downloading),
        ]
        .into_iter()
        .map(|(f, label, n)| {
            let active = f == filter;
            div()
                .id(SharedString::from(format!("filter-{label}")))
                .cursor_pointer()
                .child(chip(format!("{label}  {n}"), active).when(!active, |c| {
                    c.hover(|s| s.bg(theme::line()).text_color(theme::ink()))
                }))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.library_filter = f;
                    cx.notify();
                }))
                .into_any_element()
        })
        .collect();

        let spotlight = (filter == LibraryFilter::All)
            .then(|| shown.iter().find(|(g, _)| g.is_installed()).cloned())
            .flatten()
            .map(|(g, s)| self.library_spotlight(&g, s, window, cx));
        let tiles: Vec<AnyElement> = shown
            .into_iter()
            .map(|(g, s)| self.library_tile(&g, s, tile_w, cx).into_any_element())
            .collect();

        let checking = self.checking_updates;
        let body: AnyElement = if total == 0 {
            empty_state(
                Icon::Library,
                "No games installed",
                "Browse to add one. Everything on Foyer is free to download.",
            )
            .child(
                button("browse", "Discover games")
                    .primary()
                    .icon(Icon::Compass)
                    .on_click(cx.listener(|this, _, _, cx| this.navigate(Screen::Discover, cx))),
            )
            .into_any_element()
        } else if tiles.is_empty() {
            empty_state(
                Icon::Check,
                match filter {
                    LibraryFilter::Updates => "Everything is up to date",
                    _ => "Nothing is downloading",
                },
                "",
            )
            .into_any_element()
        } else {
            div()
                .flex()
                .flex_wrap()
                .gap(px(GAP))
                .children(tiles)
                .into_any_element()
        };

        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(ui::heading("Library"))
                            .child(ui::muted(match total {
                                0 => "Games you install show up here.".to_string(),
                                1 => "1 game on this computer.".to_string(),
                                n => format!("{n} games on this computer."),
                            })),
                    )
                    .when(total > 0, |d| {
                        d.child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    button(
                                        "check-updates",
                                        if checking {
                                            "Checking…"
                                        } else {
                                            "Check for updates"
                                        },
                                    )
                                    .ghost()
                                    .icon(Icon::Refresh)
                                    .disabled(checking)
                                    .on_click(cx.listener(|this, _, _, cx| this.check_updates(cx))),
                                )
                                .when(updates > 0, |d| {
                                    d.child(
                                        button("update-all", format!("Update all ({updates})"))
                                            .primary()
                                            .icon(Icon::Download)
                                            .on_click(
                                                cx.listener(|this, _, _, cx| this.update_all(cx)),
                                            ),
                                    )
                                }),
                        )
                    }),
            )
            .children(spotlight)
            .when(total > 0, |d| {
                d.child(div().flex().gap(px(6.)).children(tabs))
            })
            .child(body)
    }

    /// A large banner for the most recently played game.
    fn library_spotlight(
        &mut self,
        game: &InstalledGame,
        state: Option<GameState>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let image = game
            .capsule_url
            .clone()
            .and_then(|url| self.image(&url, cx));
        let height = (content_width(window) * 0.26).clamp(200., 260.);
        let slug = game.slug.clone();
        let eyebrow = if game.last_played.is_some() {
            "Continue playing"
        } else {
            "Ready to play"
        };
        let mut meta = Vec::new();
        if let Some(b) = &game.installed {
            if let Some(v) = &b.version {
                meta.push(format!("v{v}"));
            }
            meta.push(ui::size(b.size));
        }
        meta.push(match game.last_played {
            Some(at) => format!("played {}", ui::relative_day(at)),
            None => "not played yet".into(),
        });

        let s = slug.clone();
        let action = match &state {
            Some(GameState::Running) => div()
                .flex()
                .items_center()
                .gap_2()
                .h(px(40.))
                .px(px(16.))
                .rounded(theme::RADIUS_CONTROL)
                .bg(ui::rgba_with(theme::good(), 0.14))
                .text_color(theme::good())
                .font_weight(FontWeight::MEDIUM)
                .child(div().size(px(7.)).rounded_full().bg(theme::good()))
                .child("Playing now")
                .into_any_element(),
            Some(GameState::Ready | GameState::UpdateReady) => button("spot-play", "Play")
                .primary()
                .large()
                .icon(Icon::Play)
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.play(&s, cx)
                }))
                .into_any_element(),
            _ => div().into_any_element(),
        };
        let update = matches!(state, Some(GameState::UpdateReady)).then(|| {
            let s = slug.clone();
            button("spot-update", "Update")
                .icon(Icon::Download)
                .large()
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.start_job(&s, JobKind::Update, cx)
                }))
        });

        div()
            .id("spotlight")
            .relative()
            .w_full()
            .h(px(height))
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::line())
            .bg(theme::surface())
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, _, cx| this.navigate(Screen::Game(slug.clone()), cx)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w_full()
                    .h(px(height - 2.))
                    .child(ui::cover("spot-art", image, theme::RADIUS_CARD)),
            )
            .child(ui::scrim(90., 0.97).rounded(theme::RADIUS_CARD))
            .child(ui::scrim(0., 0.6).rounded(theme::RADIUS_CARD))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .bottom_0()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .gap(px(8.))
                    .p(px(28.))
                    .child(ui::eyebrow(eyebrow).text_color(theme::muted()))
                    .child(
                        div()
                            .text_size(px(32.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .line_height(px(38.))
                            .child(game.title.clone()),
                    )
                    .child(
                        ui::mono(meta.join(" · "))
                            .text_xs()
                            .text_color(theme::muted()),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .pt(px(8.))
                            .child(action)
                            .children(update),
                    ),
            )
    }

    fn library_tile(
        &mut self,
        game: &InstalledGame,
        state: Option<GameState>,
        width: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let image = game
            .capsule_url
            .clone()
            .and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let playable = matches!(state, Some(GameState::Ready | GameState::UpdateReady));
        let sub: SharedString = match &state {
            Some(GameState::Working {
                kind,
                progress,
                stopping,
            }) => phase_label(*kind, progress, *stopping).into(),
            Some(GameState::Paused { failed: true }) => "Download stopped".into(),
            Some(GameState::Paused { failed: false }) => "Paused".into(),
            Some(GameState::Running) => "Playing now".into(),
            Some(GameState::UpdateReady) => match game.update_download {
                Some(b) => format!("Update ready · {}", ui::size(b)).into(),
                None => "Update ready".into(),
            },
            _ => match game.last_played {
                Some(at) => format!("Played {}", ui::relative_day(at)).into(),
                None => "Not played yet".into(),
            },
        };
        let sub_color = match &state {
            Some(GameState::Running) => theme::good(),
            Some(GameState::UpdateReady) => theme::info(),
            Some(GameState::Paused { failed: true }) => theme::bad(),
            _ => theme::muted(),
        };

        let play_slug = slug.clone();
        div()
            .id(SharedString::from(format!("tile-{slug}")))
            .group("tile")
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(width))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, _, _, cx| this.navigate(Screen::Game(slug.clone()), cx)),
            )
            .child(
                div()
                    .relative()
                    .w_full()
                    .h(px((width * CAPSULE_RATIO).floor()))
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::surface())
                    .group_hover("tile", |s| s.border_color(theme::muted()))
                    .child(ui::cover(
                        SharedString::from(format!("tile-img-{}", game.slug)),
                        image,
                        theme::RADIUS_CARD,
                    ))
                    .when(playable, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(theme::RADIUS_CARD)
                                .bg(ui::rgba_with(theme::canvas(), 0.45))
                                .opacity(0.)
                                .group_hover("tile", |s| s.opacity(1.))
                                .child(
                                    div()
                                        .id(SharedString::from(format!("tile-play-{play_slug}")))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(48.))
                                        .rounded_full()
                                        .bg(theme::accent())
                                        .hover(|s| s.opacity(0.88))
                                        .child(icon(Icon::Play, 20., theme::accent_ink()))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.play(&play_slug, cx)
                                        })),
                                ),
                        )
                    })
                    .children(state.as_ref().and_then(|s| s.badge()).map(|(l, c)| {
                        div()
                            .absolute()
                            .top(px(8.))
                            .left(px(8.))
                            .child(ui::overlay_badge(l, c))
                    }))
                    .children(state.as_ref().and_then(|s| s.fraction()).map(|f| {
                        div()
                            .absolute()
                            .left(px(8.))
                            .right(px(8.))
                            .bottom(px(8.))
                            .child(ui::progress_bar(f, 4.))
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(3.))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(game.title.clone()),
                    )
                    .child(div().text_xs().truncate().text_color(sub_color).child(sub)),
            )
    }

    // ---- downloads --------------------------------------------------------

    fn downloads(&mut self, cx: &mut Context<Self>) -> Div {
        let mut games: Vec<InstalledGame> = self.library.data.games.values().cloned().collect();
        games.sort_by_key(|g| g.title.to_lowercase());
        let mut active = Vec::new();
        let mut paused = Vec::new();
        let mut updates = Vec::new();
        for game in games {
            match self.state_of(&game.slug) {
                Some(GameState::Working { .. }) => active.push(game),
                Some(GameState::Paused { .. }) => paused.push(game),
                Some(GameState::UpdateReady) => updates.push(game),
                _ => {}
            }
        }
        let nothing = active.is_empty() && paused.is_empty() && updates.is_empty();

        let active: Vec<AnyElement> = active
            .into_iter()
            .map(|g| self.download_card(&g, cx).into_any_element())
            .collect();
        let paused: Vec<AnyElement> = paused
            .into_iter()
            .map(|g| self.download_card(&g, cx).into_any_element())
            .collect();
        let n_updates = updates.len();
        let updates: Vec<AnyElement> = updates
            .into_iter()
            .map(|g| self.update_row(&g, cx).into_any_element())
            .collect();

        let section = |title: &'static str, count: usize| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(ui::section_title(title))
                .child(
                    ui::mono(count.to_string())
                        .text_xs()
                        .text_color(theme::faint()),
                )
        };

        div()
            .flex()
            .flex_col()
            .gap(px(24.))
            .max_w(px(900.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(ui::heading("Downloads"))
                    .child(ui::muted(
                        "Updates only fetch the parts of a game that changed.",
                    )),
            )
            .when(nothing, |d| {
                d.child(
                    empty_state(
                        Icon::Download,
                        "Nothing downloading",
                        "Installs and updates show up here while they run.",
                    )
                    .child(
                        button("dl-discover", "Discover games")
                            .icon(Icon::Compass)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.navigate(Screen::Discover, cx)),
                            ),
                    ),
                )
            })
            .when(!active.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(section("Downloading", active.len()))
                        .children(active),
                )
            })
            .when(!paused.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(section("Paused", paused.len()))
                        .children(paused),
                )
            })
            .when(!updates.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(section("Updates available", n_updates))
                                .child(
                                    button("dl-update-all", "Update all")
                                        .primary()
                                        .icon(Icon::Download)
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.update_all(cx)),
                                        ),
                                ),
                        )
                        .child(ui::card().flex().flex_col().children(updates)),
                )
            })
    }

    fn download_card(&mut self, game: &InstalledGame, cx: &mut Context<Self>) -> Div {
        let image = game
            .capsule_url
            .clone()
            .and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let id = |name: &str| SharedString::from(format!("dl-{name}-{slug}"));
        let state = self.state_of(&game.slug);

        let (body, controls): (Div, Div) = match state {
            Some(GameState::Working {
                kind,
                progress,
                stopping,
            }) => {
                let controls = if progress.phase == Phase::Finishing {
                    div()
                } else {
                    let s = slug.clone();
                    let c = slug.clone();
                    div()
                        .flex()
                        .gap(px(4.))
                        .child(
                            icon_button(id("pause"), Icon::Pause)
                                .disabled(stopping)
                                .on_click(cx.listener(move |this, _, _, cx| this.pause(&s, cx))),
                        )
                        .child(
                            icon_button(id("cancel"), Icon::X)
                                .style(ButtonStyle::Danger)
                                .on_click(cx.listener(move |this, _, _, cx| this.cancel(&c, cx))),
                        )
                };
                (job_progress(kind, &progress, stopping), controls)
            }
            Some(GameState::Paused { failed }) => {
                let what = if game.is_installed() {
                    "Update"
                } else {
                    "Download"
                };
                let detail: SharedString = match (failed, self.failures.get(&slug)) {
                    (true, Some(err)) => err.clone().into(),
                    _ => game
                        .pending
                        .as_ref()
                        .map(|p| format!("{} total", ui::size(p.size)))
                        .unwrap_or_default()
                        .into(),
                };
                let body = div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(if failed { theme::bad() } else { theme::warn() })
                            .when(failed, |d| d.child(icon(Icon::Alert, 14., theme::bad())))
                            .child(if failed {
                                format!("{what} stopped")
                            } else {
                                format!("{what} paused")
                            }),
                    )
                    .child(ui::muted(detail).text_xs().truncate());
                let s = slug.clone();
                let c = slug.clone();
                let controls = div()
                    .flex()
                    .gap(px(6.))
                    .child(
                        button(id("resume"), if failed { "Try again" } else { "Resume" })
                            .primary()
                            .icon(if failed {
                                Icon::Refresh
                            } else {
                                Icon::Download
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let kind = if this.library.get(&s).is_some_and(|g| g.is_installed())
                                {
                                    JobKind::Update
                                } else {
                                    JobKind::Install
                                };
                                this.start_job(&s, kind, cx)
                            })),
                    )
                    .child(
                        icon_button(id("discard"), Icon::X)
                            .style(ButtonStyle::Danger)
                            .on_click(cx.listener(move |this, _, _, cx| this.cancel(&c, cx))),
                    );
                (body, controls)
            }
            _ => (div(), div()),
        };

        let nav = slug.clone();
        ui::card()
            .flex()
            .items_center()
            .gap(px(18.))
            .p(px(14.))
            .child(
                div()
                    .id(id("cover"))
                    .flex_none()
                    .w(px(184.))
                    .h(px((184. * CAPSULE_RATIO).floor()))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(Screen::Game(nav.clone()), cx)
                    }))
                    .child(ui::cover(id("img"), image, theme::RADIUS_CONTROL)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_base()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(game.title.clone()),
                            )
                            .child(controls),
                    )
                    .child(body),
            )
    }

    fn update_row(&mut self, game: &InstalledGame, cx: &mut Context<Self>) -> Div {
        let image = game
            .capsule_url
            .clone()
            .and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let from = game
            .installed
            .as_ref()
            .and_then(|b| b.version.clone())
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "installed".into());
        let to = game
            .update
            .as_ref()
            .and_then(|b| b.version.clone())
            .map(|v| format!("v{v}"))
            .unwrap_or_else(|| "latest".into());
        let size = game
            .update_download
            .map(|b| format!(" · {} to download", ui::size(b)))
            .unwrap_or_default();
        div()
            .flex()
            .items_center()
            .gap(px(14.))
            .px(px(14.))
            .py(px(10.))
            .border_b_1()
            .border_color(theme::line())
            .child(
                div()
                    .flex_none()
                    .w(px(96.))
                    .h(px((96. * CAPSULE_RATIO).floor()))
                    .child(ui::cover(
                        SharedString::from(format!("upd-img-{slug}")),
                        image,
                        px(3.),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .truncate()
                            .font_weight(FontWeight::MEDIUM)
                            .child(game.title.clone()),
                    )
                    .child(
                        ui::mono(format!("{from} → {to}{size}"))
                            .text_xs()
                            .text_color(theme::muted()),
                    ),
            )
            .child(
                button(SharedString::from(format!("upd-{slug}")), "Update")
                    .icon(Icon::Download)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.start_job(&slug, JobKind::Update, cx)
                    })),
            )
    }

    // ---- settings ---------------------------------------------------------

    fn settings(&mut self, cx: &mut Context<Self>) -> Div {
        let settings = self.library.data.settings.clone();

        div()
            .flex()
            .flex_col()
            .gap(px(20.))
            .max_w(px(720.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(ui::heading("Settings"))
                    .child(ui::muted("Stored on this computer. There is no account.")),
            )
            .child(
                settings_section(
                    "Install folder",
                    "New games go into their own folder here. Games already installed stay where they are.",
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .flex_1()
                                .min_w_0()
                                .h(px(32.))
                                .px(px(10.))
                                .rounded(theme::RADIUS_CONTROL)
                                .border_1()
                                .border_color(theme::line())
                                .bg(theme::canvas())
                                .child(icon(Icon::Folder, 14., theme::faint()))
                                .child(
                                    ui::mono(pretty_path(&settings.install_root))
                                        .text_xs()
                                        .min_w_0()
                                        .truncate(),
                                ),
                        )
                        .child(
                            button("change-root", "Change…").on_click(cx.listener(
                                |this, _, window, cx| this.choose_install_root(window, cx),
                            )),
                        ),
                ),
            )
            .child(
                settings_section("Updates", "Updates download only the parts of a game that changed.")
                    .child(toggle_row(
                        "toggle-check",
                        "Check for updates",
                        "At startup and every 30 minutes.",
                        settings.check_updates,
                        cx,
                        Foyer::toggle_check_updates,
                    ))
                    .child(div().h(px(1.)).bg(theme::line()))
                    .child(toggle_row(
                        "toggle-auto",
                        "Install updates automatically",
                        "Updates start as soon as they are found.",
                        settings.auto_update,
                        cx,
                        Foyer::toggle_auto_update,
                    )),
            )
            .child(
                settings_section("About", "")
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .child(ui::detail(
                                "Version",
                                ui::mono(env!("CARGO_PKG_VERSION")).text_xs(),
                            ))
                            .child(ui::detail("Platform", platform_name(current_platform())))
                            .child(ui::detail("Account", "None needed"))
                            .child(ui::detail(
                                "Server",
                                ui::mono(self.api.base_url().to_string()).text_xs(),
                            ))
                            .child(
                                ui::detail(
                                    "Data folder",
                                    ui::mono(pretty_path(&self.data_dir)).text_xs(),
                                )
                                .border_b_0(),
                            ),
                    ),
            )
    }
}

// ---- pieces ----------------------------------------------------------------

fn nav_item(
    id: &'static str,
    i: Icon,
    label: &'static str,
    screen: Screen,
    selected: bool,
    count: Option<(usize, Rgba)>,
    cx: &mut Context<Foyer>,
) -> Stateful<Div> {
    let color = if selected {
        theme::ink()
    } else {
        theme::muted()
    };
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .px(px(10.))
        .rounded(theme::RADIUS_CONTROL)
        .cursor_pointer()
        .text_color(color)
        .relative()
        .when(selected, |d| {
            d.bg(theme::raised()).font_weight(FontWeight::MEDIUM).child(
                div()
                    .absolute()
                    .left(px(-12.))
                    .top(px(9.))
                    .bottom(px(9.))
                    .w(px(3.))
                    .rounded_r(px(3.))
                    .bg(theme::accent()),
            )
        })
        .hover(|s| s.bg(theme::raised()).text_color(theme::ink()))
        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| this.navigate(screen.clone(), cx)))
        .child(icon(i, 16., color))
        .child(div().flex_1().child(label))
        .children(count.map(|(n, c)| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .min_w(px(20.))
                .h(px(18.))
                .px(px(5.))
                .rounded_full()
                .bg(ui::rgba_with(c, 0.18))
                .text_color(c)
                .font_family(theme::FONT_MONO)
                .text_xs()
                .child(n.to_string())
        }))
}

fn phase_label(kind: JobKind, p: &Progress, stopping: bool) -> String {
    if stopping {
        return "Pausing…".into();
    }
    match p.phase {
        Phase::Preparing if kind == JobKind::Repair => "Checking files".into(),
        Phase::Preparing => "Preparing".into(),
        Phase::Downloading => match kind {
            JobKind::Install => "Downloading".into(),
            JobKind::Update => "Downloading update".into(),
            JobKind::Repair => "Downloading repairs".into(),
        },
        Phase::Installing => "Installing files".into(),
        Phase::Finishing => "Finishing".into(),
    }
}

/// Label, percentage, bar and transfer details for a running job.
fn job_progress(kind: JobKind, p: &Progress, stopping: bool) -> Div {
    let mut detail = Vec::new();
    if p.total > 0 {
        detail.push(format!("{} of {}", ui::size(p.done), ui::size(p.total)));
    }
    if p.phase == Phase::Downloading && p.bytes_per_sec > 0.0 {
        detail.push(format!("{}/s", ui::size(p.bytes_per_sec as u64)));
        if let Some(eta) = ui::eta(p.total.saturating_sub(p.done), p.bytes_per_sec) {
            detail.push(eta);
        }
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .child(phase_label(kind, p, stopping)),
                )
                .child(
                    ui::mono(format!("{}%", (p.fraction() * 100.) as u32))
                        .text_xs()
                        .text_color(theme::muted()),
                ),
        )
        .child(ui::progress_bar(p.fraction(), 6.))
        .when(!detail.is_empty(), |d| {
            d.child(
                ui::mono(detail.join(" · "))
                    .text_xs()
                    .text_color(theme::faint()),
            )
        })
}

fn skeleton_card(i: usize, width: f32) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .w(px(width))
        .child(ui::skeleton(
            ("skel-art", i),
            div()
                .w_full()
                .h(px((width * CAPSULE_RATIO).floor()))
                .rounded(theme::RADIUS_CARD),
        ))
        .child(ui::skeleton(
            ("skel-title", i),
            div().w(px(width * 0.6)).h(px(14.)).rounded(px(3.)),
        ))
        .child(ui::skeleton(
            ("skel-line", i),
            div().w(px(width * 0.85)).h(px(10.)).rounded(px(3.)),
        ))
        .into_any_element()
}

fn support_panel(game: &Game) -> Option<Div> {
    if game.support_links.is_empty() {
        return None;
    }
    let buttons = game.support_links.iter().enumerate().map(|(i, link)| {
        let url = link.url.clone();
        let label = link
            .label
            .clone()
            .unwrap_or_else(|| support_label(link.kind).to_string());
        button(("support", i), label)
            .style(ButtonStyle::Support)
            .icon(Icon::External)
            .full_width()
            .on_click(move |_, _, cx| cx.open_url(&url))
    });
    Some(
        ui::card()
            .flex()
            .flex_col()
            .gap(px(12.))
            .p(px(20.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icon(Icon::Heart, 16., theme::support()))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Support the developer"),
                    ),
            )
            .child(
                ui::muted(format!(
                    "Foyer games are free. If you enjoy {}, you can support {} directly.",
                    game.title, game.developer.display_name
                ))
                .text_xs()
                .line_height(px(18.)),
            )
            .child(div().flex().flex_col().gap(px(8.)).children(buttons)),
    )
}

fn notes_card(game: &Game) -> Option<Div> {
    let notes: Vec<&str> = game
        .content_descriptors
        .iter()
        .map(|d| descriptor_label(*d))
        .collect();
    let ai = game.ai_disclosure.clone().filter(|s| !s.trim().is_empty());
    if notes.is_empty() && ai.is_none() {
        return None;
    }
    let row = |title: &'static str, text: String| {
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(ui::eyebrow(title))
            .child(div().text_color(theme::muted()).child(text))
    };
    Some(
        ui::card()
            .flex()
            .gap(px(14.))
            .p(px(18.))
            .child(icon(Icon::Shield, 18., theme::muted()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .flex_1()
                    .min_w_0()
                    .when(!notes.is_empty(), |d| {
                        d.child(row("Content notes", capitalise(&notes.join(", "))))
                    })
                    .when_some(ai, |d, text| d.child(row("Use of generative AI", text))),
            ),
    )
}

fn settings_section(title: &'static str, description: &'static str) -> Div {
    ui::card().flex().flex_col().gap(px(14.)).p(px(20.)).child(
        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(ui::section_title(title))
            .when(!description.is_empty(), |d| {
                d.child(ui::muted(description).text_xs())
            }),
    )
}

fn toggle_row(
    id: &'static str,
    title: &'static str,
    description: &'static str,
    on: bool,
    cx: &mut Context<Foyer>,
    f: fn(&mut Foyer, &mut Context<Foyer>),
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::MEDIUM).child(title))
                .child(ui::muted(description).text_xs()),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .w(px(36.))
                .h(px(20.))
                .px(px(3.))
                .rounded_full()
                .when(on, |d| d.justify_end().bg(theme::accent()))
                .when(!on, |d| {
                    d.justify_start()
                        .bg(theme::raised())
                        .border_1()
                        .border_color(theme::line())
                })
                .child(div().size(px(14.)).rounded_full().bg(if on {
                    theme::accent_ink()
                } else {
                    theme::muted()
                })),
        )
}

fn empty_state(i: Icon, title: impl Into<SharedString>, body: impl Into<SharedString>) -> Div {
    let body: SharedString = body.into();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(10.))
        .py(px(56.))
        .px(px(24.))
        .rounded(theme::RADIUS_CARD)
        .border_1()
        .border_color(theme::line())
        .child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .size(px(44.))
                .rounded_full()
                .bg(theme::raised())
                .child(icon(i, 20., theme::muted())),
        )
        .child(
            div()
                .pt(px(4.))
                .text_base()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .when(!body.is_empty(), |d| {
            d.child(div().max_w(px(420.)).text_color(theme::muted()).child(body))
        })
        .child(div().h(px(4.)))
}

fn platform_icons(platforms: &[Platform], size: f32, color: Rgba) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(6.))
        .children(platforms.iter().map(|p| {
            icon(
                match p {
                    Platform::Windows => Icon::Windows,
                    Platform::Macos => Icon::Apple,
                    Platform::Linux => Icon::Linux,
                    Platform::Web => Icon::Compass,
                },
                size,
                color,
            )
        }))
}

/// The widest image of a game, for banners: the first screenshot, else the
/// capsule.
/// The page's own accent and the text colour for it, if the developer set
/// one. Only the accent is used in the client: the window keeps Foyer's
/// colours, so the button text is matched to the accent alone.
fn page_accent(page: &PageStyle) -> Option<(Rgba, Rgba)> {
    page.accent.as_ref()?;
    let palette = Palette::for_style(&PageStyle {
        accent: page.accent.clone(),
        ..Default::default()
    });
    Some((
        rgb(palette.accent.to_u32()),
        rgb(palette.accent_ink.to_u32()),
    ))
}

/// The banner: the developer's hero image, else the first screenshot, else
/// the capsule.
fn hero_art(game: &Game) -> Option<String> {
    let hero = game
        .page
        .hero
        .and_then(|id| game.media.iter().find(|m| m.id == id));
    if let Some(hero) = hero {
        return Some(hero.url.clone());
    }
    game.media
        .iter()
        .filter(|m| m.kind == MediaKind::Screenshot)
        .min_by_key(|m| m.position)
        .map(|m| m.url.clone())
        .or_else(|| foyer_install::library::capsule_url(game))
}

pub fn platform_name(p: Platform) -> &'static str {
    match p {
        Platform::Windows => "Windows",
        Platform::Macos => "macOS",
        Platform::Linux => "Linux",
        Platform::Web => "Web",
    }
}

fn support_label(kind: SupportKind) -> &'static str {
    match kind {
        SupportKind::Patreon => "Patreon",
        SupportKind::Kofi => "Ko-fi",
        SupportKind::GithubSponsors => "GitHub Sponsors",
        SupportKind::Itchio => "itch.io",
        SupportKind::Website => "Website",
        SupportKind::Other => "Support",
    }
}

fn descriptor_label(d: ContentDescriptor) -> &'static str {
    match d {
        ContentDescriptor::Violence => "violence",
        ContentDescriptor::Blood => "blood",
        ContentDescriptor::Horror => "horror",
        ContentDescriptor::StrongLanguage => "strong language",
        ContentDescriptor::DrugsAlcohol => "drugs and alcohol",
        ContentDescriptor::MatureThemes => "mature themes",
    }
}

/// A path for display, with the home folder shortened to `~`.
fn pretty_path(path: &std::path::Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home.and_then(|h| path.strip_prefix(h).ok().map(|p| p.to_path_buf())) {
        Some(rest) if !rest.as_os_str().is_empty() => {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        }
        _ => path.display().to_string(),
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// A plain rendering of a game's Markdown description: headings, bullet
/// lists and paragraphs, with inline markers removed.
fn markdown(source: &str) -> Div {
    let mut out = div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .max_w(px(720.))
        .line_height(px(22.));
    for block in source.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        if let Some(heading) = block.strip_prefix('#') {
            out = out.child(
                div()
                    .pt(px(6.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(strip_inline(heading.trim_start_matches('#').trim())),
            );
            continue;
        }
        let lines: Vec<&str> = block.lines().collect();
        if lines
            .iter()
            .all(|l| l.trim_start().starts_with("- ") || l.trim_start().starts_with("* "))
        {
            out = out.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(lines.iter().map(|l| {
                        div()
                            .flex()
                            .items_start()
                            .gap(px(10.))
                            .child(
                                div()
                                    .flex_none()
                                    .mt(px(9.))
                                    .size(px(4.))
                                    .rounded_full()
                                    .bg(theme::faint()),
                            )
                            .child(
                                div()
                                    .text_color(theme::muted())
                                    .child(strip_inline(&l.trim_start()[2..])),
                            )
                    })),
            );
        } else {
            out = out.child(
                div()
                    .text_color(theme::muted())
                    .child(strip_inline(&lines.join(" "))),
            );
        }
    }
    out
}

fn strip_inline(text: &str) -> String {
    text.replace("**", "").replace("__", "").replace('`', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_inline_markdown() {
        assert_eq!(strip_inline("**bold** and `code`"), "bold and code");
    }

    #[test]
    fn capitalises() {
        assert_eq!(capitalise("violence, blood"), "Violence, blood");
    }
}

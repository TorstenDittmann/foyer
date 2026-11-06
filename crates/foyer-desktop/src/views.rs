//! The screens: discover, game page, library, downloads and settings.

use foyer_install::launch::current_platform;
use foyer_install::{InstalledGame, Phase};
use foyer_types::{ContentDescriptor, Game, MediaKind, Platform, SupportKind};
use gpui::{
    AnyElement, ClickEvent, Context, Div, FontWeight, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use crate::app::{Foyer, JobKind, Screen};
use crate::theme;
use crate::ui::{self, ButtonStyle, button, disabled_button, pill};

impl Render for Foyer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content: AnyElement = match self.screen.clone() {
            Screen::Discover => self.discover(window, cx).into_any_element(),
            Screen::Game(slug) => self.game_page(&slug, cx).into_any_element(),
            Screen::Library => self.library_screen(cx).into_any_element(),
            Screen::Downloads => self.downloads(cx).into_any_element(),
            Screen::Settings => self.settings(cx).into_any_element(),
        };

        div()
            .flex()
            .size_full()
            .bg(theme::canvas())
            .text_color(theme::ink())
            .font_family(theme::FONT_SANS)
            .text_sm()
            .child(self.sidebar(cx))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .children(self.notice_bar(cx))
                    .child(
                        div()
                            .id("content")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .p_6()
                            .child(content),
                    ),
            )
    }
}

impl Foyer {
    fn sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let active = |screen: &Screen| match (&self.screen, screen) {
            (Screen::Game(_), Screen::Discover) => true,
            (a, b) => a == b,
        };
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

        let item =
            |id: &'static str,
             label: &'static str,
             screen: Screen,
             badge: usize,
             cx: &mut Context<Self>| {
                let selected = active(&screen);
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_3()
                    .py(px(7.))
                    .rounded(theme::RADIUS_CONTROL)
                    .cursor_pointer()
                    .text_color(if selected {
                        theme::ink()
                    } else {
                        theme::muted()
                    })
                    .when(selected, |d| d.bg(theme::raised()))
                    .hover(|s| s.bg(theme::raised()).text_color(theme::ink()))
                    .child(label)
                    .when(badge > 0, |d| {
                        d.child(
                            div()
                                .font_family(theme::FONT_MONO)
                                .text_xs()
                                .text_color(theme::muted())
                                .child(badge.to_string()),
                        )
                    })
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.navigate(screen.clone(), cx)
                    }))
            };

        div()
            .flex()
            .flex_col()
            .flex_none()
            .w(px(200.))
            .h_full()
            .p_3()
            .gap_1()
            .bg(theme::surface())
            .border_r_1()
            .border_color(theme::line())
            .child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_4()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Foyer"),
            )
            .child(item("nav-discover", "Discover", Screen::Discover, 0, cx))
            .child(item("nav-library", "Library", Screen::Library, updates, cx))
            .child(item(
                "nav-downloads",
                "Downloads",
                Screen::Downloads,
                downloads,
                cx,
            ))
            .child(div().flex_1())
            .child(item("nav-settings", "Settings", Screen::Settings, 0, cx))
    }

    fn notice_bar(&mut self, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        let notice = self.notice.as_ref()?;
        Some(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .px_6()
                .py_2()
                .border_b_1()
                .border_color(theme::line())
                .bg(theme::surface())
                .child(
                    div()
                        .text_color(if notice.error {
                            theme::bad()
                        } else {
                            theme::good()
                        })
                        .child(notice.text.clone()),
                )
                .child(button(
                    "dismiss-notice",
                    "Dismiss",
                    ButtonStyle::Ghost,
                    cx.listener(|this, _, _, cx| {
                        this.notice = None;
                        cx.notify();
                    }),
                )),
        )
    }

    // ---- discover ---------------------------------------------------------

    fn search_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focused = self.search_focus.is_focused(window);
        let empty = self.catalog.query.is_empty();
        div()
            .id("search")
            .track_focus(&self.search_focus)
            .flex()
            .items_center()
            .w(px(320.))
            .px_3()
            .py(px(7.))
            .rounded(theme::RADIUS_CONTROL)
            .border_1()
            .border_color(if focused {
                theme::accent()
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
            .child(
                div()
                    .flex_1()
                    .text_color(if empty { theme::faint() } else { theme::ink() })
                    .child(if empty && !focused {
                        SharedString::from("Search games")
                    } else {
                        SharedString::from(self.catalog.query.clone())
                    }),
            )
            .when(focused, |d| {
                d.child(div().w(px(1.)).h(px(14.)).bg(theme::ink()))
            })
    }

    fn discover(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let games: Vec<Game> = self.catalog.games.clone();
        let cards: Vec<AnyElement> = games
            .iter()
            .map(|game| self.game_card(game, cx).into_any_element())
            .collect();

        let status: Option<Div> = if let Some(err) = &self.catalog.error {
            Some(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_color(theme::bad()).child(format!(
                        "Could not load games from {}: {err}",
                        self.api.base_url()
                    )))
                    .child(button(
                        "retry",
                        "Try again",
                        ButtonStyle::Secondary,
                        cx.listener(|this, _, _, cx| this.load_catalog_page(cx)),
                    )),
            )
        } else if self.catalog.loading && games.is_empty() {
            Some(ui::muted("Loading games…"))
        } else if games.is_empty() {
            Some(ui::muted("No games match. Try another search."))
        } else {
            None
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(ui::heading("Discover"))
                    .child(self.search_box(window, cx)),
            )
            .when_some(self.catalog.tag.clone(), |d, tag| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(ui::muted("Tagged"))
                        .child(pill(tag))
                        .child(button(
                            "clear-tag",
                            "Clear",
                            ButtonStyle::Ghost,
                            cx.listener(|this, _, _, cx| this.filter_tag(None, cx)),
                        )),
                )
            })
            .children(status)
            .child(div().flex().flex_wrap().gap_3().children(cards))
            .when(self.catalog.next_cursor.is_some(), |d| {
                d.child(
                    div()
                        .flex()
                        .justify_center()
                        .child(if self.catalog.loading {
                            ui::into_any(disabled_button("Loading…"))
                        } else {
                            ui::into_any(button(
                                "more",
                                "Load more",
                                ButtonStyle::Secondary,
                                cx.listener(|this, _, _, cx| this.load_catalog_page(cx)),
                            ))
                        }),
                )
            })
    }

    fn game_card(&mut self, game: &Game, cx: &mut Context<Self>) -> impl IntoElement {
        let image = foyer_install::library::capsule_url(game).and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let installed = self
            .library
            .get(&game.slug)
            .is_some_and(|g| g.is_installed());
        let playable_here = game.platforms.contains(&self.platform);
        div()
            .id(SharedString::from(format!("card-{}", game.slug)))
            .flex()
            .flex_col()
            .w(px(220.))
            .p_2()
            .gap_2()
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::line())
            .bg(theme::surface())
            .cursor_pointer()
            .hover(|s| s.bg(theme::raised()))
            .on_click(
                cx.listener(move |this, _, _, cx| this.navigate(Screen::Game(slug.clone()), cx)),
            )
            .child(div().h(px(103.)).child(ui::cover(image)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .px_1()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(game.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme::muted())
                            .truncate()
                            .child(game.tagline.clone()),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_xs()
                            .text_color(theme::faint())
                            .child(div().truncate().child(game.developer.display_name.clone()))
                            .child(if installed {
                                div().text_color(theme::good()).child("Installed")
                            } else if !playable_here {
                                div().child(format!("No {} build", platform_name(self.platform)))
                            } else {
                                div()
                            }),
                    ),
            )
    }

    // ---- game page --------------------------------------------------------

    fn game_page(&mut self, slug: &str, cx: &mut Context<Self>) -> impl IntoElement {
        let back = div().flex().child(button(
            "back",
            "← Discover",
            ButtonStyle::Ghost,
            cx.listener(|this, _, _, cx| this.navigate(Screen::Discover, cx)),
        ));
        let page_error = self.pages.get(slug).and_then(|p| p.error.clone());
        let Some(game) = self.pages.get(slug).and_then(|p| p.game.clone()) else {
            return div()
                .flex()
                .flex_col()
                .gap_4()
                .child(back)
                .child(match page_error {
                    Some(err) => div().text_color(theme::bad()).child(err),
                    None => ui::muted("Loading…"),
                });
        };

        let capsule = foyer_install::library::capsule_url(&game).and_then(|u| self.image(&u, cx));
        let mut shots: Vec<_> = game
            .media
            .iter()
            .filter(|m| m.kind == MediaKind::Screenshot)
            .collect();
        shots.sort_by_key(|m| m.position);
        let shots: Vec<AnyElement> = shots
            .into_iter()
            .map(|m| m.url.clone())
            .collect::<Vec<_>>()
            .into_iter()
            .map(|url| {
                let image = self.image(&url, cx);
                div()
                    .flex_none()
                    .w(px(320.))
                    .h(px(180.))
                    .child(ui::cover(image))
                    .into_any_element()
            })
            .collect();

        let tags: Vec<AnyElement> = game
            .tags
            .iter()
            .map(|tag| {
                let t = tag.clone();
                div()
                    .id(SharedString::from(format!("tag-{tag}")))
                    .cursor_pointer()
                    .child(pill(tag.clone()).hover(|s| s.text_color(theme::ink())))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.filter_tag(Some(t.clone()), cx)),
                    )
                    .into_any_element()
            })
            .collect();

        let support: Vec<AnyElement> = game
            .support_links
            .iter()
            .enumerate()
            .map(|(i, link)| {
                let url = link.url.clone();
                let label = link
                    .label
                    .clone()
                    .unwrap_or_else(|| support_label(link.kind).to_string());
                button(
                    ("support", i),
                    label,
                    ButtonStyle::Support,
                    move |_, _, cx| cx.open_url(&url),
                )
                .into_any_element()
            })
            .collect();

        let notes: Vec<String> = game
            .content_descriptors
            .iter()
            .map(|d| descriptor_label(*d).to_string())
            .collect();

        div()
            .flex()
            .flex_col()
            .gap_5()
            .max_w(px(980.))
            .child(back)
            .child(
                div()
                    .flex()
                    .gap_5()
                    .child(
                        div()
                            .flex_none()
                            .w(px(460.))
                            .h(px(215.))
                            .child(ui::cover(capsule)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(ui::heading(game.title.clone()))
                            .child(ui::muted(format!("by {}", game.developer.display_name)))
                            .child(div().child(game.tagline.clone()))
                            .child(
                                div().flex().gap_2().children(
                                    game.platforms.iter().map(|p| pill(platform_name(*p))),
                                ),
                            )
                            .child(div().flex_1())
                            .child(self.game_actions(&game, cx)),
                    ),
            )
            .when(!support.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(ui::muted(
                            "Foyer games are free. If you enjoy this one, support the developer:",
                        ))
                        .child(div().flex().flex_wrap().gap_2().children(support)),
                )
            })
            .when(!shots.is_empty(), |d| {
                d.child(
                    div()
                        .id("shots")
                        .flex()
                        .gap_3()
                        .overflow_x_scroll()
                        .children(shots),
                )
            })
            .when(!tags.is_empty(), |d| {
                d.child(div().flex().flex_wrap().gap_2().children(tags))
            })
            .child(markdown(&game.description))
            .when(!notes.is_empty(), |d| {
                d.child(ui::muted(format!("Content notes: {}", notes.join(", "))))
            })
            .when_some(
                game.ai_disclosure.clone().filter(|s| !s.is_empty()),
                |d, text| d.child(ui::muted(format!("Use of generative AI: {text}"))),
            )
    }

    /// The install / play / update controls for a game page.
    fn game_actions(&mut self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        if self.library.get(&game.slug).is_some() {
            return self.library_actions(&game.slug, true, cx);
        }

        let platform = self.platform;
        let page = self.pages.get(&game.slug);
        let Some(channels) = page.and_then(|p| p.channels.clone()) else {
            return ui::into_any(disabled_button("Checking builds…"));
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
            let text = if others.is_empty() {
                "No downloadable build yet.".to_string()
            } else {
                format!(
                    "Not available for {} yet. Available for {}.",
                    platform_name(platform),
                    others.join(", ")
                )
            };
            return ui::into_any(ui::muted(text));
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
                .items_center()
                .gap_2()
                .child(ui::muted("Channel"))
                .children(ours.iter().map(|c| {
                    let name = c.channel.clone();
                    let slug = game.slug.clone();
                    let chosen = name == selected;
                    div()
                        .id(SharedString::from(format!("channel-{name}")))
                        .px_2()
                        .py(px(3.))
                        .rounded(theme::RADIUS_CONTROL)
                        .border_1()
                        .border_color(if chosen {
                            theme::accent()
                        } else {
                            theme::line()
                        })
                        .text_xs()
                        .cursor_pointer()
                        .child(name.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(page) = this.pages.get_mut(&slug) {
                                page.channel = Some(name.clone());
                            }
                            cx.notify();
                        }))
                }))
        });

        let game_owned = game.clone();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(picker)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(button(
                        "install",
                        "Install",
                        ButtonStyle::Primary,
                        cx.listener(move |this, _, _, cx| {
                            this.install(&game_owned, &channel, cx);
                            this.navigate(Screen::Downloads, cx);
                        }),
                    ))
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
            .child(
                ui::mono(format!(
                    "Installs to {}",
                    self.library.new_install_dir(&game.slug).display()
                ))
                .text_xs()
                .text_color(theme::faint())
                .truncate(),
            )
            .into_any_element()
    }

    /// Controls for a game in the library. `full` shows every action
    /// inline (game page); otherwise extras sit behind "More".
    fn library_actions(&mut self, slug: &str, full: bool, cx: &mut Context<Self>) -> AnyElement {
        let Some(game) = self.library.get(slug).cloned() else {
            return div().into_any_element();
        };
        let slug_s = slug.to_string();
        let id = |name: &str| SharedString::from(format!("{name}-{slug}"));
        let mut row = div().flex().flex_wrap().items_center().gap_2();
        let mut status: Option<Div> = None;

        if let Some(job) = self.jobs.get(slug) {
            let p = job.handle.progress();
            let stopping = job.handle.is_stopped();
            status = Some(job_status(job.kind, &p, stopping));
            // Swapping files in cannot be stopped; don't offer to.
            if p.phase == Phase::Finishing {
                return div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .children(status)
                    .into_any_element();
            }
            let s = slug_s.clone();
            row = row.child(if stopping {
                ui::into_any(disabled_button("Pausing…"))
            } else {
                ui::into_any(button(
                    id("pause"),
                    "Pause",
                    ButtonStyle::Secondary,
                    cx.listener(move |this, _, _, cx| this.pause(&s, cx)),
                ))
            });
            let s = slug_s.clone();
            row = row.child(button(
                id("cancel"),
                "Cancel",
                ButtonStyle::Ghost,
                cx.listener(move |this, _, _, cx| this.cancel(&s, cx)),
            ));
            return div()
                .flex()
                .flex_col()
                .gap_2()
                .children(status)
                .child(row)
                .into_any_element();
        }

        if let Some(pending) = &game.pending {
            let what = if game.installed.is_some() {
                "Update"
            } else {
                "Install"
            };
            let failed = self.failures.get(slug).cloned();
            status = Some(match &failed {
                Some(err) => div()
                    .text_color(theme::bad())
                    .child(format!("{what} stopped: {err}")),
                None => ui::muted(format!("{what} paused · {}", ui::size(pending.size))),
            });
            let s = slug_s.clone();
            row = row.child(button(
                id("resume"),
                if failed.is_some() { "Retry" } else { "Resume" },
                ButtonStyle::Primary,
                cx.listener(move |this, _, _, cx| {
                    let kind = if this.library.get(&s).is_some_and(|g| g.is_installed()) {
                        JobKind::Update
                    } else {
                        JobKind::Install
                    };
                    this.start_job(&s, kind, cx)
                }),
            ));
            let s = slug_s.clone();
            row = row.child(button(
                id("discard"),
                "Cancel",
                ButtonStyle::Ghost,
                cx.listener(move |this, _, _, cx| this.cancel(&s, cx)),
            ));
        } else if game.is_installed() {
            let running = self.running.contains_key(slug);
            let s = slug_s.clone();
            row = row.child(if running {
                ui::into_any(disabled_button("Running"))
            } else {
                ui::into_any(button(
                    id("play"),
                    "Play",
                    ButtonStyle::Primary,
                    cx.listener(move |this, _, _, cx| this.play(&s, cx)),
                ))
            });
            if let Some(update) = &game.update {
                let s = slug_s.clone();
                let size = game
                    .update_download
                    .map(|b| format!(" · {}", ui::size(b)))
                    .unwrap_or_default();
                row = row.child(button(
                    id("update"),
                    format!(
                        "Update{}{size}",
                        update
                            .version
                            .as_deref()
                            .map(|v| format!(" to v{v}"))
                            .unwrap_or_default()
                    ),
                    ButtonStyle::Secondary,
                    cx.listener(move |this, _, _, cx| this.start_job(&s, JobKind::Update, cx)),
                ));
            }
            if let Some(err) = self.failures.get(slug) {
                status = Some(div().text_color(theme::bad()).child(err.clone()));
            }
        }

        let expanded = full || self.open_menus.contains(slug);
        if !full {
            let s = slug_s.clone();
            row = row.child(button(
                id("more"),
                if expanded { "Less" } else { "More" },
                ButtonStyle::Ghost,
                cx.listener(move |this, _, _, cx| {
                    if !this.open_menus.remove(&s) {
                        this.open_menus.insert(s.clone());
                    }
                    cx.notify();
                }),
            ));
        }
        let extras = expanded.then(|| {
            let mut extras = div().flex().flex_wrap().gap_2();
            if game.is_installed() {
                let s = slug_s.clone();
                extras = extras.child(button(
                    id("verify"),
                    "Verify files",
                    ButtonStyle::Ghost,
                    cx.listener(move |this, _, _, cx| this.start_job(&s, JobKind::Repair, cx)),
                ));
                let s = slug_s.clone();
                extras = extras.child(button(
                    id("exe"),
                    "Choose executable",
                    ButtonStyle::Ghost,
                    cx.listener(move |this, _, window, cx| this.choose_executable(&s, window, cx)),
                ));
            }
            let s = slug_s.clone();
            extras = extras.child(button(
                id("folder"),
                "Open folder",
                ButtonStyle::Ghost,
                cx.listener(move |this, _, _, cx| this.open_folder(&s, cx)),
            ));
            let s = slug_s.clone();
            extras.child(button(
                id("uninstall"),
                "Uninstall",
                ButtonStyle::Ghost,
                cx.listener(move |this, _, _, cx| this.uninstall(&s, cx)),
            ))
        });

        div()
            .flex()
            .flex_col()
            .gap_2()
            .children(status)
            .child(row)
            .children(extras)
            .when(full, |d| d.child(install_details(&game)))
            .into_any_element()
    }

    // ---- library ----------------------------------------------------------

    fn library_screen(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut games: Vec<InstalledGame> = self.library.data.games.values().cloned().collect();
        games.sort_by(|a, b| {
            b.last_played
                .cmp(&a.last_played)
                .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        });
        let updates = games.iter().filter(|g| g.update.is_some()).count();

        let rows: Vec<AnyElement> = games
            .iter()
            .map(|game| self.library_row(game, cx).into_any_element())
            .collect();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(ui::heading("Library"))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .when(updates > 1, |d| {
                                d.child(button(
                                    "update-all",
                                    format!("Update all ({updates})"),
                                    ButtonStyle::Secondary,
                                    cx.listener(|this, _, _, cx| this.update_all(cx)),
                                ))
                            })
                            .child(if self.checking_updates {
                                ui::into_any(disabled_button("Checking…"))
                            } else {
                                ui::into_any(button(
                                    "check-updates",
                                    "Check for updates",
                                    ButtonStyle::Ghost,
                                    cx.listener(|this, _, _, cx| this.check_updates(cx)),
                                ))
                            }),
                    ),
            )
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(ui::muted("No games installed. Browse to add one."))
                        .child(button(
                            "browse",
                            "Discover games",
                            ButtonStyle::Secondary,
                            cx.listener(|this, _, _, cx| this.navigate(Screen::Discover, cx)),
                        )),
                )
            })
            .child(div().flex().flex_col().gap_2().children(rows))
    }

    fn library_row(&mut self, game: &InstalledGame, cx: &mut Context<Self>) -> impl IntoElement {
        let image = game
            .capsule_url
            .clone()
            .and_then(|url| self.image(&url, cx));
        let slug = game.slug.clone();
        let mut meta = Vec::new();
        if let Some(b) = &game.installed {
            if let Some(v) = &b.version {
                meta.push(format!("v{v}"));
            }
            meta.push(ui::size(b.size));
        }
        meta.push(game.channel.clone());
        if game.unavailable {
            meta.push("no longer listed".into());
        }

        div()
            .flex()
            .items_center()
            .gap_4()
            .p_2()
            .rounded(theme::RADIUS_CARD)
            .border_1()
            .border_color(theme::line())
            .bg(theme::surface())
            .child(
                div()
                    .id(SharedString::from(format!("row-{slug}")))
                    .flex_none()
                    .w(px(150.))
                    .h(px(70.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.navigate(Screen::Game(slug.clone()), cx)
                    }))
                    .child(ui::cover(image)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .truncate()
                            .child(game.title.clone()),
                    )
                    .child(
                        ui::mono(meta.join(" · "))
                            .text_xs()
                            .text_color(theme::muted()),
                    )
                    .child(self.library_actions(&game.slug, false, cx)),
            )
    }

    // ---- downloads --------------------------------------------------------

    fn downloads(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let slugs: Vec<String> = self
            .library
            .data
            .games
            .values()
            .filter(|g| self.jobs.contains_key(&g.slug) || g.pending.is_some())
            .map(|g| g.slug.clone())
            .collect();
        let updates = self
            .library
            .data
            .games
            .values()
            .filter(|g| {
                g.update.is_some() && g.pending.is_none() && !self.jobs.contains_key(&g.slug)
            })
            .count();

        let games: Vec<InstalledGame> = slugs
            .iter()
            .filter_map(|slug| self.library.get(slug).cloned())
            .collect();
        let rows: Vec<AnyElement> = games
            .into_iter()
            .map(|game| {
                let image = game
                    .capsule_url
                    .clone()
                    .and_then(|url| self.image(&url, cx));
                div()
                    .flex()
                    .gap_4()
                    .p_3()
                    .rounded(theme::RADIUS_CARD)
                    .border_1()
                    .border_color(theme::line())
                    .bg(theme::surface())
                    .child(
                        div()
                            .flex_none()
                            .w(px(120.))
                            .h(px(56.))
                            .child(ui::cover(image)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(game.title.clone()),
                            )
                            .child(self.library_actions(&game.slug, false, cx)),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(ui::heading("Downloads"))
            .when(rows.is_empty(), |d| {
                d.child(ui::muted("Nothing is downloading."))
            })
            .child(div().flex().flex_col().gap_2().children(rows))
            .when(updates > 0, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(ui::muted(format!(
                            "{updates} update{} ready.",
                            if updates == 1 { "" } else { "s" }
                        )))
                        .child(button(
                            "dl-update-all",
                            "Update all",
                            ButtonStyle::Secondary,
                            cx.listener(|this, _, _, cx| this.update_all(cx)),
                        )),
                )
            })
    }

    // ---- settings ---------------------------------------------------------

    fn settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.library.data.settings.clone();
        let toggle = |id: &'static str,
                      label: &'static str,
                      on: bool,
                      cx: &mut Context<Self>,
                      f: fn(&mut Foyer, &mut Context<Foyer>)| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap_3()
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(16.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(if on { theme::accent() } else { theme::line() })
                        .when(on, |d| {
                            d.bg(theme::accent())
                                .text_color(theme::accent_ink())
                                .text_xs()
                                .child("✓")
                        }),
                )
                .child(label)
        };
        let row = |label: &'static str| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .p_4()
                .rounded(theme::RADIUS_CARD)
                .border_1()
                .border_color(theme::line())
                .bg(theme::surface())
                .child(div().font_weight(FontWeight::MEDIUM).child(label))
        };

        div()
            .flex()
            .flex_col()
            .gap_4()
            .max_w(px(720.))
            .child(ui::heading("Settings"))
            .child(
                row("Install folder")
                    .child(ui::muted("New games go into their own folder here. Installed games stay where they are."))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(ui::mono(settings.install_root.display().to_string()).flex_1().truncate())
                            .child(button(
                                "change-root",
                                "Change…",
                                ButtonStyle::Secondary,
                                cx.listener(|this, _, window, cx| this.choose_install_root(window, cx)),
                            )),
                    ),
            )
            .child(
                row("Updates")
                    .child(toggle(
                        "toggle-check",
                        "Check for updates at startup and every 30 minutes",
                        settings.check_updates,
                        cx,
                        Foyer::toggle_check_updates,
                    ))
                    .child(toggle(
                        "toggle-auto",
                        "Install updates automatically",
                        settings.auto_update,
                        cx,
                        Foyer::toggle_auto_update,
                    )),
            )
            .child(
                row("About")
                    .child(ui::muted(format!(
                        "Foyer {} for {}. No account needed: games download anonymously.",
                        env!("CARGO_PKG_VERSION"),
                        platform_name(current_platform())
                    )))
                    .child(ui::mono(format!("Server: {}", self.api.base_url())).text_xs().text_color(theme::muted()))
                    .child(ui::mono(format!("Data: {}", self.data_dir.display())).text_xs().text_color(theme::muted())),
            )
    }
}

fn job_status(kind: JobKind, p: &foyer_install::Progress, stopping: bool) -> Div {
    let text = match (&p.phase, stopping) {
        (_, true) => "Pausing…".to_string(),
        (Phase::Preparing, _) => format!("{} · preparing", kind.label()),
        (Phase::Downloading, _) => {
            let mut text = format!(
                "{} · {} of {}",
                kind.label(),
                ui::size(p.done),
                ui::size(p.total)
            );
            if p.bytes_per_sec > 0.0 {
                text.push_str(&format!(" · {}/s", ui::size(p.bytes_per_sec as u64)));
            }
            if let Some(eta) = ui::eta(p.total.saturating_sub(p.done), p.bytes_per_sec) {
                text.push_str(&format!(" · {eta}"));
            }
            text
        }
        (Phase::Installing, _) => format!("{} · writing files", kind.label()),
        (Phase::Finishing, _) => format!("{} · finishing", kind.label()),
    };
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(ui::progress_bar(p.fraction()))
        .child(ui::mono(text).text_xs().text_color(theme::muted()))
}

fn install_details(game: &InstalledGame) -> Div {
    let mut lines = vec![format!("Folder: {}", game.install_dir.display())];
    if let Some(exe) = &game.launch {
        lines.push(format!("Starts: {exe}"));
    }
    lines.push(format!("Channel: {}", game.channel));
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .text_xs()
        .text_color(theme::faint())
        .font_family(theme::FONT_MONO)
        .children(lines.into_iter().map(|l| div().truncate().child(l)))
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

/// A plain rendering of a game's Markdown description: headings, bullet
/// lists and paragraphs, with inline markers removed.
fn markdown(source: &str) -> Div {
    let mut out = div().flex().flex_col().gap_2().max_w(px(720.));
    for block in source.split("\n\n") {
        let block = block.trim();
        if block.is_empty() {
            continue;
        }
        if let Some(heading) = block.strip_prefix('#') {
            out = out.child(
                div()
                    .pt_2()
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
                    .gap_1()
                    .children(lines.iter().map(|l| {
                        div()
                            .flex()
                            .gap_2()
                            .child(div().text_color(theme::faint()).child("•"))
                            .child(strip_inline(&l.trim_start()[2..]))
                    })),
            );
        } else {
            out = out.child(
                div()
                    .text_color(theme::ink())
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
}

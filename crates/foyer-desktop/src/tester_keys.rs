//! Tester keys: a developer shares a key (`fyk_...`) or a website link
//! (`https://foyer.lol/tester?key=fyk_...`) to let players into a game
//! before it is published, or into its private channels.
//!
//! Keys live in `settings.tester_keys` and go along with every API call
//! (`Api::set_tester_keys`). Unpublished games never appear in the catalog,
//! so adding a key opens the game it unlocks straight away.

use std::collections::HashMap;

use foyer_types::TesterAccess;
use gpui::{
    ClickEvent, Context, Div, FocusHandle, FontWeight, KeyDownEvent, SharedString, Window, div,
    prelude::*, px,
};

use crate::app::{Foyer, Screen};
use crate::runtime;
use crate::theme;
use crate::ui::{self, ButtonStyle, Icon, button, icon, icon_button};

/// What the server says about a key we hold.
pub enum KeyInfo {
    Checking,
    Valid(TesterAccess),
    /// Unknown, expired or revoked, or the check failed.
    Invalid(String),
}

pub struct TesterKeys {
    /// The text in the "Add tester key" field.
    pub input: String,
    pub focus: FocusHandle,
    pub adding: bool,
    pub error: Option<String>,
    pub info: HashMap<String, KeyInfo>,
}

impl TesterKeys {
    pub fn new(cx: &mut Context<Foyer>) -> Self {
        Self {
            input: String::new(),
            focus: cx.focus_handle(),
            adding: false,
            error: None,
            info: HashMap::new(),
        }
    }
}

/// The key in what the player pasted: a bare key or a link with `key=`.
pub fn parse_key(input: &str) -> Option<String> {
    let input = input.trim();
    let key = match input.find("key=") {
        Some(at) => input[at + 4..].split(['&', '#', ' ']).next().unwrap_or(""),
        None => input,
    };
    let valid = key.starts_with("fyk_")
        && key.len() > 4
        && key.len() <= 200
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    valid.then(|| key.to_string())
}

impl Foyer {
    /// Asks the server what each saved key unlocks, for Settings.
    pub fn check_tester_keys(&mut self, cx: &mut Context<Self>) {
        for key in self.library.data.settings.tester_keys.clone() {
            self.tester_keys.info.insert(key.clone(), KeyInfo::Checking);
            let api = self.api.clone();
            let lookup = key.clone();
            let request = runtime::spawn(async move { api.redeem_tester_key(&lookup).await });
            cx.spawn(async move |this, cx| {
                let result = request.await;
                this.update(cx, |this, cx| {
                    let info = match result {
                        Ok(Ok(access)) => KeyInfo::Valid(access),
                        Ok(Err(err)) if err.status().is_some_and(|s| s.as_u16() == 404) => {
                            KeyInfo::Invalid("Expired or revoked".into())
                        }
                        Ok(Err(err)) => KeyInfo::Invalid(err.to_string()),
                        Err(err) => KeyInfo::Invalid(err.to_string()),
                    };
                    this.tester_keys.info.insert(key, info);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// Checks the key in the input field, saves it and opens its game.
    pub fn add_tester_key(&mut self, cx: &mut Context<Self>) {
        if self.tester_keys.adding {
            return;
        }
        let Some(key) = parse_key(&self.tester_keys.input) else {
            self.tester_keys.error =
                Some("That doesn't look like a tester key. Keys start with fyk_.".into());
            cx.notify();
            return;
        };
        self.tester_keys.adding = true;
        self.tester_keys.error = None;
        cx.notify();
        let api = self.api.clone();
        let lookup = key.clone();
        let request = runtime::spawn(async move { api.redeem_tester_key(&lookup).await });
        cx.spawn(async move |this, cx| {
            let result = request.await;
            this.update(cx, |this, cx| {
                this.tester_keys.adding = false;
                let access = match result {
                    Ok(Ok(access)) => access,
                    Ok(Err(err)) => {
                        this.tester_keys.error =
                            Some(if err.status().is_some_and(|s| s.as_u16() == 404) {
                                "This key doesn't work. It may have expired or been revoked.".into()
                            } else {
                                err.to_string()
                            });
                        cx.notify();
                        return;
                    }
                    Err(err) => {
                        this.tester_keys.error = Some(err.to_string());
                        cx.notify();
                        return;
                    }
                };
                let keys = &mut this.library.data.settings.tester_keys;
                if !keys.contains(&key) {
                    keys.insert(0, key.clone());
                    keys.truncate(foyer_types::MAX_KEYS_PER_REQUEST);
                }
                this.api.set_tester_keys(keys);
                if let Err(err) = this.library.save() {
                    this.notify_user(format!("Could not save your library: {err}"), true, cx);
                }
                this.tester_keys.input.clear();
                let (slug, title) = (access.game.clone(), access.title.clone());
                this.tester_keys.info.insert(key, KeyInfo::Valid(access));
                // What the key unlocks may change any page.
                this.pages.clear();
                this.search(cx);
                this.check_updates(cx);
                this.notify_user(format!("Tester access to {title} added"), false, cx);
                this.navigate(Screen::Game(slug), cx);
            })
            .ok();
        })
        .detach();
    }

    pub fn remove_tester_key(&mut self, key: &str, cx: &mut Context<Self>) {
        let keys = &mut self.library.data.settings.tester_keys;
        keys.retain(|k| k != key);
        self.api.set_tester_keys(keys);
        self.tester_keys.info.remove(key);
        if let Err(err) = self.library.save() {
            self.notify_user(format!("Could not save your library: {err}"), true, cx);
        }
        self.pages.clear();
        self.search(cx);
        cx.notify();
    }

    // ---- view -------------------------------------------------------------

    /// The "Tester keys" section in Settings.
    pub fn tester_keys_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let focused = self.tester_keys.focus.is_focused(window);
        let state = &self.tester_keys;
        let empty = state.input.is_empty();

        let field = div()
            .id("tester-key-input")
            .track_focus(&state.focus)
            .flex()
            .items_center()
            .gap(px(8.))
            .flex_1()
            .min_w_0()
            .h(px(32.))
            .px(px(10.))
            .rounded(theme::RADIUS_CONTROL)
            .border_1()
            .border_color(if focused {
                theme::muted()
            } else {
                theme::line()
            })
            .bg(theme::canvas())
            .cursor_text()
            .on_click(cx.listener(|this, _, window, _| window.focus(&this.tester_keys.focus)))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                let key = &event.keystroke;
                let m = &key.modifiers;
                let input = &mut this.tester_keys.input;
                match key.key.as_str() {
                    "enter" => {
                        this.add_tester_key(cx);
                        return;
                    }
                    "escape" => input.clear(),
                    "backspace" => {
                        input.pop();
                    }
                    "v" if m.control || m.platform => {
                        if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                            input.push_str(text.trim());
                            // Cut on a character boundary; pasted text may be anything.
                            *input = input.chars().take(300).collect();
                        }
                    }
                    _ => {
                        if m.control || m.platform || m.alt {
                            return;
                        }
                        if let Some(text) = &key.key_char
                            && input.len() < 300
                        {
                            input.push_str(text);
                        }
                    }
                }
                this.tester_keys.error = None;
                cx.notify();
            }))
            .child(icon(Icon::Shield, 14., theme::faint()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .font_family(theme::FONT_MONO)
                    .text_color(if empty { theme::faint() } else { theme::ink() })
                    .child(div().truncate().child(if empty && !focused {
                        SharedString::from("Paste a key (fyk_…) or a tester link")
                    } else {
                        SharedString::from(state.input.clone())
                    }))
                    .when(focused, |d| {
                        d.child(div().flex_none().w(px(1.)).h(px(14.)).bg(theme::ink()))
                    }),
            );

        let rows: Vec<Div> = self
            .library
            .data
            .settings
            .tester_keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let (title, detail, bad) = match state.info.get(key) {
                    Some(KeyInfo::Valid(access)) => {
                        let channels = if access.channels.is_empty() {
                            "All channels".to_string()
                        } else {
                            access.channels.join(", ")
                        };
                        let expiry = access
                            .expires_at
                            .map(|t| format!(" · until {}", t.format("%-d %b %Y")))
                            .unwrap_or_default();
                        (access.title.clone(), format!("{channels}{expiry}"), false)
                    }
                    Some(KeyInfo::Invalid(why)) => ("Not working".into(), why.clone(), true),
                    _ => ("Checking…".into(), String::new(), false),
                };
                let short: String = key.chars().take(12).collect();
                let slug = match state.info.get(key) {
                    Some(KeyInfo::Valid(access)) => Some(access.game.clone()),
                    _ => None,
                };
                let remove = key.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .py(px(8.))
                    .border_t_1()
                    .border_color(theme::line())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if bad { theme::bad() } else { theme::ink() })
                                    .truncate()
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme::faint())
                                    .truncate()
                                    .child(format!("{short}… · {detail}")),
                            ),
                    )
                    .when_some(slug, |d, slug| {
                        d.child(button(("tester-open", i), "Open").on_click(cx.listener(
                            move |this, _: &ClickEvent, _, cx| {
                                this.navigate(Screen::Game(slug.clone()), cx)
                            },
                        )))
                    })
                    .child(
                        icon_button(("tester-remove", i), Icon::Trash)
                            .style(ButtonStyle::Danger)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.remove_tester_key(&remove, cx)
                            })),
                    )
            })
            .collect();

        ui::card()
            .flex()
            .flex_col()
            .gap(px(14.))
            .p(px(20.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(ui::section_title("Tester keys"))
                    .child(
                        ui::muted(
                            "A developer can give you a key to play a game before it's out, or to join a beta.",
                        )
                        .text_xs(),
                    ),
            )
            .child(
                div().flex().items_center().gap(px(8.)).child(field).child(
                    button(
                        "tester-add",
                        if state.adding { "Checking…" } else { "Add key" },
                    )
                    .disabled(state.adding || empty)
                    .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.add_tester_key(cx))),
                ),
            )
            .when_some(state.error.clone(), |d, error| {
                d.child(div().text_xs().text_color(theme::bad()).child(error))
            })
            .when(!rows.is_empty(), |d| d.child(div().flex().flex_col().children(rows)))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_key;

    #[test]
    fn reads_bare_keys_and_links() {
        assert_eq!(parse_key("  fyk_abc123 \n").as_deref(), Some("fyk_abc123"));
        assert_eq!(
            parse_key("https://foyer.lol/tester?key=fyk_abc-123").as_deref(),
            Some("fyk_abc-123")
        );
        assert_eq!(
            parse_key("https://foyer.lol/tester?key=fyk_abc&utm=x").as_deref(),
            Some("fyk_abc")
        );
    }

    #[test]
    fn rejects_other_text() {
        assert_eq!(parse_key(""), None);
        assert_eq!(parse_key("fyk_"), None);
        assert_eq!(parse_key("hello"), None);
        assert_eq!(parse_key("https://foyer.lol/games/x"), None);
        assert_eq!(parse_key("fyk_a b"), None);
        assert_eq!(parse_key("fyk_<script>"), None);
    }
}

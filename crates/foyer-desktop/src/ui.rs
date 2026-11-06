//! Small building blocks shared by the screens.

use gpui::{
    AnyElement, ClickEvent, Div, ElementId, FontWeight, SharedString, Stateful, Window, div,
    prelude::*, px, relative,
};

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Light; at most one per screen.
    Primary,
    /// Outlined.
    Secondary,
    /// Coral: supporting the developer only.
    Support,
    /// Text only.
    Ghost,
}

pub fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    style: ButtonStyle,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> Stateful<Div> {
    let base = div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .px(px(14.))
        .py(px(7.))
        .rounded(theme::RADIUS_CONTROL)
        .text_size(px(13.))
        .font_weight(FontWeight::MEDIUM)
        .cursor_pointer()
        .child(label.into())
        .on_click(on_click);
    match style {
        ButtonStyle::Primary => base
            .bg(theme::accent())
            .text_color(theme::accent_ink())
            .hover(|s| s.opacity(0.9)),
        ButtonStyle::Secondary => base
            .border_1()
            .border_color(theme::line())
            .text_color(theme::ink())
            .hover(|s| s.bg(theme::raised())),
        ButtonStyle::Support => base
            .bg(theme::support())
            .text_color(theme::support_ink())
            .hover(|s| s.opacity(0.9)),
        ButtonStyle::Ghost => base
            .text_color(theme::muted())
            .hover(|s| s.text_color(theme::ink()).bg(theme::raised())),
    }
}

/// A greyed-out button that does nothing.
pub fn disabled_button(label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(14.))
        .py(px(7.))
        .rounded(theme::RADIUS_CONTROL)
        .border_1()
        .border_color(theme::line())
        .text_size(px(13.))
        .text_color(theme::faint())
        .child(label.into())
}

pub fn pill(label: impl Into<SharedString>) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded(theme::RADIUS_CONTROL)
        .bg(theme::raised())
        .text_xs()
        .text_color(theme::muted())
        .child(label.into())
}

/// Coral progress bar.
pub fn progress_bar(fraction: f32) -> Div {
    div()
        .w_full()
        .h(px(4.))
        .rounded(px(2.))
        .bg(theme::raised())
        .child(
            div()
                .h_full()
                .rounded(px(2.))
                .bg(theme::support())
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

pub fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .text_xl()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ink())
        .child(text.into())
}

pub fn muted(text: impl Into<SharedString>) -> Div {
    div().text_color(theme::muted()).child(text.into())
}

pub fn mono(text: impl Into<SharedString>) -> Div {
    div().font_family(theme::FONT_MONO).child(text.into())
}

/// A rounded box for cover art: the image when it is loaded, a plain panel
/// before.
pub fn cover(image: Option<std::path::PathBuf>) -> Div {
    let frame = div()
        .w_full()
        .h_full()
        .rounded(px(4.))
        .overflow_hidden()
        .bg(theme::raised());
    match image {
        Some(path) => frame.child(
            gpui::img(path)
                .size_full()
                .object_fit(gpui::ObjectFit::Cover),
        ),
        None => frame,
    }
}

/// `1.4 GB`, `820 MB`, `12 KB`.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

/// `3 min left`, `under a minute left`.
pub fn eta(remaining: u64, bytes_per_sec: f64) -> Option<String> {
    if bytes_per_sec < 1.0 {
        return None;
    }
    let secs = remaining as f64 / bytes_per_sec;
    Some(if secs < 60.0 {
        "under a minute left".into()
    } else if secs < 3600.0 {
        format!("{} min left", (secs / 60.0).ceil() as u64)
    } else {
        format!("{:.1} h left", secs / 3600.0)
    })
}

pub fn into_any(el: impl IntoElement) -> AnyElement {
    el.into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_sizes() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1_500), "1.5 KB");
        assert_eq!(size(820_000_000), "820 MB");
        assert_eq!(size(1_400_000_000), "1.4 GB");
    }

    #[test]
    fn formats_eta() {
        assert_eq!(eta(100, 0.0), None);
        assert_eq!(eta(100, 10.0).as_deref(), Some("under a minute left"));
        assert_eq!(eta(6_000, 10.0).as_deref(), Some("10 min left"));
    }
}

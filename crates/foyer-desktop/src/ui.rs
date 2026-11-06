//! Building blocks shared by the screens: icons, buttons, chips, covers,
//! progress bars and formatting helpers. Everything uses the Midnight
//! tokens from `theme`.

use std::borrow::Cow;
use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, AssetSource, ClickEvent, Div, ElementId, FontWeight,
    Hsla, ObjectFit, Pixels, Rgba, SharedString, Stateful, Svg, Window, div, img,
    linear_color_stop, linear_gradient, prelude::*, pulsating_between, px, relative, svg,
};

use crate::theme;

// ---- icons ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Alert,
    Apple,
    ArrowLeft,
    Check,
    Compass,
    Download,
    External,
    Folder,
    Heart,
    Library,
    Linux,
    Logo,
    Pause,
    Play,
    Refresh,
    Search,
    Settings,
    Shield,
    Terminal,
    Trash,
    Windows,
    X,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::Alert => "icons/alert.svg",
            Icon::Apple => "icons/apple.svg",
            Icon::ArrowLeft => "icons/arrow-left.svg",
            Icon::Check => "icons/check.svg",
            Icon::Compass => "icons/compass.svg",
            Icon::Download => "icons/download.svg",
            Icon::External => "icons/external.svg",
            Icon::Folder => "icons/folder.svg",
            Icon::Heart => "icons/heart.svg",
            Icon::Library => "icons/library.svg",
            Icon::Linux => "icons/linux.svg",
            Icon::Logo => "icons/logo.svg",
            Icon::Pause => "icons/pause.svg",
            Icon::Play => "icons/play.svg",
            Icon::Refresh => "icons/refresh.svg",
            Icon::Search => "icons/search.svg",
            Icon::Settings => "icons/settings.svg",
            Icon::Shield => "icons/shield.svg",
            Icon::Terminal => "icons/terminal.svg",
            Icon::Trash => "icons/trash.svg",
            Icon::Windows => "icons/windows.svg",
            Icon::X => "icons/x.svg",
        }
    }
}

/// The bundled icons, served to GPUI's `svg()` element.
pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        macro_rules! icons {
            ($($name:literal),* $(,)?) => {
                match path {
                    $(concat!("icons/", $name, ".svg") => Some(include_bytes!(concat!("../assets/icons/", $name, ".svg")).as_slice()),)*
                    _ => None,
                }
            };
        }
        let bytes = icons!(
            "alert",
            "apple",
            "arrow-left",
            "check",
            "compass",
            "download",
            "external",
            "folder",
            "heart",
            "library",
            "linux",
            "logo",
            "pause",
            "play",
            "refresh",
            "search",
            "settings",
            "shield",
            "terminal",
            "trash",
            "windows",
            "x",
        );
        Ok(bytes.map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(Vec::new())
    }
}

pub fn icon(icon: Icon, size: f32, color: impl Into<Hsla>) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(px(size))
        .text_color(color.into())
}

// ---- buttons ----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonStyle {
    /// Light; at most one per screen.
    Primary,
    /// Raised surface with a border.
    Secondary,
    /// Coral: supporting the developer only.
    Support,
    /// Text only until hovered.
    Ghost,
    /// Ghost, red on hover: destructive actions.
    Danger,
}

/// A button builder. Finish it with [`Button::on_click`].
pub struct Button {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<Icon>,
    style: ButtonStyle,
    large: bool,
    full: bool,
    disabled: bool,
}

pub fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button {
        id: id.into(),
        label: Some(label.into()),
        icon: None,
        style: ButtonStyle::Secondary,
        large: false,
        full: false,
        disabled: false,
    }
}

pub fn icon_button(id: impl Into<ElementId>, icon: Icon) -> Button {
    Button {
        id: id.into(),
        label: None,
        icon: Some(icon),
        style: ButtonStyle::Ghost,
        large: false,
        full: false,
        disabled: false,
    }
}

impl Button {
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = style;
        self
    }
    pub fn primary(self) -> Self {
        self.style(ButtonStyle::Primary)
    }
    pub fn ghost(self) -> Self {
        self.style(ButtonStyle::Ghost)
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn large(mut self) -> Self {
        self.large = true;
        self
    }
    pub fn full_width(mut self) -> Self {
        self.full = true;
        self
    }
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_click(
        self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Stateful<Div> {
        let (bg, fg, border): (Option<Rgba>, Rgba, Option<Rgba>) = match self.style {
            ButtonStyle::Primary => (Some(theme::accent()), theme::accent_ink(), None),
            ButtonStyle::Secondary => (Some(theme::raised()), theme::ink(), Some(theme::line())),
            ButtonStyle::Support => (Some(theme::support()), theme::support_ink(), None),
            ButtonStyle::Ghost | ButtonStyle::Danger => (None, theme::muted(), None),
        };
        let fg = if self.disabled { theme::faint() } else { fg };
        let icon_only = self.label.is_none();
        let (h, px_x, text) = match (self.large, icon_only) {
            (true, _) => (px(40.), px(18.), px(14.)),
            (false, true) => (px(30.), px(7.), px(13.)),
            (false, false) => (px(32.), px(13.), px(13.)),
        };
        let style = self.style;
        let disabled = self.disabled;

        let mut el = div()
            .id(self.id)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .h(h)
            .px(px_x)
            .rounded(theme::RADIUS_CONTROL)
            .text_size(text)
            .font_weight(FontWeight::MEDIUM)
            .text_color(fg)
            .when(self.full, |d| d.w_full())
            .when_some(bg, |d, bg| d.bg(bg))
            .when_some(border, |d, b| d.border_1().border_color(b))
            .when_some(self.icon, |d, i| {
                d.child(icon(i, if self.large { 16. } else { 15. }, fg))
            })
            .when_some(self.label, |d, l| d.child(l));

        if disabled {
            el = el.opacity(0.6).cursor_default();
            if style == ButtonStyle::Primary {
                el = el.bg(theme::raised()).text_color(theme::faint());
            }
            return el;
        }
        el.cursor_pointer()
            .hover(move |s| match style {
                ButtonStyle::Primary | ButtonStyle::Support => s.opacity(0.88),
                ButtonStyle::Secondary => s.bg(theme::line()),
                ButtonStyle::Ghost => s.bg(theme::raised()).text_color(theme::ink()),
                ButtonStyle::Danger => s.bg(theme::raised()).text_color(theme::bad()),
            })
            .active(|s| s.opacity(0.75))
            .on_click(handler)
    }
}

// ---- chips, pills and labels --------------------------------------------------

/// A rounded tag chip, like the website's.
pub fn chip(label: impl Into<SharedString>, active: bool) -> Div {
    div()
        .flex_none()
        .px(px(10.))
        .py(px(3.))
        .rounded_full()
        .text_xs()
        .when(active, |d| {
            d.bg(theme::accent())
                .text_color(theme::accent_ink())
                .font_weight(FontWeight::MEDIUM)
        })
        .when(!active, |d| {
            d.bg(theme::raised()).text_color(theme::muted())
        })
        .child(label.into())
}

/// A small status badge, tinted with a status colour.
pub fn badge(label: impl Into<SharedString>, color: Rgba) -> Div {
    let mut tint = color;
    tint.a = 0.16;
    div()
        .flex_none()
        .px(px(8.))
        .py(px(2.))
        .rounded_full()
        .bg(tint)
        .text_color(color)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .child(label.into())
}

/// A badge that sits on top of cover art, readable on any image.
pub fn overlay_badge(label: impl Into<SharedString>, color: Rgba) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .px(px(8.))
        .py(px(3.))
        .rounded_full()
        .bg(rgba_with(theme::canvas(), 0.82))
        .text_color(color)
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .child(label.into())
}

pub fn eyebrow(text: impl Into<SharedString>) -> Div {
    div()
        .text_xs()
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme::faint())
        .child(text.into().to_uppercase())
}

pub fn heading(text: impl Into<SharedString>) -> Div {
    div()
        .text_2xl()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::ink())
        .child(text.into())
}

pub fn section_title(text: impl Into<SharedString>) -> Div {
    div()
        .text_base()
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

/// A bordered surface panel.
pub fn card() -> Div {
    div()
        .rounded(theme::RADIUS_CARD)
        .border_1()
        .border_color(theme::line())
        .bg(theme::surface())
}

/// Label + value line for detail lists.
pub fn detail(label: impl Into<SharedString>, value: impl IntoElement) -> Div {
    div()
        .flex()
        .justify_between()
        .gap_4()
        .py(px(7.))
        .border_b_1()
        .border_color(theme::line())
        .child(
            div()
                .flex_none()
                .text_color(theme::muted())
                .child(label.into()),
        )
        .child(div().min_w_0().truncate().child(value))
}

// ---- covers and progress ------------------------------------------------------

/// Cover art filling its parent, with a pulsing placeholder while it loads.
pub fn cover(id: impl Into<ElementId>, image: Option<PathBuf>, radius: Pixels) -> AnyElement {
    match image {
        // `Cover` scales past the box, so clip what spills over.
        Some(path) => div()
            .size_full()
            .overflow_hidden()
            .rounded(radius)
            .child(
                img(path)
                    .size_full()
                    .rounded(radius)
                    .object_fit(ObjectFit::Cover),
            )
            .into_any_element(),
        None => skeleton(id, div().size_full().rounded(radius)).into_any_element(),
    }
}

/// A pulsing placeholder in the shape of `block`.
pub fn skeleton(id: impl Into<ElementId>, block: Div) -> gpui::AnimationElement<Div> {
    block.bg(theme::raised()).with_animation(
        id,
        Animation::new(Duration::from_millis(1400))
            .repeat()
            .with_easing(pulsating_between(0.45, 1.0)),
        |d, delta| d.opacity(delta),
    )
}

/// A gradient from the canvas colour to transparent, laid over art so text
/// on top stays readable. `angle` follows CSS: 0 = towards the top.
pub fn scrim(angle: f32, strength: f32) -> Div {
    let solid = rgba_with(theme::canvas(), strength);
    let clear = rgba_with(theme::canvas(), 0.0);
    div().absolute().inset_0().bg(linear_gradient(
        angle,
        linear_color_stop(solid, 0.0),
        linear_color_stop(clear, 1.0),
    ))
}

/// Coral progress bar.
pub fn progress_bar(fraction: f32, height: f32) -> Div {
    div()
        .w_full()
        .h(px(height))
        .rounded_full()
        .bg(theme::raised())
        .child(
            div()
                .h_full()
                .rounded_full()
                .bg(theme::support())
                .w(relative(fraction.clamp(0.0, 1.0))),
        )
}

pub fn rgba_with(color: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..color }
}

// ---- formatting -----------------------------------------------------------------

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

/// `today`, `yesterday`, `3 days ago`, `12 Mar 2026`.
pub fn relative_day(when: chrono::DateTime<chrono::Utc>) -> String {
    let days = (chrono::Utc::now().date_naive() - when.date_naive()).num_days();
    match days {
        ..=0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        _ => when.format("%-d %b %Y").to_string(),
    }
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

    #[test]
    fn serves_every_icon() {
        for icon in [Icon::Play, Icon::Windows, Icon::Linux, Icon::Apple, Icon::X] {
            assert!(Assets.load(icon.path()).unwrap().is_some(), "{icon:?}");
        }
    }
}

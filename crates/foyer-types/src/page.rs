//! How a developer styles their game page.
//!
//! A [`PageStyle`] is stored with each game. Everything in it is optional:
//! an empty style is the standard Store look. The website and the desktop
//! client turn a style into concrete colours with [`Palette::for_style`],
//! which derives every text and surface colour from the developer's two
//! choices (background and accent) so the page always stays readable. The
//! API refuses styles that can't be made readable ([`PageStyle::check`]).
//!
//! The checks live here, not in the API, so the website can show the same
//! answers while a developer edits, and the in-memory mock agrees with the
//! real API.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{Media, MediaKind};

/// The most content blocks a page can have.
pub const MAX_BLOCKS: usize = 24;
/// The most bullets in a features block.
pub const MAX_FEATURES: usize = 12;

/// WCAG 2.2 AA contrast for body text.
pub const MIN_TEXT_CONTRAST: f64 = 4.5;
/// WCAG 2.2 AA contrast for buttons and other non-text UI (1.4.11).
pub const MIN_UI_CONTRAST: f64 = 3.0;

/// How a game page looks.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct PageStyle {
    /// Page background as `#rrggbb`. Text and panel colours are derived from
    /// it. `None` keeps Foyer's dark grey.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    /// Download button, focus ring and highlights as `#rrggbb`. `None` keeps
    /// Foyer's green.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent: Option<String>,
    /// Wide art shown across the top of the page: the id of one of the game's
    /// `artwork` or `screenshot` images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hero: Option<Uuid>,
    /// The game's logo, shown instead of the plain-text title: the id of one
    /// of the game's `artwork` images. The title stays in the page for screen
    /// readers and search engines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<Uuid>,
    #[serde(default)]
    pub title_style: TitleStyle,
    /// The main column, top to bottom. Empty means the standard layout:
    /// screenshots, then the description.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<PageBlock>,
}

/// How the title is set when there is no logo.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TitleStyle {
    #[default]
    Standard,
    /// Big uppercase letters, like promotional art.
    Uppercase,
}

impl TitleStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            TitleStyle::Standard => "standard",
            TitleStyle::Uppercase => "uppercase",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "standard" => Some(TitleStyle::Standard),
            "uppercase" => Some(TitleStyle::Uppercase),
            _ => None,
        }
    }
}

/// One piece of the main column of a game page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PageBlock {
    /// The screenshot gallery. At most once.
    Screenshots,
    /// The game's description ("About this game"). At most once.
    About,
    /// A section with a heading and plain-text paragraphs (blank line
    /// between paragraphs).
    Text { heading: String, body: String },
    /// A bulleted list, such as key features.
    Features { heading: String, items: Vec<String> },
    /// A quote from a review or a player.
    Quote { text: String, source: String },
    /// One of the game's images, full width.
    Image {
        media: Uuid,
        #[serde(default)]
        caption: String,
    },
    /// A YouTube or Vimeo video.
    Trailer { url: String },
}

impl PageBlock {
    /// The `type` tag, as serialized.
    pub fn kind(&self) -> &'static str {
        match self {
            PageBlock::Screenshots => "screenshots",
            PageBlock::About => "about",
            PageBlock::Text { .. } => "text",
            PageBlock::Features { .. } => "features",
            PageBlock::Quote { .. } => "quote",
            PageBlock::Image { .. } => "image",
            PageBlock::Trailer { .. } => "trailer",
        }
    }
}

/// Why a [`PageStyle`] was refused: the field and a sentence for the
/// developer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageError {
    pub field: String,
    pub message: String,
}

impl PageError {
    fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

fn check_len(field: &str, value: &str, max: usize, required: bool) -> Result<(), PageError> {
    if required && value.trim().is_empty() {
        return Err(PageError::new(field, "can't be empty"));
    }
    if value.chars().count() > max {
        return Err(PageError::new(
            field,
            format!("must be at most {max} characters"),
        ));
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(PageError::new(field, "contains control characters"));
    }
    Ok(())
}

impl PageStyle {
    /// The blocks to render: the developer's, or the standard layout.
    pub fn layout(&self) -> Vec<PageBlock> {
        if self.blocks.is_empty() {
            vec![PageBlock::Screenshots, PageBlock::About]
        } else {
            self.blocks.clone()
        }
    }

    /// Lowercases colours and drops whitespace, so equal styles compare
    /// equal. Call before [`Self::check`] and storing.
    pub fn normalize(&mut self) {
        for colour in [&mut self.background, &mut self.accent] {
            *colour = colour
                .take()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .map(|c| match Rgb::parse(&c) {
                    Some(rgb) => rgb.hex(),
                    None => c,
                });
        }
    }

    /// Checks the style against the game's images. Refuses colours that
    /// can't be made readable, references to images the game doesn't have,
    /// unknown video hosts and oversized content.
    pub fn check(&self, media: &[Media]) -> Result<(), PageError> {
        let find = |id: Uuid| media.iter().find(|m| m.id == id);
        for (field, value) in [("background", &self.background), ("accent", &self.accent)] {
            if let Some(value) = value
                && Rgb::parse(value).is_none()
            {
                return Err(PageError::new(field, "must be a colour like #1a2b3c"));
            }
        }
        Palette::for_style(self).check()?;

        if let Some(hero) = self.hero
            && !find(hero)
                .is_some_and(|m| matches!(m.kind, MediaKind::Artwork | MediaKind::Screenshot))
        {
            return Err(PageError::new("hero", "pick one of the game's images"));
        }
        if let Some(logo) = self.logo
            && !find(logo).is_some_and(|m| m.kind == MediaKind::Artwork)
        {
            return Err(PageError::new(
                "logo",
                "pick one of the game's artwork images",
            ));
        }

        if self.blocks.len() > MAX_BLOCKS {
            return Err(PageError::new(
                "blocks",
                format!("a page can have at most {MAX_BLOCKS} blocks"),
            ));
        }
        let (mut screenshots, mut about) = (0, 0);
        for (i, block) in self.blocks.iter().enumerate() {
            let field = |name: &str| format!("blocks[{i}].{name}");
            match block {
                PageBlock::Screenshots => screenshots += 1,
                PageBlock::About => about += 1,
                PageBlock::Text { heading, body } => {
                    check_len(&field("heading"), heading, 80, false)?;
                    check_len(&field("body"), body, 5000, true)?;
                }
                PageBlock::Features { heading, items } => {
                    check_len(&field("heading"), heading, 80, false)?;
                    if items.is_empty() || items.len() > MAX_FEATURES {
                        return Err(PageError::new(
                            field("items"),
                            format!("list 1 to {MAX_FEATURES} features"),
                        ));
                    }
                    for item in items {
                        check_len(&field("items"), item, 160, true)?;
                    }
                }
                PageBlock::Quote { text, source } => {
                    check_len(&field("text"), text, 400, true)?;
                    check_len(&field("source"), source, 80, false)?;
                }
                PageBlock::Image { media: id, caption } => {
                    if !find(*id).is_some_and(|m| m.kind != MediaKind::Capsule) {
                        return Err(PageError::new(
                            field("media"),
                            "pick one of the game's images",
                        ));
                    }
                    check_len(&field("caption"), caption, 160, false)?;
                }
                PageBlock::Trailer { url } => {
                    if trailer_embed(url).is_none() {
                        return Err(PageError::new(
                            field("url"),
                            "must be a YouTube or Vimeo video link",
                        ));
                    }
                }
            }
        }
        if screenshots > 1 || about > 1 {
            return Err(PageError::new(
                "blocks",
                "the screenshots and about blocks can each appear once",
            ));
        }
        Ok(())
    }

    /// Ids of the images the style points at.
    pub fn media_ids(&self) -> impl Iterator<Item = Uuid> + '_ {
        self.hero
            .into_iter()
            .chain(self.logo)
            .chain(self.blocks.iter().filter_map(|b| match b {
                PageBlock::Image { media, .. } => Some(*media),
                _ => None,
            }))
    }

    /// Forgets an image that was deleted.
    pub fn forget_media(&mut self, id: Uuid) {
        if self.hero == Some(id) {
            self.hero = None;
        }
        if self.logo == Some(id) {
            self.logo = None;
        }
        self.blocks
            .retain(|b| !matches!(b, PageBlock::Image { media, .. } if *media == id));
    }
}

/// The privacy-friendly embed URL for a YouTube or Vimeo link, or `None` for
/// anything else. Only the video id is taken from the input, so the result
/// is always a fixed host and a safe path.
pub fn trailer_embed(url: &str) -> Option<String> {
    let url = url.trim();
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let path = path.trim_end_matches('/');

    let youtube_id = |id: &str| {
        (id.len() == 11
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .then(|| format!("https://www.youtube-nocookie.com/embed/{id}"))
    };
    match host {
        "youtube.com" | "youtube-nocookie.com" => {
            if path == "watch" {
                let id = query.split('&').find_map(|pair| pair.strip_prefix("v="))?;
                youtube_id(id)
            } else {
                let id = path
                    .strip_prefix("embed/")
                    .or_else(|| path.strip_prefix("shorts/"))
                    .or_else(|| path.strip_prefix("live/"))?;
                youtube_id(id)
            }
        }
        "youtu.be" => youtube_id(path),
        "vimeo.com" | "player.vimeo.com" => {
            let id = path.strip_prefix("video/").unwrap_or(path);
            (!id.is_empty() && id.len() <= 12 && id.chars().all(|c| c.is_ascii_digit()))
                .then(|| format!("https://player.vimeo.com/video/{id}?dnt=1"))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- colour

/// An sRGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// `#rrggbb` or `#rgb`, case-insensitive.
    pub fn parse(value: &str) -> Option<Self> {
        let hex = value.trim().strip_prefix('#')?;
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |s: &str| u8::from_str_radix(s, 16).ok();
        match hex.len() {
            6 => Some(Rgb(
                channel(&hex[0..2])?,
                channel(&hex[2..4])?,
                channel(&hex[4..6])?,
            )),
            3 => {
                let d = |i: usize| channel(&hex[i..i + 1]).map(|v| v * 17);
                Some(Rgb(d(0)?, d(1)?, d(2)?))
            }
            _ => None,
        }
    }

    pub const fn from_u32(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    pub fn to_u32(self) -> u32 {
        (self.0 as u32) << 16 | (self.1 as u32) << 8 | self.2 as u32
    }

    /// `#rrggbb`, lowercase.
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }

    /// WCAG relative luminance.
    pub fn luminance(self) -> f64 {
        let lin = |c: u8| {
            let c = c as f64 / 255.0;
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(self.0) + 0.7152 * lin(self.1) + 0.0722 * lin(self.2)
    }

    /// WCAG contrast ratio, 1 to 21.
    pub fn contrast(self, other: Rgb) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// `self` moved `amount` (0 to 1) of the way towards `other`.
    pub fn mix(self, other: Rgb, amount: f64) -> Rgb {
        let m = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * amount).round() as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }
}

/// The Store design tokens (`docs/design-language.md`).
pub mod store {
    use super::Rgb;
    pub const CANVAS: Rgb = Rgb::from_u32(0x121214);
    pub const CHROME: Rgb = Rgb::from_u32(0x18181b);
    pub const SURFACE: Rgb = Rgb::from_u32(0x1e1e22);
    pub const RAISED: Rgb = Rgb::from_u32(0x2a2a2e);
    pub const LINE: Rgb = Rgb::from_u32(0x2e2e33);
    pub const INK: Rgb = Rgb::from_u32(0xf0f0f2);
    pub const MUTED: Rgb = Rgb::from_u32(0xa6a6ad);
    pub const FAINT: Rgb = Rgb::from_u32(0x8a8a92);
    pub const ACCENT: Rgb = Rgb::from_u32(0x38c172);
    pub const ACCENT_INK: Rgb = Rgb::from_u32(0x0c1f13);
    pub const INFO: Rgb = Rgb::from_u32(0x6fb8ff);
    /// Ink for light backgrounds.
    pub const DARK_INK: Rgb = Rgb::from_u32(0x141416);
}

/// A full set of colours for one game page, derived from its [`PageStyle`].
/// Token names match the design language, so the website can set them as
/// CSS variables and every component picks them up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub canvas: Rgb,
    pub surface: Rgb,
    pub raised: Rgb,
    pub line: Rgb,
    pub ink: Rgb,
    pub muted: Rgb,
    pub faint: Rgb,
    pub accent: Rgb,
    pub accent_ink: Rgb,
    pub info: Rgb,
    /// Whether the page is light with dark text.
    pub light: bool,
}

/// One contrast pairing on a page, for the editor's readout.
#[derive(Debug, Clone, PartialEq)]
pub struct ContrastCheck {
    pub label: &'static str,
    pub ratio: f64,
    pub minimum: f64,
}

impl ContrastCheck {
    pub fn passes(&self) -> bool {
        self.ratio + 1e-9 >= self.minimum
    }
}

/// Moves `colour` towards `towards` until it reaches `minimum` contrast
/// against every colour in `against`. Starts at `start` (the share of
/// `towards` to mix in) and only moves closer to `towards`.
fn readable(colour: Rgb, towards: Rgb, against: &[Rgb], minimum: f64, start: f64) -> Rgb {
    let mut amount = start;
    loop {
        let candidate = colour.mix(towards, amount);
        if amount >= 1.0 || against.iter().all(|bg| candidate.contrast(*bg) >= minimum) {
            return candidate;
        }
        amount = (amount + 0.02).min(1.0);
    }
}

/// Text for a button filled with `fill`: a deep shade of the fill, or white,
/// whichever reads better; plain black if neither reaches AA.
fn ink_on(fill: Rgb) -> Rgb {
    let dark = fill.mix(Rgb(0, 0, 0), 0.85);
    let white = Rgb(255, 255, 255);
    let best = if fill.contrast(dark) >= fill.contrast(white) {
        dark
    } else {
        white
    };
    if best.contrast(fill) >= MIN_TEXT_CONTRAST {
        best
    } else {
        Rgb(0, 0, 0)
    }
}

impl Palette {
    /// The standard Store colours.
    pub fn store() -> Self {
        Palette {
            canvas: store::CANVAS,
            surface: store::SURFACE,
            raised: store::RAISED,
            line: store::LINE,
            ink: store::INK,
            muted: store::MUTED,
            faint: store::FAINT,
            accent: store::ACCENT,
            accent_ink: store::ACCENT_INK,
            info: store::INFO,
            light: false,
        }
    }

    /// Derives the page's colours. Invalid colours fall back to the Store
    /// defaults, so rendering never fails; [`PageStyle::check`] refuses them
    /// on save.
    pub fn for_style(style: &PageStyle) -> Self {
        let background = style.background.as_deref().and_then(Rgb::parse);
        let accent = style.accent.as_deref().and_then(Rgb::parse);
        let mut p = Palette::store();
        if let Some(canvas) = background
            && canvas != store::CANVAS
        {
            // Light or dark text, whichever reads better.
            let light = canvas.contrast(store::DARK_INK) > canvas.contrast(store::INK);
            let ink = if light { store::DARK_INK } else { store::INK };
            p.light = light;
            p.canvas = canvas;
            p.ink = ink;
            // Panels step towards the text colour, like the Store greys do.
            p.surface = canvas.mix(ink, 0.05);
            p.raised = canvas.mix(ink, 0.10);
            p.line = canvas.mix(ink, 0.12);
            let text_bgs = [p.canvas, p.surface];
            p.muted = readable(canvas, ink, &text_bgs, MIN_TEXT_CONTRAST, 0.62);
            p.faint = readable(canvas, ink, &text_bgs, MIN_TEXT_CONTRAST, 0.52);
            p.info = if light {
                readable(
                    Rgb::from_u32(0x1f5fae),
                    ink,
                    &text_bgs,
                    MIN_TEXT_CONTRAST,
                    0.0,
                )
            } else {
                readable(store::INFO, ink, &text_bgs, MIN_TEXT_CONTRAST, 0.0)
            };
        }
        if let Some(accent) = accent {
            p.accent = accent;
            p.accent_ink = ink_on(accent);
        } else if background.is_some() {
            // Keep Foyer's green, nudged until it stands out from the page:
            // darker on light pages (dark enough for white text), lighter on
            // dark ones.
            let white = Rgb(255, 255, 255);
            let towards = if p.light { Rgb(0, 0, 0) } else { white };
            let mut amount = 0.0;
            loop {
                let green = store::ACCENT.mix(towards, amount);
                let carries_white = !p.light || green.contrast(white) >= MIN_TEXT_CONTRAST;
                if amount >= 1.0 || (green.contrast(p.canvas) >= MIN_UI_CONTRAST && carries_white) {
                    p.accent = green;
                    break;
                }
                amount += 0.02;
            }
            p.accent_ink = if p.accent == store::ACCENT {
                store::ACCENT_INK
            } else {
                ink_on(p.accent)
            };
        }
        p
    }

    /// The pairings that must stay readable, with their ratios.
    pub fn contrast_checks(&self) -> Vec<ContrastCheck> {
        let check = |label, a: Rgb, b: Rgb, minimum| ContrastCheck {
            label,
            ratio: a.contrast(b),
            minimum,
        };
        vec![
            check(
                "Text on the background",
                self.ink,
                self.canvas,
                MIN_TEXT_CONTRAST,
            ),
            check(
                "Secondary text on panels",
                self.muted,
                self.surface,
                MIN_TEXT_CONTRAST,
            ),
            check(
                "Hints on panels",
                self.faint,
                self.surface,
                MIN_TEXT_CONTRAST,
            ),
            check(
                "Links on the background",
                self.info,
                self.canvas,
                MIN_TEXT_CONTRAST,
            ),
            check(
                "Button text on the accent",
                self.accent_ink,
                self.accent,
                MIN_TEXT_CONTRAST,
            ),
            check(
                "Accent against the background",
                self.accent,
                self.canvas,
                MIN_UI_CONTRAST,
            ),
        ]
    }

    /// Refuses palettes with a pairing below WCAG AA.
    pub fn check(&self) -> Result<(), PageError> {
        for c in self.contrast_checks() {
            if !c.passes() {
                let field = if c.label.contains("ccent") {
                    "accent"
                } else {
                    "background"
                };
                let advice = if field == "accent" {
                    "pick an accent that stands out more from the background"
                } else {
                    "pick a darker or lighter background; mid tones can't fit readable text"
                };
                return Err(PageError::new(
                    field,
                    format!(
                        "{} has contrast {:.1}:1, below {:.1}:1; {advice}",
                        c.label.to_lowercase(),
                        c.ratio,
                        c.minimum
                    ),
                ));
            }
        }
        Ok(())
    }

    /// CSS custom properties that override the Store tokens, for a `style`
    /// attribute. Only hex colours are emitted, so it is safe to inline.
    pub fn css_vars(&self) -> String {
        let vars = [
            ("canvas", self.canvas),
            ("chrome", self.canvas),
            ("surface", self.surface),
            ("raised", self.raised),
            ("line", self.line),
            ("ink", self.ink),
            ("muted", self.muted),
            ("faint", self.faint),
            ("accent", self.accent),
            ("accent-ink", self.accent_ink),
            ("info", self.info),
        ];
        let mut css = String::new();
        for (name, colour) in vars {
            css.push_str(&format!("--color-{name}:{};", colour.hex()));
        }
        css.push_str(if self.light {
            "color-scheme:light;"
        } else {
            "color-scheme:dark;"
        });
        css
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(kind: MediaKind) -> Media {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Media {
            id: Uuid::from_u128(NEXT.fetch_add(1, Ordering::Relaxed) as u128),
            kind,
            url: String::new(),
            position: 0,
        }
    }

    #[test]
    fn store_palette_matches_the_design_tokens() {
        assert_eq!(Palette::for_style(&PageStyle::default()), Palette::store());
        assert!(Palette::store().check().is_ok());
    }

    #[test]
    fn parses_and_formats_colours() {
        assert_eq!(Rgb::parse("#FFF"), Some(Rgb(255, 255, 255)));
        assert_eq!(
            Rgb::parse("#1a2B3c").map(Rgb::hex).as_deref(),
            Some("#1a2b3c")
        );
        assert_eq!(Rgb::parse("1a2b3c"), None);
        assert_eq!(Rgb::parse("#12345"), None);
        assert_eq!(Rgb::parse("#gg0000"), None);
        let ratio = Rgb(0, 0, 0).contrast(Rgb(255, 255, 255));
        assert!((ratio - 21.0).abs() < 1e-6);
    }

    #[test]
    fn derived_palettes_are_always_readable() {
        // Every background either derives a passing palette or is refused,
        // and every refused one is a mid tone.
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(51) {
                    let style = PageStyle {
                        background: Some(Rgb(r, g, b).hex()),
                        ..Default::default()
                    };
                    let palette = Palette::for_style(&style);
                    match palette.check() {
                        Ok(()) => {
                            assert!(palette.contrast_checks().iter().all(|c| c.passes()))
                        }
                        Err(e) => {
                            let best = Rgb(r, g, b)
                                .contrast(store::INK)
                                .max(Rgb(r, g, b).contrast(store::DARK_INK));
                            assert!(best < 6.0, "{} refused: {e}", Rgb(r, g, b).hex());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn light_backgrounds_get_dark_text_and_a_darker_green() {
        let style = PageStyle {
            background: Some("#f4ecd8".into()),
            ..Default::default()
        };
        let p = Palette::for_style(&style);
        assert!(p.light);
        assert_eq!(p.ink, store::DARK_INK);
        assert!(p.check().is_ok(), "{:?}", p.check());
    }

    #[test]
    fn refuses_an_accent_that_disappears() {
        let style = PageStyle {
            background: Some("#101010".into()),
            accent: Some("#202020".into()),
            ..Default::default()
        };
        let err = style.check(&[]).unwrap_err();
        assert_eq!(err.field, "accent");
        let ok = PageStyle {
            background: Some("#101010".into()),
            accent: Some("#ffb000".into()),
            ..Default::default()
        };
        assert!(ok.check(&[]).is_ok());
    }

    #[test]
    fn refuses_mid_grey_backgrounds() {
        let style = PageStyle {
            background: Some("#777777".into()),
            ..Default::default()
        };
        assert_eq!(style.check(&[]).unwrap_err().field, "background");
    }

    #[test]
    fn checks_media_references_and_blocks() {
        let art = media(MediaKind::Artwork);
        let shot = media(MediaKind::Screenshot);
        let capsule = media(MediaKind::Capsule);
        let all = [art.clone(), shot.clone(), capsule.clone()];

        let ok = PageStyle {
            hero: Some(shot.id),
            logo: Some(art.id),
            blocks: vec![
                PageBlock::Trailer {
                    url: "https://youtu.be/dQw4w9WgXcQ".into(),
                },
                PageBlock::About,
                PageBlock::Image {
                    media: art.id,
                    caption: "The lighthouse".into(),
                },
                PageBlock::Features {
                    heading: "Features".into(),
                    items: vec!["Hand-drawn".into()],
                },
            ],
            ..Default::default()
        };
        assert!(ok.check(&all).is_ok());

        let logo_shot = PageStyle {
            logo: Some(shot.id),
            ..Default::default()
        };
        assert_eq!(logo_shot.check(&all).unwrap_err().field, "logo");

        let unknown = PageStyle {
            hero: Some(Uuid::from_u128(u128::MAX)),
            ..Default::default()
        };
        assert_eq!(unknown.check(&all).unwrap_err().field, "hero");

        let twice = PageStyle {
            blocks: vec![PageBlock::About, PageBlock::About],
            ..Default::default()
        };
        assert_eq!(twice.check(&all).unwrap_err().field, "blocks");

        let capsule_image = PageStyle {
            blocks: vec![PageBlock::Image {
                media: capsule.id,
                caption: String::new(),
            }],
            ..Default::default()
        };
        assert_eq!(
            capsule_image.check(&all).unwrap_err().field,
            "blocks[0].media"
        );

        let empty_text = PageStyle {
            blocks: vec![PageBlock::Text {
                heading: "Story".into(),
                body: " ".into(),
            }],
            ..Default::default()
        };
        assert_eq!(empty_text.check(&all).unwrap_err().field, "blocks[0].body");
    }

    #[test]
    fn forgets_deleted_media() {
        let id = Uuid::from_u128(7);
        let mut style = PageStyle {
            hero: Some(id),
            logo: Some(id),
            blocks: vec![
                PageBlock::About,
                PageBlock::Image {
                    media: id,
                    caption: String::new(),
                },
            ],
            ..Default::default()
        };
        style.forget_media(id);
        assert_eq!(style.hero, None);
        assert_eq!(style.logo, None);
        assert_eq!(style.blocks, vec![PageBlock::About]);
    }

    #[test]
    fn embeds_only_youtube_and_vimeo() {
        let yt = Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ".to_string());
        assert_eq!(
            trailer_embed("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            yt
        );
        assert_eq!(
            trailer_embed("https://youtube.com/watch?feature=share&v=dQw4w9WgXcQ"),
            yt
        );
        assert_eq!(trailer_embed("https://youtu.be/dQw4w9WgXcQ"), yt);
        assert_eq!(
            trailer_embed("https://m.youtube.com/shorts/dQw4w9WgXcQ"),
            yt
        );
        assert_eq!(
            trailer_embed("https://vimeo.com/76979871").as_deref(),
            Some("https://player.vimeo.com/video/76979871?dnt=1")
        );
        assert_eq!(
            trailer_embed("https://youtu.be/dQw4w9WgXc\"><script>"),
            None
        );
        assert_eq!(
            trailer_embed("https://evil.example/watch?v=dQw4w9WgXcQ"),
            None
        );
        assert_eq!(trailer_embed("javascript:alert(1)"), None);
        assert_eq!(trailer_embed("https://vimeo.com/abc"), None);
    }

    #[test]
    fn serializes_blocks_with_a_type_tag() {
        let style = PageStyle {
            accent: Some("#ffb000".into()),
            blocks: vec![
                PageBlock::About,
                PageBlock::Quote {
                    text: "Lovely".into(),
                    source: "A player".into(),
                },
            ],
            ..Default::default()
        };
        let json = serde_json::to_value(&style).unwrap();
        assert_eq!(json["blocks"][0]["type"], "about");
        assert_eq!(json["blocks"][1]["type"], "quote");
        assert_eq!(json["title_style"], "standard");
        let back: PageStyle = serde_json::from_value(json).unwrap();
        assert_eq!(back, style);
        let empty: PageStyle = serde_json::from_str("{}").unwrap();
        assert_eq!(empty, PageStyle::default());
    }
}

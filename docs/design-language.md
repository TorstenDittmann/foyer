# Foyer design language: Midnight

Foyer is dark, modern and plain. The interface stays quiet so the games' own
art is the colour on screen. It is compact, so a browse grid or a desktop
library fits a lot of games at once. Supporting a developer is the one thing
that stands out in colour on every page.

The tokens live in two places, kept identical:

- Website: the `@theme` block in `crates/foyer-web/style/tailwind.css`
- Desktop client: `crates/foyer-desktop/src/theme.rs`

Components use only the semantic names below, never raw colours. To change
the look, change the values in both files.

## Colour

| Token | Value | Use |
| --- | --- | --- |
| `canvas` | `#0b0f1a` | Page and window background |
| `surface` | `#121829` | Cards, panels, sidebars |
| `raised` | `#1a2238` | Hover, selected rows, tags |
| `line` | `#222b42` | Borders and dividers |
| `ink` | `#e8ecf6` | Main text |
| `muted` | `#8590a8` | Secondary text, metadata |
| `faint` | `#6b7690` | Hints, placeholders, disabled |
| `accent` | `#e8ecf6` | Primary buttons (Download, Play), focus ring |
| `accent-ink` | `#0b0f1a` | Text on `accent` |
| `support` | `#ff6b5a` | Support links (Ko-fi, Patreon), download progress |
| `support-ink` | `#1d0805` | Text on `support` |
| `good` / `warn` / `bad` / `info` | `#3ecf8e` / `#f5b942` / `#ef4565` / `#6ea8ff` | Status only: review state, errors, notices |

Rules of thumb:

- Primary actions are light, not coloured. A page has at most one.
- Coral means "support the developer" (and progress in the client). Don't
  use it for anything else, so it keeps that meaning.
- Status colours appear only on small elements (a pill, an icon, a line of
  text), never as large fills.

## Type

- **Onest** for everything: headings, UI and body. Headings use weight 600
  with slightly tight tracking (`tracking-tight`); body is 400, labels 500.
- **IBM Plex Mono** for versions, file and patch sizes, build hashes and other
  numbers that line up.
- Base text is 14px on the web and in the client; buttons are 13px.

Fonts are self-hosted (`crates/foyer-web/public/fonts`) and bundled in the
client (`crates/foyer-desktop/assets/fonts`). Both are under the SIL Open Font
License; the licence files sit next to the fonts.

## Shape and density

- Corners: 6px for cards (`rounded-card`), 4px for buttons and inputs
  (`rounded-control`), 4px for cover art.
- Borders: 1px `line`. No shadows.
- Spacing is compact: 12px gaps in grids, 8px between buttons, buttons at
  14px horizontal and 8px vertical padding.

## Voice

Plain and short. Say what happened and what to do next.

- Empty library: "No games installed. Browse to add one."
- After install: "Installed: Moth & Lantern."
- Rejected submission: "Not accepted yet. See the review notes for what to fix."

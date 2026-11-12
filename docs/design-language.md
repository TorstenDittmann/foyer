# Foyer design language: Store

Foyer looks like a modern game storefront. The interface is neutral grey so
the games' own art carries the colour, the way it does on Steam, Epic or GOG.
Pages are built from familiar store pieces: a featured banner, rows of game
capsules, review labels, a screenshot viewer and a download box. Green means
"get it" everywhere: Download, Install, Play.

The tokens live in two places, kept identical:

- Website: the `@theme` block in `crates/foyer-web/style/tailwind.css`
- Desktop client: `crates/foyer-desktop/src/theme.rs`

Components use only the semantic names below, never raw colours. To change
the look, change the values in both files.

## Colour

| Token | Value | Use |
| --- | --- | --- |
| `canvas` | `#121214` | Page and window background |
| `chrome` | `#18181b` | Site header, client sidebar |
| `surface` | `#1e1e22` | Cards, the download box, panels |
| `raised` | `#2a2a2e` | Secondary buttons, tags, selected rows, hover |
| `line` | `#2e2e33` | Dividers (use sparingly; prefer spacing) |
| `ink` | `#f0f0f2` | Main text |
| `muted` | `#a6a6ad` | Secondary text, metadata |
| `faint` | `#8a8a92` | Hints, placeholders, small caps labels |
| `accent` | `#38c172` | Download, Install, Play; download progress; focus ring |
| `accent-ink` | `#0c1f13` | Text on `accent` |
| `support` | `#ff6b5a` | Tips and support links (Ko-fi, Patreon) |
| `support-ink` | `#1d0805` | Text on `support` |
| `info` | `#6fb8ff` | Review summaries ("Very positive"), links to developers |
| `good` / `warn` / `bad` | `#38c172` / `#f5b942` / `#ef4565` | Status only: review state, errors, notices |

Rules of thumb:

- Green is for getting a game. A page has one green button at most.
- Other actions are `raised` grey buttons. A white button is allowed on top of
  game art, where grey would disappear (the featured banner).
- Coral means "support the developer". Don't use it for anything else.
- Never fill large areas with colour. Art is the colour.

## Type

- **Barlow** for UI and body text: 400 body, 500 nav, 600 labels and buttons,
  700 for emphasis. Body text is 15px on the web.
- **Barlow Semi Condensed** (`font-display`) for page titles and game titles,
  weight 700. Big promotional titles on art can be uppercase.
- **IBM Plex Mono** for versions, file and patch sizes and build hashes.
- Small labels above values ("RELEASED", "DEVELOPER") are 11px uppercase
  with wide tracking in `faint`.

Fonts are self-hosted (`crates/foyer-web/public/fonts`) and bundled in the
client (`crates/foyer-desktop/assets/fonts`). Both are under the SIL Open Font
License; the licence files sit next to the fonts.

## Shape and density

- Corners: 10px for cards, banners and screenshots (`rounded-card`), 6px for
  buttons, inputs and tags (`rounded-control`).
- No borders on cards; separate them with `surface` against `canvas`.
  No shadows.
- Game art: 16:9 capsules in rows and the screenshot viewer, 3:4 covers in
  short lists and the client sidebar.
- Review labels follow the store convention: "Very positive (312)" in `info`.

## Voice

Plain and short, like a store label. Say what happened and what to do next.

- Empty library: "No games installed. Browse the store to add one."
- After install: "Installed: Moth & Lantern."
- Rejected submission: "Not accepted yet. See the review notes for what to fix."

## Developer page designs

A developer can make their game's page look like their game from the
dashboard's "Page design" tab. The design is stored as `PageStyle` on the game
(`crates/foyer-types/src/page.rs`) and rendered on the server.

What they can set:

- A **background** colour and an **accent** colour. Only these two are
  chosen; `Palette::for_style` derives every other token from them (panels
  step towards the text colour like the Store greys do; text is light or dark,
  whichever reads better). On the website the result overrides the tokens as
  CSS variables on the page's wrapper, so every component picks it up.
- A **hero** image across the top and a **logo** that replaces the title text
  (the title stays as its alt text), or a big uppercase title.
- The **layout** of the main column: screenshots, about, text sections,
  feature lists, quotes, images with captions and YouTube/Vimeo trailers, in
  any order. The side column (download box, support, details) stays put so
  every page works the same way.

Guardrails, so pages stay readable and recognisably Foyer:

- Every derived palette meets WCAG 2.2 AA: 4.5:1 for text, including hints on
  panels and links, and 3:1 for the accent against the background. Text on
  the accent is chosen to reach 4.5:1. Colours that can't (mid-grey
  backgrounds, an accent that disappears into the background) are refused on
  save with the ratio and what to change. The design tab shows each ratio.
- Without a custom accent, Foyer's green is darkened or lightened until it
  stands out from a custom background.
- The site header and footer keep the Store look; only the page body changes.
- Trailers embed from `youtube-nocookie.com` or Vimeo with `dnt=1` only.
- The desktop client uses the hero, logo, title style, layout and accent (on
  the Install button and highlights). It keeps its own background, so the
  window stays consistent.

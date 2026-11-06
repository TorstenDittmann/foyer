# Foyer

Foyer is a curated showcase for indie games. It is not a store: games are free
to download, and each page links to the developer's Patreon, Ko-fi or similar
so players can support them directly.

Every game is reviewed before it is listed. Foyer does not accept NSFW games,
asset flips or spam, or "games" with no real gameplay (click-through text or
images).

## Repository layout

Everything is Rust and lives in one Cargo workspace, so types and logic can be
shared between the server, the website, the CLI and the desktop client.

| Path | What it is |
| --- | --- |
| `crates/foyer-api` | HTTP API: Axum on libSQL (local SQLite file or a remote sqld/Turso database). |
| `crates/foyer-web` | Website: Leptos with server-side rendering and hydration, styled with Tailwind. |
| `crates/foyer-cli` | `foyer` command for developers: create build manifests and diff builds. |
| `crates/foyer-desktop` | Desktop client built with GPUI: browse, install and update games. |
| `crates/foyer-patch` | Patch engine shared by the CLI and the desktop client: hashes builds into manifests and plans updates between them. |

Build files will be stored in any S3-compatible object storage.

## Getting started

Dev tooling is managed with [mise](https://mise.jdx.dev). It installs the
pinned Rust toolchain (with the `wasm32-unknown-unknown` target) and
`cargo-leptos`:

```sh
mise install
mise tasks          # list everything below
mise run dev        # website with live reload on http://127.0.0.1:3000
mise run api        # API on http://127.0.0.1:8080
mise run lint       # fmt check + clippy for every crate and build target
mise run test
mise run ci         # what CI runs on Linux
```

Without mise you need `rustup` (it reads `rust-toolchain.toml`) and
`cargo install cargo-leptos --locked`. Plain `cargo build` builds the API, CLI,
patch engine and web crate; the desktop client needs system libraries (see
below).

### API

```sh
mise run api
curl http://127.0.0.1:8080/health
```

| Variable | Default | Meaning |
| --- | --- | --- |
| `FOYER_BIND` | `127.0.0.1:8080` | Address to listen on. |
| `FOYER_DATABASE_URL` | `foyer.db` | A file path for local SQLite, or a `libsql://` / `https://` URL for a remote database. |
| `FOYER_DATABASE_AUTH_TOKEN` | | Auth token for a remote database. |
| `RUST_LOG` | `foyer_api=info,tower_http=info` | Log filter. |

Schema migrations live in `crates/foyer-api/src/db.rs` and run on startup.

### Website

The website is built with [cargo-leptos](https://github.com/leptos-rs/cargo-leptos),
which compiles the server, compiles the browser bundle to WebAssembly and runs
Tailwind.

```sh
mise run dev         # cargo leptos watch
```

Pages: discover (`/`), browse and search (`/games`), game pages, developer
profiles, the content guidelines, sign in, and the developer dashboard
(`/dashboard`) for creating game pages and submitting them for review.

Until the API serves these endpoints, the site reads an in-memory mock
(`crates/foyer-web/src/mock.rs`) behind the server functions in `src/api.rs`.
Sign in with `demo@foyer.play` / `foyer-demo` to see a developer account with
games in every review state. Styling uses only the semantic tokens defined in
`style/tailwind.css`, so a design direction can be applied there.

`mise run build:web` (`cargo leptos build --release`) produces the server binary in
`target/release/foyer-web` and static assets in `target/site`.

### CLI

```sh
cargo run -p foyer-cli -- manifest path/to/build -o manifest.json
cargo run -p foyer-cli -- diff old-build/ new-build/
```

### Desktop client

The desktop client is not a default workspace member because GPUI needs system
libraries. On Debian/Ubuntu:

```sh
sudo apt-get install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev libvulkan-dev libfontconfig-dev libxcb1-dev
mise run desktop
```

On macOS you need Xcode (for Metal). See the
[GPUI docs](https://www.gpui.rs/) for other platforms.

## CI

GitHub Actions sets up the same tools with mise and runs `mise run lint`,
`mise run test` and `mise run build:web` on Linux, plus a macOS build of the
desktop client.

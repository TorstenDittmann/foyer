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
| `crates/foyer-cli` | `foyer` command for developers: log in, push builds to channels with delta uploads, check status. |
| `crates/foyer-desktop` | Desktop client built with GPUI: browse, install and update games. |
| `crates/foyer-types` | Wire types shared by the API, website, CLI and desktop client (wasm-friendly). |
| `crates/foyer-patch` | Patch engine shared by the API, CLI and desktop client: chunks and hashes builds into manifests and plans which chunks to move. |

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
| `FOYER_ADMIN_EMAILS` | | Comma-separated emails that become admins when they register. |
| `FOYER_S3_BUCKET` | | Bucket for builds and images. Without it, upload endpoints answer 503. |
| `FOYER_S3_ENDPOINT` | | S3-compatible endpoint, e.g. `https://<account>.r2.cloudflarestorage.com`. |
| `FOYER_S3_REGION` | `auto` | Region to sign for. |
| `FOYER_S3_ACCESS_KEY_ID`, `FOYER_S3_SECRET_ACCESS_KEY` | | Credentials used to presign URLs and check uploads. |
| `FOYER_S3_VIRTUAL_HOSTED` | `false` | `true` for `https://<bucket>.<endpoint>` URLs instead of path-style. |
| `FOYER_MEDIA_BASE_URL` | bucket URL | Public URL (CDN) that serves the bucket, used for game images. |
| `RUST_LOG` | `foyer_api=info,tower_http=info` | Log filter. |

Schema migrations live in `crates/foyer-api/src/migrations/` and run on
startup.

Every endpoint lives under `/v1` and is described by the OpenAPI document at
`GET /openapi.json`. A copy is checked in at `crates/foyer-api/openapi.json`;
tests fail when it drifts, and
`FOYER_UPDATE_OPENAPI=1 cargo test -p foyer-api --test openapi` regenerates it.
Clients authenticate with `Authorization: Bearer <token>`, using a session
token from `POST /v1/auth/login` or an API token from `POST /v1/auth/tokens`.

Builds never pass through the API: the client sends a manifest, uploads the
missing chunks straight to storage with presigned URLs, then finalizes. Games
are listed only after a reviewer approves a submission.

### Website

The website is built with [cargo-leptos](https://github.com/leptos-rs/cargo-leptos),
which compiles the server, compiles the browser bundle to WebAssembly and runs
Tailwind.

```sh
mise run dev         # cargo leptos watch
```

`mise run build:web` (`cargo leptos build --release`) produces the server binary in
`target/release/foyer-web` and static assets in `target/site`.

### CLI

`foyer` uploads builds. It splits every file into content-defined chunks
(FastCDC, addressed by BLAKE3 hash), asks the API which chunks storage is
missing, and uploads only those straight to S3 through presigned URLs. A patch
to one asset re-uploads a few MiB instead of the whole game, and re-running an
interrupted push resumes it.

```sh
cargo run -p foyer-cli -- login                    # paste an API token from the dashboard
cargo run -p foyer-cli -- push build/linux --game my-game --channel beta --platform linux -v 1.2.0
cargo run -p foyer-cli -- status my-game           # latest build per channel and platform
cargo run -p foyer-cli -- push build/linux -g my-game -p linux --dry-run   # scan only
cargo run -p foyer-cli -- manifest path/to/build -o manifest.json
cargo run -p foyer-cli -- diff old-build/ new-build/
```

| Variable | Meaning |
| --- | --- |
| `FOYER_TOKEN` | API token; overrides the one saved by `login` (use this in CI). |
| `FOYER_API_URL` | API base URL, default `https://api.foyer.play`. |
| `FOYER_CONFIG_DIR` | Where `credentials.json` is kept, default the OS config dir + `foyer`. |

`push --json` prints the resulting build as JSON for scripts. The CLI tests in
`crates/foyer-cli/tests` run the real binary against an in-process mock of the
API and storage.

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

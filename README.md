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
| `crates/foyer-desktop` | Desktop client built with GPUI: browse, install, update and launch games. No account needed. |
| `crates/foyer-install` | The desktop client's engine: anonymous API client, delta installs and updates with pause/resume, verify/repair, uninstall, launching. No UI code, so it is tested on its own. |
| `crates/foyer-types` | Wire types shared by the API, website, CLI and desktop client (wasm-friendly). |
| `crates/foyer-patch` | Patch engine shared by the API, CLI and desktop client: chunks and hashes builds into manifests and plans which chunks to move. |
| `install/` | Install scripts for the CLI, also served by the website at `/install.sh` and `/install.ps1`. |
| `action.yml` | GitHub Action that pushes a build with the CLI. |

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
mise run worker     # background jobs: malware scans, zips, email
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
| `FOYER_ADMIN_EMAILS` | | Comma-separated emails that become admins once they confirm the address. |
| `FOYER_S3_BUCKET` | | Bucket for builds and images. Without it, upload endpoints answer 503. |
| `FOYER_S3_ENDPOINT` | | S3-compatible endpoint, e.g. `https://<account>.r2.cloudflarestorage.com`. |
| `FOYER_S3_REGION` | `auto` | Region to sign for. |
| `FOYER_S3_ACCESS_KEY_ID`, `FOYER_S3_SECRET_ACCESS_KEY` | | Credentials used to presign URLs and check uploads. |
| `FOYER_S3_VIRTUAL_HOSTED` | `false` | `true` for `https://<bucket>.<endpoint>` URLs instead of path-style. |
| `FOYER_MEDIA_BASE_URL` | bucket URL | Public URL (CDN) that serves the bucket, used for game images. |
| `FOYER_PUBLIC_WEB_URL` | `http://localhost:3000` | The website's public address, for links in emails, the Discord redirect (`<url>/auth/discord/callback`) and passkeys (its host is the WebAuthn relying party; passkeys need a domain name, not an IP address). |
| `FOYER_DISCORD_CLIENT_ID`, `FOYER_DISCORD_CLIENT_SECRET` | | Turn on "Continue with Discord". Without them the endpoints answer 503. |
| `FOYER_SMTP_URL` | | Worker: SMTP server, e.g. `smtps://user:pass@smtp.example.com` or `smtp://host:587?tls=required`. Without it emails are only logged. |
| `FOYER_MAIL_FROM` | `Foyer <noreply@foyer.lol>` | Worker: sender address. |
| `FOYER_CLAMD_ADDR` | | Worker: clamd `host:port` for malware scans. Without it builds are published with scan status `skipped`. |
| `FOYER_RATE_LIMITS` | `on` | `off` turns off the per-IP limits on sign-in (10/min), uploads (120/min), downloads (300/min) and player reports (2/min, bursts of 5). |
| `FOYER_PROXY_HOPS` | `0` | Reverse proxies in front of the API; the client IP is that many entries from the right of `X-Forwarded-For`. |
| `FOYER_INTERNAL_KEY` | | Shared with the website, which sends it with `X-Foyer-Client-IP` so limits apply per visitor. |
| `RUST_LOG` | `foyer_api=info,tower_http=info` | Log filter. |

Schema migrations live in `crates/foyer-api/src/migrations/` and run on
startup.

`foyer-api worker` runs the background jobs from the same binary and
configuration. Jobs live in the `jobs` table, so any number of workers can run
next to any number of API replicas. When a build is finalized the worker
downloads it, checks every chunk's hash, scans each file with ClamAV, packs one
zip for browser downloads and only then makes the build the channel's current
one. A build with malware is blocked and shows up in the review queue; a scan
that couldn't cover every file (clamd's size limit) is published but flagged
there too. The worker needs disk space in `TMPDIR` for the unpacked build plus
its zip. It also sends verification and password reset emails, and release
emails (below).

Every endpoint lives under `/v1` and is described by the OpenAPI document at
`GET /openapi.json`. A copy is checked in at `crates/foyer-api/openapi.json`;
tests fail when it drifts, and
`FOYER_UPDATE_OPENAPI=1 cargo test -p foyer-api --test openapi` regenerates it.
Clients authenticate with `Authorization: Bearer <token>`, using a session
token from `POST /v1/auth/login` or an API token from `POST /v1/auth/tokens`.

Builds never pass through the API: the client sends a manifest, uploads the
missing chunks straight to storage with presigned URLs, then finalizes. Games
are listed only after a reviewer approves a submission.

Tester keys (`fyk_...`, from `POST /v1/games/{slug}/tester-keys`) unlock a
game before it is published, or its private channels after, without an
account. Clients send the keys they hold in `X-Foyer-Key` (comma separated)
and every player endpoint serves what they cover; `POST
/v1/tester-keys/redeem` says which game a key is for. A channel goes private
with `PATCH /v1/games/{slug}/channels/{channel}` or `private: true` on a
push. Keys expire (30 days by default, a year at most), stop working when a
game is rejected or delisted, and only a hash is stored.

Players need no account. `GET /v1/games/{slug}/downloads` lists the zips,
`POST /v1/games/{slug}/channels/{channel}/archive` returns a short-lived link
to one, and the desktop client installs from `POST .../download-urls` and asks
`POST /v1/updates` which installed games have a newer build. Downloads are
counted per game, platform, day and source (web, client install, client
update); `GET /v1/games/{slug}/stats` reads them. Search uses SQLite FTS5.

Accounts are optional for players and add:

- **Wishlist**: `PUT`/`DELETE /v1/me/wishlist/{slug}`, `GET /v1/me/wishlist`.
- **Following developers**: `PUT`/`DELETE /v1/me/following/{slug}`,
  `GET /v1/me/following`.
- **Release emails**, off until the player turns them on with
  `PATCH /v1/me/settings`. When a game is approved for the first time, the
  worker emails each follower of its developer who opted in and confirmed
  their address, once (`release_notices` keeps track). Each email has an
  unsubscribe link (`POST /v1/unsubscribe`) that works without signing in.
- **Passkeys**, for any account: `POST /v1/auth/passkeys/register/{start,finish}`
  adds one, `POST /v1/auth/passkeys/login/{start,finish}` signs in without a
  username or password (discoverable credentials, user verification
  required). WebAuthn is handled by [webauthn-rs](https://github.com/kanidm/webauthn-rs);
  the challenge state stays in the database, so any API replica can finish a
  ceremony another one started. Passkeys are bound to the host of
  `FOYER_PUBLIC_WEB_URL`; with an IP address there they are off (503).

Moderation after launch is human-only too. Anyone can report a published
game (`POST /v1/games/{slug}/reports`, no account needed, rate limited per
address, one open report per reporter and game). Reports wait in a staff
queue (`GET /v1/review/reports`) until a reviewer dismisses them or delists
the game; nothing is hidden automatically. A developer can appeal a rejection
or delisting once (`POST /v1/games/{slug}/appeals`); a different reviewer
upholds it, overturns it (the game is published) or reopens the game for
changes. Strikes are a per-developer record of violations, issued with a
rejection or delisting or on their own, and revoked when an appeal succeeds.
They inform reviewers and don't ban anyone by themselves.

### Website

The website is built with [cargo-leptos](https://github.com/leptos-rs/cargo-leptos),
which compiles the server, compiles the browser bundle to WebAssembly and runs
Tailwind.

```sh
mise run dev         # cargo leptos watch
```

Pages: discover (`/`), browse and search (`/games`), game pages with
download buttons, developer profiles, the content guidelines, sign in (email
and password, passkey or Discord), email confirmation and password reset, the
player's account (`/account`: wishlist, followed developers, release emails,
passkeys), the unsubscribe page for release emails, the
developer dashboard (`/dashboard`) for creating game pages and submitting them
for review, its API tokens page (`/dashboard/tokens`), each game's downloads
(`/dashboard/games/{slug}/downloads`) and testers
(`/dashboard/games/{slug}/testers`: private channels and tester keys), appeals
and strikes, a "Report this game" form on every game page, and the staff
review screen (`/review`) with tabs for submissions, player reports and
appeals, and per game its builds, malware scan results, downloads, reports,
delisting and the developer's strikes.

A tester link (`/tester?key=...`) checks the key, keeps it in the
`foyer_keys` cookie and opens the game; the site then sends the visitor's
keys with every API call, so unpublished games and private channels show up
for them with a "private preview" note.

A download button points at `/download/{game}/{channel}` on the website,
which asks the API for a fresh link and redirects to it.

Passkey buttons are handled by a small script, `public/passkey.js`, which
calls the browser's WebAuthn API and relays each step through the website's
`/auth/passkey/...` routes to the API. Use `http://localhost:3000` locally
(not `127.0.0.1`): WebAuthn needs a domain name.

The site talks to the API from its server (never from the browser), using the
`foyer-types` crate for every request and response. Run `mise run api` next
to `mise run dev`. The visitor's API session token lives in an `HttpOnly`
cookie.

| Variable | Default | Meaning |
| --- | --- | --- |
| `FOYER_API_URL` | `http://127.0.0.1:8080` | Where the API is. |
| `FOYER_INTERNAL_KEY` | | Same value as the API's. Lets the API rate limit each visitor rather than the website. |
| `FOYER_PROXY_HOPS` | `0` | Reverse proxies in front of the website, to find the visitor's IP in `X-Forwarded-For`. |
| `FOYER_WEB_MOCK` | | Set to `1` to use in-memory sample data instead of the API (`src/mock.rs`). Sign in with `demo@foyer.lol` / `foyer-demo` to see a developer account with games in every review state. |

Styling uses only the semantic tokens defined in `style/tailwind.css`, so a
design direction can be applied there.

`mise run build:web` (`cargo leptos build --release`) produces the server binary in
`target/release/foyer-web` and static assets in `target/site`.

### CLI

`foyer` uploads builds. It splits every file into content-defined chunks
(FastCDC, addressed by BLAKE3 hash), asks the API which chunks storage is
missing, and uploads only those straight to S3 through presigned URLs. A patch
to one asset re-uploads a few MiB instead of the whole game, and re-running an
interrupted push resumes it.

Install it from a release (Linux and macOS; on Windows run
`irm https://foyer.lol/install.ps1 | iex`):

```sh
curl -fsSL https://foyer.lol/install.sh | sh
```

Releases are cut by pushing a `cli-v<version>` tag matching the workspace
version; `.github/workflows/release-cli.yml` builds static Linux (x86_64,
arm64), macOS (Intel, Apple silicon) and Windows binaries and publishes them
with a `SHA256SUMS` file the install scripts check.

```sh
cargo run -p foyer-cli -- login                    # paste an API token from the dashboard
cargo run -p foyer-cli -- push build/linux --game my-game --channel beta --platform linux -v 1.2.0
cargo run -p foyer-cli -- status my-game           # latest build per channel and platform
cargo run -p foyer-cli -- push build/linux -g my-game -p linux --dry-run   # scan only
cargo run -p foyer-cli -- manifest path/to/build -o manifest.json
cargo run -p foyer-cli -- diff old-build/ new-build/
cargo run -p foyer-cli -- push build/linux -g my-game -p linux -c beta --private --wait
cargo run -p foyer-cli -- channel my-game beta --public
cargo run -p foyer-cli -- keys create my-game -l "Playtest" -c beta   # prints the key and a link
cargo run -p foyer-cli -- keys list my-game
cargo run -p foyer-cli -- keys revoke my-game <id>
```

`push --wait` polls until the worker has scanned the build and it is live,
and exits non-zero if it failed or was rejected, so CI notices.

#### GitHub Action

`action.yml` installs the CLI and pushes a build:

```yaml
- uses: TorstenDittmann/foyer@cli-v0.1.0
  with:
    token: ${{ secrets.FOYER_TOKEN }}   # an API token from /dashboard/tokens
    game: my-game
    platform: windows
    path: build/windows
    channel: beta
    version: ${{ github.ref_name }}
```

It also takes `launch`, `launch-args` (one per line), `private`, `wait`
(default `true`), `wait-timeout`, `cli-version` and `api-url`, and outputs
`build-id`, `state` and `build` (JSON). Used at a `cli-v*` tag it runs the
CLI from the same release.

| Variable | Meaning |
| --- | --- |
| `FOYER_TOKEN` | API token; overrides the one saved by `login` (use this in CI). |
| `FOYER_API_URL` | API base URL, default `https://api.foyer.lol`. |
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

The client never logs in. It browses the catalog, installs a game's build for
the current platform (Windows, Linux, macOS) and keeps it updated using only
public endpoints:

- **Install and update** download only the chunks that are not on disk yet:
  an update to one asset fetches a few MiB, reusing everything unchanged from
  the installed files. Files are staged and swapped in at the end, so the old
  version keeps working while an update downloads, and files a game writes
  itself (saves, settings) are never touched.
- **Pause and resume** keep every chunk already downloaded, also across
  restarts. Expired links are refreshed and failed chunks retried.
- **Verify files** re-hashes an install and re-downloads anything damaged.
- **Updates** are checked at startup and every 30 minutes (optionally
  installed automatically) with `POST /v1/updates`, falling back to each
  game's channels.
- **Play** starts the executable picked from the build (the game's `.exe` on
  Windows; ELF binaries, `.x86_64`, `.sh` on Linux), which the player can
  change.

| Variable | Default | Meaning |
| --- | --- | --- |
| `FOYER_API_URL` | `https://api.foyer.lol` | API base URL. |
| `FOYER_DATA_DIR` | OS data dir + `Foyer` | Where `library.json` and the image cache live. |

Tester keys in `settings.tester_keys` of `library.json` are sent with every
API call (never to storage), so the games and private channels they unlock
install and update like public ones.

Games install into `~/Games/Foyer/<game>` by default; Settings changes the
folder for new installs. Each install keeps its state in a `.foyer-client`
folder inside the game folder.

## CI

GitHub Actions sets up the same tools with mise and runs `mise run lint`,
`mise run test` and `mise run build:web` on Linux, plus macOS and Windows builds of the
desktop client (Windows also runs the install engine tests). A second workflow
builds the container images and pushes them to GHCR.

## Deployment

Production runs on [orch](https://orch.torsten.work): the API, the website, the
job worker, ClamAV and a libSQL server as one application, with builds in an
S3-compatible bucket. The images come from the `Dockerfile` (targets `api` and
`web`) and `deploy/clamav/`. [`deploy/README.md`](deploy/README.md) covers the
`orch.pkl`, the secrets to set and how updates roll out.

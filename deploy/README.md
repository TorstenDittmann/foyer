# Deploying Foyer on orch

Foyer runs as one [orch](https://orch.torsten.work) application, `foyer`,
described in [`orch.pkl`](orch.pkl):

| Service | Image | What it does |
| --- | --- | --- |
| `web` | `ghcr.io/torstendittmann/foyer-web` | Website (Leptos SSR) on `foyer.play`, port 3000. |
| `api` | `ghcr.io/torstendittmann/foyer-api` | HTTP API on `api.foyer.play`, port 8080, health check `/health`. |
| `worker` | `ghcr.io/torstendittmann/foyer-api` | Job worker, the API image started as `foyer-api worker`: ClamAV scans and per-platform zips. |
| `clamav` | `ghcr.io/torstendittmann/foyer-clamav` | clamd on TCP 3310, only reachable inside the cluster. |
| `db` | `ghcr.io/tursodatabase/libsql-server` | libSQL server managed by orch, with nightly backups. |

Build files and images live in an S3-compatible bucket outside the cluster.

```
browser ──https──▶ web ──http (cluster)──▶ api ──▶ db (libSQL)
CLI / desktop ──https──▶ api                 │
                                             └──▶ S3 (presigned URLs; clients upload and download directly)
worker ──▶ db, S3, clamav:3310
```

The website talks to the API over the cluster network
(`http://api.foyer.svc.internal:8080`); browsers never call the API directly.

## Images

`.github/workflows/images.yml` builds the images:

- Pull requests that touch the server crates or the Dockerfiles only build them.
- Pushes to `main` push the `main` and `sha-<commit>` tags to GHCR.
- `v*` tags push versioned tags such as `1.2.0` and `1.2`.
- Every Monday it rebuilds `foyer-clamav` so the image ships recent signatures.
  clamd also updates its signatures every 12 hours while it runs.

The images are built for `linux/amd64`. For arm64 nodes, add `linux/arm64`
to `platforms` in the workflow (the Rust builds then run under emulation and
take much longer).

Build them locally with:

```sh
docker build --target api -t foyer-api .
docker build --target web -t foyer-web .
docker build -t foyer-clamav deploy/clamav
```

The Rust images run as an unprivileged user (uid 10001) with `tini` as PID 1.
The ClamAV image runs as the `clamav` user and needs no extra Linux
capabilities, so the application can be deployed with a deployer token or
from GitHub.

## One-time setup

You need the `orch` CLI with a target for the cluster (`orch target add`).
The commands below assume it is the default target; otherwise add
`--target NAME`.

### 1. DNS

Point A records for both hostnames at the cluster's ingress nodes (TCP 80 and
443 must be reachable for certificates):

- `foyer.play` → website
- `api.foyer.play` → API

To use other hostnames, change `webDomain` and `apiDomain` at the top of
`orch.pkl`. The CLI's default API URL is `https://api.foyer.play`; with
another API host, set `FOYER_API_URL` for the CLI.

### 2. Image access

GHCR makes new packages private. Either make `foyer-api`, `foyer-web` and
`foyer-clamav` public (GitHub → Packages → package settings → Change
visibility), or give the cluster read access once with a token that has
`read:packages`:

```sh
printf '%s' "$GHCR_READ_TOKEN" | orch registry login ghcr.io --username TorstenDittmann --password-stdin
```

The first images appear after the workflow has run on `main`.

### 3. Secrets

Set these before the first deploy. A missing secret blocks the rollout with
`secret_missing`. Use `printf '%s'`, not `echo`, so no newline ends up in the
value:

```sh
printf '%s' 'https://s3.example.com' | orch secret set foyer s3-endpoint --from-stdin
```

| Secret | Variable | Used by | Value |
| --- | --- | --- | --- |
| `s3-endpoint` | `FOYER_S3_ENDPOINT` | api, worker | S3 endpoint URL, e.g. `https://s3.example.com`. |
| `s3-bucket` | `FOYER_S3_BUCKET` | api, worker | Bucket name. |
| `s3-access-key-id` | `FOYER_S3_ACCESS_KEY_ID` | api, worker | Access key with read, write and HEAD on the bucket. |
| `s3-secret-access-key` | `FOYER_S3_SECRET_ACCESS_KEY` | api, worker | Its secret key. |
| `media-base-url` | `FOYER_MEDIA_BASE_URL` | api, worker | Public URL that serves the bucket's `games/*/media/*` objects (a CDN or the bucket's public URL), e.g. `https://media.foyer.play`. |
| `admin-emails` | `FOYER_ADMIN_EMAILS` | api, worker | Comma-separated emails that become admins when they register. |
| `smtp-url` | `FOYER_SMTP_URL` | api, worker | Mail server, e.g. `smtps://user:pass@smtp.example.com:465`. |
| `discord-client-id` | `FOYER_DISCORD_CLIENT_ID` | api | Discord application's client ID. Redirect URL: see the API docs once Discord sign-in lands. |
| `discord-client-secret` | `FOYER_DISCORD_CLIENT_SECRET` | api | Discord application's client secret. |
| `internal-key` | `FOYER_INTERNAL_KEY` | api, web | Shared secret so the website can pass visitors' IPs to the API. Generate one: `openssl rand -base64 32`. |

orch creates `db-jwt-key`, `db-token` and `db-url` for the libSQL database on
the first deploy; don't set them yourself.

`smtp-url`, the Discord pair and `internal-key` are read by code that is still
in review (email verification, Discord sign-in, rate limiting). Set them
anyway: orch only checks that they exist.

Clients upload and download build chunks straight from the bucket with
presigned URLs, so the S3 endpoint must be reachable from the internet. If the
store only supports virtual-hosted URLs
(`https://BUCKET.ENDPOINT`), set `FOYER_S3_VIRTUAL_HOSTED` to `"true"` in
`orch.pkl`; if it signs for a fixed region, change `FOYER_S3_REGION`.

### 4. Deploy

```sh
orch validate -f deploy/orch.pkl
orch deploy -f deploy/orch.pkl --wait
orch app status foyer
```

The first deploy starts `db`, then `api` (which runs the schema migrations),
then `clamav`, `worker` and `web`. clamd needs about a minute to load its
signatures.

## Updates

`deploy.autoUpdate` is on: every five minutes the controller checks whether
the `main` tags point at new digests and rolls them out with health checks and
automatic rollback. Merging to `main` is all a release needs.

Run `orch deploy -f deploy/orch.pkl --wait` yourself only after changing
`orch.pkl`. To roll back:

```sh
orch app history foyer
orch rollback foyer REVISION --wait
```

`orch rollback` pauses auto-updates until the next `orch deploy`.

To deploy from GitHub Actions instead, with history that links each commit,
run `orch github trust TorstenDittmann/foyer --app foyer` and add a job that
runs `orch deploy -f deploy/orch.pkl --wait` after the images are pushed (see
[Deploy from GitHub](https://orch.torsten.work/github/)). Then turn off
`autoUpdate`.

## Plain settings

These are set in `orch.pkl` as plain values:

| Variable | Service | Value |
| --- | --- | --- |
| `FOYER_BIND` | api, worker | `0.0.0.0:8080` (set in the image). |
| `FOYER_DATABASE_URL` | api, worker | `http://db.foyer.svc.internal:8080`, with the token from `db-token` in `FOYER_DATABASE_AUTH_TOKEN`. |
| `FOYER_S3_REGION` | api, worker | `auto`. |
| `FOYER_S3_VIRTUAL_HOSTED` | api, worker | `false` (path-style URLs). |
| `FOYER_PUBLIC_WEB_URL` | api, worker | `https://foyer.play`, for links in emails and the Discord redirect. |
| `FOYER_MAIL_FROM` | api, worker | `Foyer <noreply@foyer.play>`. |
| `FOYER_PROXY_HOPS` | api | `1`: orch's ingress is the only proxy in front of the API. |
| `FOYER_CLAMD_ADDR` | worker | `clamav.foyer.svc.internal:3310`. |
| `TMPDIR` | worker | `/tmp`, where builds are unpacked and zipped. |
| `FOYER_API_URL` | web | `http://api.foyer.svc.internal:8080`. |
| `LEPTOS_SITE_ADDR` | web | `0.0.0.0:3000` (set in the image). |
| `RUST_LOG` | api, worker | `foyer_api=info,tower_http=info`. |

## Resources

| Service | CPU | Memory |
| --- | --- | --- |
| api | 500m | 256Mi |
| worker | 1 | 1Gi |
| clamav | 500m | 2Gi |
| web | 250m | 256Mi |
| db | 250m | 256Mi |

That is about 2.5 CPUs and 3.75 GiB reserved in total. clamd holds roughly
1 GiB of signatures in memory and reloads them in place, which pauses scans
for a few seconds.

The worker unpacks a build and writes its zip in `/tmp` on the container's
own filesystem, so the node it runs on needs free disk for the largest build
plus its zip.

## Database

The libSQL database `db` runs on one node with a local volume, `db-data`.
orch backs it up every night at 03:00 UTC and keeps 14 backups:

```sh
orch db backups foyer db
orch db backup foyer db           # take one now
orch db restore foyer db BACKUP
```

Backups stay on the database's node unless the cluster has a backup
destination that copies them to S3.

To use a libSQL server outside the cluster instead (for example Turso),
remove the `databases` block and `dependsOn { "db" }`, set
`FOYER_DATABASE_URL` to its `libsql://` or `https://` URL, and point
`FOYER_DATABASE_AUTH_TOKEN` at a secret holding its token.

## Day-to-day

```sh
orch app status foyer
orch logs foyer api
orch logs foyer worker
orch tui
```

# syntax=docker/dockerfile:1.7
#
# Container images for the Foyer API and website. One file, two targets:
#
#   docker build --target api -t foyer-api .
#   docker build --target web -t foyer-web .
#
# The job worker runs from the API image as `foyer-api worker`. ClamAV runs as
# its own image, see deploy/clamav/Dockerfile.

ARG RUST_IMAGE=rust:1.97-slim-trixie
ARG RUNTIME_IMAGE=debian:trixie-slim

# Toolchain pinned by rust-toolchain.toml (with the wasm32 target), plus
# cargo-chef so dependencies are built in their own cached layer.
FROM ${RUST_IMAGE} AS toolchain
ARG CARGO_CHEF_VERSION=0.1.78
WORKDIR /src
COPY rust-toolchain.toml ./
RUN rustup toolchain install && rustc --version
RUN cargo install cargo-chef --locked --version "${CARGO_CHEF_VERSION}"

FROM toolchain AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ---------------------------------------------------------------- API build
FROM toolchain AS api-build
COPY --from=planner /src/recipe.json recipe.json
RUN cargo chef cook --release --locked -p foyer-api --recipe-path recipe.json
COPY . .
RUN cargo build --release --locked -p foyer-api

# ------------------------------------------------------------ website build
FROM toolchain AS web-build
ARG CARGO_LEPTOS_VERSION=0.3.11
ADD https://github.com/leptos-rs/cargo-leptos/releases/download/v${CARGO_LEPTOS_VERSION}/cargo-leptos-x86_64-unknown-linux-gnu.tar.gz /tmp/cargo-leptos.tar.gz
RUN tar -xzf /tmp/cargo-leptos.tar.gz -C /tmp \
 && install -m 0755 "$(find /tmp -type f -name cargo-leptos | head -n1)" /usr/local/bin/cargo-leptos \
 && rm -rf /tmp/cargo-leptos* \
 && cargo-leptos --version
COPY --from=planner /src/recipe.json recipe.json
# Same profiles, features and target dirs that cargo-leptos uses (see
# [package.metadata.leptos]); it builds the browser bundle in target/front.
RUN cargo chef cook --release --locked -p foyer-web --no-default-features --features ssr \
      --recipe-path recipe.json \
 && cargo chef cook --profile wasm-release --locked -p foyer-web --no-default-features --features hydrate \
      --target wasm32-unknown-unknown --target-dir target/front --recipe-path recipe.json
COPY . .
# --precompress writes .br and .gz copies of the bundle, which the server
# sends as they are instead of compressing on every request.
RUN cargo leptos build --release --precompress

# ------------------------------------------------------------- runtime base
FROM ${RUNTIME_IMAGE} AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --user-group --create-home --home-dir /app --shell /usr/sbin/nologin foyer
WORKDIR /app
USER 10001:10001
# tini is PID 1: it forwards SIGTERM to the server and reaps child processes.
ENTRYPOINT ["/usr/bin/tini", "--"]

# ---------------------------------------------------------------- API image
FROM runtime AS api
LABEL org.opencontainers.image.title="foyer-api" \
      org.opencontainers.image.description="Foyer HTTP API and job worker (foyer-api worker)" \
      org.opencontainers.image.source="https://github.com/TorstenDittmann/foyer"
COPY --from=api-build /src/target/release/foyer-api /usr/local/bin/foyer-api
ENV FOYER_BIND=0.0.0.0:8080 \
    RUST_LOG=foyer_api=info,tower_http=info \
    NO_COLOR=1
EXPOSE 8080
ENTRYPOINT ["/usr/bin/tini", "--", "foyer-api"]

# ------------------------------------------------------------ website image
FROM runtime AS web
LABEL org.opencontainers.image.title="foyer-web" \
      org.opencontainers.image.description="Foyer website (Leptos SSR)" \
      org.opencontainers.image.source="https://github.com/TorstenDittmann/foyer"
COPY --from=web-build /src/target/release/foyer-web /usr/local/bin/foyer-web
# The bundle's file hashes; the server reads them from next to its binary.
COPY --from=web-build /src/target/release/hash.txt /usr/local/bin/hash.txt
COPY --from=web-build /src/target/site /app/site
ENV LEPTOS_OUTPUT_NAME=foyer-web \
    LEPTOS_SITE_ROOT=/app/site \
    LEPTOS_SITE_PKG_DIR=pkg \
    LEPTOS_SITE_ADDR=0.0.0.0:3000 \
    LEPTOS_ENV=PROD \
    LEPTOS_HASH_FILES=true \
    FOYER_API_URL=http://127.0.0.1:8080
EXPOSE 3000
ENTRYPOINT ["/usr/bin/tini", "--", "foyer-web"]

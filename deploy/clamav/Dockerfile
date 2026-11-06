# syntax=docker/dockerfile:1.7
#
# clamd for the Foyer job worker. The worker streams files to it over TCP
# (INSTREAM) at FOYER_CLAMD_ADDR, e.g. clamav.foyer.svc.internal:3310.
#
# Unlike the official clamav/clamav image this one runs as the unprivileged
# clamav user from the start, so orch can run it without extra Linux
# capabilities and it can still be deployed from GitHub Actions.
#
# Signatures are baked in at build time (CI rebuilds the image weekly) and
# freshclam keeps them current at runtime.

ARG BASE_IMAGE=debian:trixie-slim
FROM ${BASE_IMAGE}

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates clamav-daemon clamav-freshclam tini \
 && rm -rf /var/lib/apt/lists/* /etc/clamav/*.conf \
 && install -d -o clamav -g clamav /var/lib/clamav /run/clamav

COPY --chmod=0644 clamd.conf freshclam.conf /etc/clamav/

USER clamav
# Changing SIGNATURES_STAMP (CI passes the date) skips the layer cache so a
# rebuild bakes in fresh signatures. A failed download is not fatal: the
# entrypoint fetches signatures on start when none are present.
ARG SIGNATURES_STAMP=unset
RUN echo "signatures ${SIGNATURES_STAMP}" \
 && (freshclam --foreground --stdout || echo "freshclam failed at build time; signatures will be fetched on start")

COPY --chmod=0755 entrypoint.sh /usr/local/bin/clamav-entrypoint

LABEL org.opencontainers.image.title="foyer-clamav" \
      org.opencontainers.image.description="clamd for the Foyer job worker" \
      org.opencontainers.image.source="https://github.com/TorstenDittmann/foyer"
EXPOSE 3310
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/clamav-entrypoint"]

#!/bin/sh
# Keeps signatures current with freshclam and runs clamd in the foreground.
set -eu

if [ ! -e /var/lib/clamav/main.cvd ] && [ ! -e /var/lib/clamav/main.cld ]; then
  echo "no ClamAV signatures yet, downloading them"
  freshclam --foreground --stdout
fi

# Check for new signatures twice a day, which stays well inside the mirror's
# rate limits. freshclam tells clamd to reload (NotifyClamd) after an update.
(
  while sleep 43200; do
    freshclam --foreground --stdout || echo "freshclam failed; retrying in 12 hours"
  done
) &

exec clamd --foreground

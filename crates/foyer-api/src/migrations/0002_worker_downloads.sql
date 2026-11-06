-- Background jobs, build processing (malware scan and zip), launch targets
-- and anonymous download counts.

-- Work for `foyer-api worker`. A job is claimed by setting `locked_until`;
-- a worker that dies leaves the lease to run out, and another worker picks
-- the job up again.
CREATE TABLE jobs (
    id           TEXT PRIMARY KEY,
    kind         TEXT NOT NULL,
    payload      TEXT NOT NULL DEFAULT '{}',
    state        TEXT NOT NULL DEFAULT 'queued'
        CHECK (state IN ('queued', 'running', 'done', 'failed')),
    attempts     INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    run_at       TEXT NOT NULL,
    locked_until TEXT,
    last_error   TEXT,
    created_at   TEXT NOT NULL,
    finished_at  TEXT
);
CREATE INDEX jobs_due ON jobs (state, run_at);

-- `scan_status` is NULL until the worker scanned the build.
ALTER TABLE builds ADD COLUMN scan_status TEXT
    CHECK (scan_status IN ('clean', 'infected', 'incomplete', 'skipped'));
ALTER TABLE builds ADD COLUMN scan_findings TEXT NOT NULL DEFAULT '[]';
ALTER TABLE builds ADD COLUMN scanned_at TEXT;
-- Size of the zip in storage; NULL when there is none (not packed yet, or
-- replaced by a newer build on the channel).
ALTER TABLE builds ADD COLUMN archive_size INTEGER;
ALTER TABLE builds ADD COLUMN failure TEXT;
-- JSON `LaunchTarget`, or NULL.
ALTER TABLE builds ADD COLUMN launch TEXT;
CREATE INDEX builds_flagged ON builds (game_id, scan_status);

-- Anonymous download counters, one row per game, platform, source and UTC
-- day. Nothing about the downloader is stored.
CREATE TABLE download_counts (
    game_id  TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    platform TEXT NOT NULL,
    source   TEXT NOT NULL CHECK (source IN ('web', 'client_install', 'client_update')),
    day      TEXT NOT NULL,
    count    INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (game_id, platform, source, day)
);

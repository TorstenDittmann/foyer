-- Accounts, developer profiles, game pages, builds and the review queue.
-- Ids are UUID strings, timestamps RFC 3339 strings in UTC, lists JSON arrays.

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL DEFAULT 'user' CHECK (role IN ('user', 'reviewer', 'admin')),
    created_at    TEXT NOT NULL
);

-- Session tokens (from login) and API tokens (for the CLI and CI). Only a
-- SHA-256 of the secret is stored.
CREATE TABLE tokens (
    id           TEXT PRIMARY KEY,
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('session', 'api')),
    name         TEXT NOT NULL DEFAULT '',
    token_hash   TEXT NOT NULL UNIQUE,
    created_at   TEXT NOT NULL,
    last_used_at TEXT,
    expires_at   TEXT
);
CREATE INDEX tokens_by_user ON tokens (user_id, kind);

CREATE TABLE developers (
    id            TEXT PRIMARY KEY,
    owner_id      TEXT NOT NULL UNIQUE REFERENCES users (id),
    slug          TEXT NOT NULL UNIQUE,
    display_name  TEXT NOT NULL,
    bio           TEXT NOT NULL DEFAULT '',
    website_url   TEXT,
    support_links TEXT NOT NULL DEFAULT '[]',
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE games (
    id                  TEXT PRIMARY KEY,
    developer_id        TEXT NOT NULL REFERENCES developers (id),
    slug                TEXT NOT NULL UNIQUE,
    title               TEXT NOT NULL,
    tagline             TEXT NOT NULL DEFAULT '',
    description         TEXT NOT NULL DEFAULT '',
    tags                TEXT NOT NULL DEFAULT '[]',
    content_descriptors TEXT NOT NULL DEFAULT '[]',
    ai_disclosure       TEXT,
    support_links       TEXT NOT NULL DEFAULT '[]',
    status              TEXT NOT NULL DEFAULT 'draft'
        CHECK (status IN ('draft', 'in_review', 'changes_requested', 'published', 'rejected', 'delisted')),
    published_at        TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);
CREATE INDEX games_by_developer ON games (developer_id);
CREATE INDEX games_published ON games (status, published_at);

CREATE TABLE media (
    id           TEXT PRIMARY KEY,
    game_id      TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('capsule', 'screenshot')),
    content_type TEXT NOT NULL,
    position     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL
);
CREATE INDEX media_by_game ON media (game_id, kind, position);

CREATE TABLE builds (
    id           TEXT PRIMARY KEY,
    game_id      TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    channel      TEXT NOT NULL,
    platform     TEXT NOT NULL CHECK (platform IN ('windows', 'macos', 'linux', 'web')),
    version      TEXT,
    state        TEXT NOT NULL DEFAULT 'uploading'
        CHECK (state IN ('uploading', 'processing', 'ready', 'failed', 'rejected')),
    manifest     TEXT NOT NULL,
    total_size   INTEGER NOT NULL,
    chunk_count  INTEGER NOT NULL,
    created_by   TEXT NOT NULL REFERENCES users (id),
    created_at   TEXT NOT NULL,
    finalized_at TEXT
);
CREATE INDEX builds_by_game ON builds (game_id, created_at);

-- A channel (e.g. `windows-beta`) is tied to one platform and points at the
-- build players get.
CREATE TABLE channels (
    game_id          TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    name             TEXT NOT NULL,
    platform         TEXT NOT NULL,
    current_build_id TEXT REFERENCES builds (id),
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (game_id, name)
);

-- Chunks confirmed to be in storage, so later pushes skip them.
CREATE TABLE chunks (
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    hash       TEXT NOT NULL,
    size       INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    PRIMARY KEY (game_id, hash)
);

CREATE TABLE submissions (
    id              TEXT PRIMARY KEY,
    game_id         TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    status          TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'in_review', 'approved', 'changes_requested', 'rejected')),
    developer_notes TEXT NOT NULL DEFAULT '',
    reviewer_id     TEXT REFERENCES users (id),
    decision_notes  TEXT,
    rule            TEXT,
    created_at      TEXT NOT NULL,
    claimed_at      TEXT,
    decided_at      TEXT
);
CREATE INDEX submissions_queue ON submissions (status, created_at);
CREATE INDEX submissions_by_game ON submissions (game_id, created_at);
-- At most one open submission per game.
CREATE UNIQUE INDEX submissions_one_open ON submissions (game_id)
    WHERE status IN ('pending', 'in_review');

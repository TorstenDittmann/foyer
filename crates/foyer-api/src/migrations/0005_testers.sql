-- Tester keys and private channels.

-- A private channel is served only to tester keys that cover it, the
-- game's developer and reviewers.
ALTER TABLE channels ADD COLUMN private INTEGER NOT NULL DEFAULT 0;

-- Keys a developer hands to testers: they unlock one game before it is
-- published, or its private channels after. Only a SHA-256 of the key is
-- stored. `channels` is a JSON array; empty means every channel.
CREATE TABLE tester_keys (
    id           TEXT PRIMARY KEY,
    game_id      TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    label        TEXT NOT NULL DEFAULT '',
    channels     TEXT NOT NULL DEFAULT '[]',
    key_hash     TEXT NOT NULL UNIQUE,
    prefix       TEXT NOT NULL,
    created_by   TEXT NOT NULL REFERENCES users (id),
    created_at   TEXT NOT NULL,
    expires_at   TEXT,
    last_used_at TEXT
);
CREATE INDEX tester_keys_by_game ON tester_keys (game_id, created_at);

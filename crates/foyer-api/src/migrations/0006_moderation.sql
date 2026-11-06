-- Player reports, developer appeals and strikes.

-- Delistings were stored as rejected submissions with notes starting
-- "Delisted: ". Give them their own kind so they can be told apart.
ALTER TABLE submissions ADD COLUMN kind TEXT NOT NULL DEFAULT 'review'
    CHECK (kind IN ('review', 'delisting'));
UPDATE submissions SET kind = 'delisting',
    decision_notes = substr(decision_notes, length('Delisted: ') + 1)
    WHERE status = 'rejected' AND decision_notes LIKE 'Delisted: %';

-- A player's report on a game. Anonymous reports have no reporter_id;
-- reporter_key identifies the reporter for de-duplication either way
-- (`user:<id>`, or `ip:<sha256 of the address>` without storing the
-- address itself).
CREATE TABLE reports (
    id               TEXT PRIMARY KEY,
    game_id          TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    reason           TEXT NOT NULL CHECK (reason IN
        ('malware', 'broken', 'stolen', 'guidelines', 'misleading', 'spam', 'other')),
    details          TEXT NOT NULL DEFAULT '',
    reporter_id      TEXT REFERENCES users (id) ON DELETE SET NULL,
    reporter_key     TEXT NOT NULL,
    status           TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'dismissed', 'actioned')),
    resolved_by      TEXT REFERENCES users (id),
    resolution_notes TEXT,
    created_at       TEXT NOT NULL,
    resolved_at      TEXT
);
CREATE INDEX reports_by_status ON reports (status, game_id, created_at);
CREATE INDEX reports_by_game ON reports (game_id, created_at);
-- One open report per reporter and game.
CREATE UNIQUE INDEX reports_one_open ON reports (game_id, reporter_key)
    WHERE status = 'open';

-- A recorded violation against a developer.
CREATE TABLE strikes (
    id            TEXT PRIMARY KEY,
    developer_id  TEXT NOT NULL REFERENCES developers (id) ON DELETE CASCADE,
    game_id       TEXT REFERENCES games (id) ON DELETE SET NULL,
    -- The rejection or delisting it came with, so a successful appeal can
    -- take it back.
    submission_id TEXT REFERENCES submissions (id) ON DELETE SET NULL,
    rule          TEXT NOT NULL,
    notes         TEXT NOT NULL,
    issued_by     TEXT REFERENCES users (id),
    created_at    TEXT NOT NULL,
    revoked_at    TEXT,
    revoked_by    TEXT REFERENCES users (id),
    revoke_reason TEXT
);
CREATE INDEX strikes_by_developer ON strikes (developer_id, created_at);
CREATE INDEX strikes_by_submission ON strikes (submission_id);

-- A developer's appeal of a rejected submission or a delisting. One per
-- decision.
CREATE TABLE appeals (
    id             TEXT PRIMARY KEY,
    submission_id  TEXT NOT NULL UNIQUE REFERENCES submissions (id) ON DELETE CASCADE,
    game_id        TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    statement      TEXT NOT NULL,
    status         TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'upheld', 'overturned', 'reopened')),
    reviewer_id    TEXT REFERENCES users (id),
    decision_notes TEXT,
    created_at     TEXT NOT NULL,
    decided_at     TEXT
);
CREATE INDEX appeals_queue ON appeals (status, created_at);
CREATE INDEX appeals_by_game ON appeals (game_id, created_at);

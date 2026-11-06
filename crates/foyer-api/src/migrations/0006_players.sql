-- Player features: wishlists, following developers, release emails and
-- passkeys.

-- Opt-in: email when a followed developer releases a game. Off until the
-- player turns it on.
ALTER TABLE users ADD COLUMN release_emails INTEGER NOT NULL DEFAULT 0;
-- Secret in the "unsubscribe" link of release emails. It can only turn
-- release emails off, so it is stored as is and made on first use.
ALTER TABLE users ADD COLUMN unsubscribe_token TEXT;
CREATE UNIQUE INDEX users_by_unsubscribe ON users (unsubscribe_token)
    WHERE unsubscribe_token IS NOT NULL;

CREATE TABLE wishlist (
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (user_id, game_id)
);
CREATE INDEX wishlist_by_game ON wishlist (game_id);

CREATE TABLE follows (
    user_id      TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    developer_id TEXT NOT NULL REFERENCES developers (id) ON DELETE CASCADE,
    created_at   TEXT NOT NULL,
    PRIMARY KEY (user_id, developer_id)
);
CREATE INDEX follows_by_developer ON follows (developer_id);

-- One row per follower told about a game's release, so a retried
-- `notify_release` job never emails anyone twice.
CREATE TABLE release_notices (
    game_id    TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    PRIMARY KEY (game_id, user_id)
);

-- Passkeys. `credential_id` is the authenticator's id (base64url), `passkey`
-- the JSON webauthn-rs needs to check sign-ins (public key, counter).
CREATE TABLE passkeys (
    id            TEXT PRIMARY KEY,
    user_id       TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    credential_id TEXT NOT NULL UNIQUE,
    name          TEXT NOT NULL,
    passkey       TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    last_used_at  TEXT
);
CREATE INDEX passkeys_by_user ON passkeys (user_id);

-- Passkey registrations and sign-ins in progress. The challenge state stays
-- on the server; the client only gets `id`. Each is used once.
CREATE TABLE passkey_ceremonies (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('register', 'login')),
    user_id    TEXT REFERENCES users (id) ON DELETE CASCADE,
    state      TEXT NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX passkey_ceremonies_expiry ON passkey_ceremonies (expires_at);

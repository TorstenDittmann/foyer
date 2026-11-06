-- Email verification, password resets and Discord sign-in.

ALTER TABLE users ADD COLUMN email_verified_at TEXT;
-- Discord's user id for accounts that signed in with Discord. Such accounts
-- may have no password (`password_hash` is then '').
ALTER TABLE users ADD COLUMN discord_id TEXT;
CREATE UNIQUE INDEX users_by_discord ON users (discord_id) WHERE discord_id IS NOT NULL;

-- One-time links sent by email. Only a SHA-256 of the token is stored. A
-- token is good only for the address it was sent to.
CREATE TABLE email_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    purpose    TEXT NOT NULL CHECK (purpose IN ('verify_email', 'reset_password')),
    email      TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used_at    TEXT
);
CREATE INDEX email_tokens_by_user ON email_tokens (user_id, purpose);

-- Discord sign-ins in progress (the OAuth `state`), so a callback can only
-- finish a sign-in this API started. Hashed like tokens.
CREATE TABLE oauth_states (
    state_hash TEXT PRIMARY KEY,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL
);

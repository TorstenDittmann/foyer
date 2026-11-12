-- Page design: colours, hero and logo art and the order of the page's
-- sections, as the JSON of `foyer_types::PageStyle`. `{}` is the standard
-- Store look.
ALTER TABLE games ADD COLUMN page_style TEXT NOT NULL DEFAULT '{}';

-- A third image kind, `artwork`, for hero banners, logos and image blocks.
-- SQLite can't change a CHECK constraint, so the table is rebuilt. Nothing
-- references `media`, so dropping it is safe with foreign keys on.
CREATE TABLE media_new (
    id           TEXT PRIMARY KEY,
    game_id      TEXT NOT NULL REFERENCES games (id) ON DELETE CASCADE,
    kind         TEXT NOT NULL CHECK (kind IN ('capsule', 'screenshot', 'artwork')),
    content_type TEXT NOT NULL,
    position     INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL
);
INSERT INTO media_new (id, game_id, kind, content_type, position, created_at)
    SELECT id, game_id, kind, content_type, position, created_at FROM media;
DROP TABLE media;
ALTER TABLE media_new RENAME TO media;
CREATE INDEX media_by_game ON media (game_id, kind, position);

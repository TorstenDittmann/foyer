-- Full-text search over game pages (FTS5). Triggers keep the index in step
-- with `games` and developer names; only published games are searched, but
-- every game is indexed so publishing needs no extra step.

CREATE VIRTUAL TABLE games_fts USING fts5(
    game_id UNINDEXED,
    title,
    tagline,
    tags,
    developer,
    description,
    tokenize = 'unicode61 remove_diacritics 2'
);

INSERT INTO games_fts (game_id, title, tagline, tags, developer, description)
SELECT g.id, g.title, g.tagline,
       (SELECT group_concat(value, ' ') FROM json_each(g.tags)),
       d.display_name, g.description
FROM games g JOIN developers d ON d.id = g.developer_id;

CREATE TRIGGER games_fts_insert AFTER INSERT ON games BEGIN
    INSERT INTO games_fts (game_id, title, tagline, tags, developer, description)
    VALUES (
        new.id, new.title, new.tagline,
        (SELECT group_concat(value, ' ') FROM json_each(new.tags)),
        (SELECT display_name FROM developers WHERE id = new.developer_id),
        new.description
    );
END;

CREATE TRIGGER games_fts_update
AFTER UPDATE OF title, tagline, tags, description, developer_id ON games BEGIN
    DELETE FROM games_fts WHERE game_id = old.id;
    INSERT INTO games_fts (game_id, title, tagline, tags, developer, description)
    VALUES (
        new.id, new.title, new.tagline,
        (SELECT group_concat(value, ' ') FROM json_each(new.tags)),
        (SELECT display_name FROM developers WHERE id = new.developer_id),
        new.description
    );
END;

CREATE TRIGGER games_fts_delete AFTER DELETE ON games BEGIN
    DELETE FROM games_fts WHERE game_id = old.id;
END;

CREATE TRIGGER games_fts_developer AFTER UPDATE OF display_name ON developers BEGIN
    UPDATE games_fts SET developer = new.display_name
    WHERE game_id IN (SELECT id FROM games WHERE developer_id = new.id);
END;

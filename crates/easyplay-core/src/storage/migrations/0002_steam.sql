-- EasyPlay library schema, version 2 (DATA_MODEL §6, ADR-0031): what EasyPlay itself knows
-- about a Steam game. Everything Steam knows is read from Steam's files on every scan and is
-- never stored. Released migrations are never edited.

CREATE TABLE steam_games (
  app_id        INTEGER PRIMARY KEY CHECK (app_id > 0),
  first_seen_at INTEGER NOT NULL,
  updated_at    INTEGER NOT NULL,
  favorite      INTEGER NOT NULL DEFAULT 0,
  hidden        INTEGER NOT NULL DEFAULT 0,
  notes         TEXT NOT NULL DEFAULT ''
) STRICT;

CREATE TABLE steam_game_tags (
  app_id INTEGER NOT NULL REFERENCES steam_games(app_id) ON DELETE CASCADE,
  tag_id INTEGER NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
  PRIMARY KEY (app_id, tag_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX steam_game_tags_tag ON steam_game_tags(tag_id);

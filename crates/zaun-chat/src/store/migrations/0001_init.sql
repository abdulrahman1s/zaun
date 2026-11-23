-- Every value column holds a sealed blob (seal.rs). Eviction: the engine deletes rows it
-- no longer needs (kv, files) and trims items by retention; seq only ever grows, by design.
CREATE TABLE meta (k TEXT PRIMARY KEY, v BLOB NOT NULL) WITHOUT ROWID;
CREATE TABLE kv (k BLOB PRIMARY KEY, v BLOB NOT NULL) WITHOUT ROWID;
CREATE TABLE items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    chat BLOB NOT NULL,
    v BLOB NOT NULL
);
CREATE INDEX items_chat_id ON items (chat, id);
CREATE TABLE seq (name TEXT PRIMARY KEY, next INTEGER NOT NULL) WITHOUT ROWID;
CREATE TABLE files (id INTEGER PRIMARY KEY, v BLOB NOT NULL);

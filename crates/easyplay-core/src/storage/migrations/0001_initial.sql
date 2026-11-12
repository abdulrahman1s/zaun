-- EasyPlay library schema, version 1 (DATA_MODEL §6). Released migrations are never edited.
PRAGMA application_id = 1162890329; -- 0x45504C59 "EPLY"

CREATE TABLE meta (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL
) STRICT, WITHOUT ROWID;
-- keys: created_at, install_id (random 128-bit hex), last_app_version, last_backup_at

CREATE TABLE games (
  id                     TEXT PRIMARY KEY,
  name                   TEXT NOT NULL CHECK (length(name) BETWEEN 1 AND 80),
  sort_name              TEXT NOT NULL,
  name_is_user_set       INTEGER NOT NULL DEFAULT 0,
  root                   TEXT,
  added_at               INTEGER NOT NULL,
  updated_at             INTEGER NOT NULL,
  deleted_at             INTEGER,
  favorite               INTEGER NOT NULL DEFAULT 0,
  hidden                 INTEGER NOT NULL DEFAULT 0,
  notes                  TEXT NOT NULL DEFAULT '',
  policy_json            TEXT NOT NULL,
  policy_schema          INTEGER NOT NULL,
  policy_updated_at      INTEGER NOT NULL,
  launch_json            TEXT NOT NULL,
  launch_schema          INTEGER NOT NULL,
  umu_id                 TEXT,
  store                  TEXT,
  runtime_ref_json       TEXT NOT NULL,
  runtime_follow_updates INTEGER NOT NULL DEFAULT 1,
  runtime_last_resolved  TEXT,
  prefix_tainted_at      INTEGER,
  prefix_prepared_with   TEXT,
  flags                  INTEGER NOT NULL DEFAULT 0,
  last_launched_at       INTEGER
) STRICT;
CREATE INDEX games_sort ON games(sort_name) WHERE deleted_at IS NULL;

CREATE TABLE executables (
  id                 TEXT PRIMARY KEY,
  game_id            TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
  role               TEXT NOT NULL CHECK (role IN ('primary','alternate','launcher','tool','installer')),
  path               TEXT NOT NULL,
  canonical_path     TEXT NOT NULL UNIQUE,
  rel_path           TEXT,
  mount_point        TEXT,
  size               INTEGER NOT NULL,
  mtime_ns           INTEGER NOT NULL,
  quick_hash         BLOB NOT NULL CHECK (length(quick_hash) = 32),
  full_hash          BLOB CHECK (full_hash IS NULL OR length(full_hash) = 32),
  pe_json            TEXT NOT NULL,
  args_override_json TEXT,
  label              TEXT,
  added_at           INTEGER NOT NULL
) STRICT;
CREATE UNIQUE INDEX executables_one_primary ON executables(game_id) WHERE role = 'primary';
CREATE INDEX executables_game       ON executables(game_id);
CREATE INDEX executables_quick_hash ON executables(quick_hash);

CREATE TABLE tags (
  id         INTEGER PRIMARY KEY,
  name       TEXT NOT NULL UNIQUE COLLATE NOCASE CHECK (length(name) BETWEEN 1 AND 32),
  color      INTEGER NOT NULL DEFAULT 0 CHECK (color BETWEEN 0 AND 7),
  created_at INTEGER NOT NULL
) STRICT;
CREATE TABLE game_tags (
  game_id TEXT    NOT NULL REFERENCES games(id) ON DELETE CASCADE,
  tag_id  INTEGER NOT NULL REFERENCES tags(id)  ON DELETE CASCADE,
  PRIMARY KEY (game_id, tag_id)
) STRICT, WITHOUT ROWID;
CREATE INDEX game_tags_tag ON game_tags(tag_id);

CREATE TABLE artwork (
  game_id    TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
  slot       TEXT NOT NULL CHECK (slot IN ('cover','icon','hero')),
  file       TEXT NOT NULL,              -- file name inside artwork/<game_id>/
  hash       BLOB NOT NULL CHECK (length(hash) = 32),
  width      INTEGER NOT NULL,
  height     INTEGER NOT NULL,
  source     TEXT NOT NULL CHECK (source IN ('file','executable_icon','generated')),
  updated_at INTEGER NOT NULL,
  PRIMARY KEY (game_id, slot)
) STRICT, WITHOUT ROWID;

CREATE TABLE sessions (
  id                     TEXT PRIMARY KEY,
  game_id                TEXT REFERENCES games(id) ON DELETE SET NULL,
  game_name              TEXT NOT NULL,
  executable_id          TEXT,
  kind                   TEXT NOT NULL CHECK (kind IN ('game','installer','tool','prepare','provision')),
  state                  TEXT NOT NULL CHECK (state IN ('starting','running','lingering','stopping','ended')),
  started_at             INTEGER NOT NULL,
  ended_at               INTEGER,
  duration_ms            INTEGER,
  active_ms              INTEGER NOT NULL DEFAULT 0,
  last_heartbeat_at      INTEGER,
  end_reason             TEXT,
  end_detail             TEXT,           -- exit code, signal number or error code
  exit_status            INTEGER,
  counted                INTEGER NOT NULL DEFAULT 0,
  supervisor_pid         INTEGER,
  supervisor_start_ticks INTEGER,
  boot_id                TEXT,
  sandbox_pid            INTEGER,
  pid_ns                 INTEGER,
  preset                 TEXT,
  network                TEXT,
  runtime_name           TEXT,
  slr_variant            TEXT,
  isolation              TEXT,
  tz_offset_min          INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX sessions_game_time ON sessions(game_id, started_at DESC);
CREATE INDEX sessions_time      ON sessions(started_at);
CREATE INDEX sessions_open      ON sessions(state) WHERE state != 'ended';

CREATE TABLE session_events (
  id         INTEGER PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  at         INTEGER NOT NULL,
  kind       TEXT NOT NULL,              -- phase, state, stop_requested, error, note
  data_json  TEXT NOT NULL DEFAULT '{}'
) STRICT;
CREATE INDEX session_events_session ON session_events(session_id, id);

CREATE TABLE game_stats (
  game_id         TEXT PRIMARY KEY REFERENCES games(id) ON DELETE CASCADE,
  total_ms        INTEGER NOT NULL DEFAULT 0,
  sessions        INTEGER NOT NULL DEFAULT 0,
  failed_launches INTEGER NOT NULL DEFAULT 0,
  first_played_at INTEGER,
  last_played_at  INTEGER,
  longest_ms      INTEGER NOT NULL DEFAULT 0,
  last_launch_at  INTEGER
) STRICT, WITHOUT ROWID;

CREATE TABLE playtime_daily (
  game_key TEXT    NOT NULL,             -- game id, or 'removed:<name>' for kept history of removed games
  day      INTEGER NOT NULL,             -- local calendar date as YYYYMMDD
  ms       INTEGER NOT NULL,
  sessions INTEGER NOT NULL DEFAULT 0,   -- sessions that started on this day
  PRIMARY KEY (game_key, day)
) STRICT, WITHOUT ROWID;
CREATE INDEX playtime_daily_day ON playtime_daily(day);

CREATE TABLE runtimes (
  name          TEXT PRIMARY KEY,
  family        TEXT NOT NULL CHECK (family IN ('umu_proton','ge_proton','valve_proton','other')),
  version_key   TEXT NOT NULL,           -- zero-padded natural sort key
  origin        TEXT NOT NULL CHECK (origin IN ('store','external')),
  origin_label  TEXT NOT NULL,
  path          TEXT NOT NULL UNIQUE,
  required_slr  TEXT NOT NULL,
  display_name  TEXT NOT NULL,
  has_fixes     INTEGER NOT NULL DEFAULT 0,
  size_bytes    INTEGER,
  discovered_at INTEGER NOT NULL,
  last_used_at  INTEGER
) STRICT, WITHOUT ROWID;

CREATE TABLE slr_installs (
  variant              TEXT PRIMARY KEY, -- steamrt3, steamrt4
  version              TEXT NOT NULL,
  path                 TEXT NOT NULL,
  platform_dir         TEXT NOT NULL,
  manifest_tree_sha256 BLOB,
  manifest_built_at    INTEGER,
  size_bytes           INTEGER
) STRICT, WITHOUT ROWID;

CREATE TABLE runtime_audit (
  variant       TEXT PRIMARY KEY,
  pending       INTEGER NOT NULL DEFAULT 0,
  last_run_at   INTEGER,
  last_clean_at INTEGER,
  last_result   TEXT                     -- clean | tampered | missing
) STRICT, WITHOUT ROWID;

CREATE TABLE runtime_events (
  id           INTEGER PRIMARY KEY,
  at           INTEGER NOT NULL,
  variant      TEXT,
  kind         TEXT NOT NULL,            -- provisioned, updated, audit_clean, audit_tampered, quarantined, removed
  summary_json TEXT NOT NULL DEFAULT '{}'
) STRICT;

CREATE TABLE kv (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at INTEGER NOT NULL
) STRICT, WITHOUT ROWID;
-- UI state: library.view, library.sort.<collection>, library.columns, sidebar.tag_order,
--           last_route, onboarding.step, dismissed.<notice-id>, integration.previous_handlers
-- (window geometry is not here: it lives in <state>/window.json, see PERSISTENCE §1)

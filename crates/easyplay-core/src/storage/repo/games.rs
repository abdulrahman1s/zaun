//! `games` and the library snapshot.

use std::collections::HashMap;

use ormlite::Model;

use super::executables::ExeRow;
use super::{
    Arg, GamePatch, GameRecord, GameRepo, LibraryRows, conn, dec_path_opt, enc_path_opt, exec_args,
    id_parse, json_parse, json_text, ms_opt, not_found, runtime_name, ts, ts_opt, uint,
};
use crate::clock::Timestamp;
use crate::ids::GameId;
use crate::model::{
    ExeRole, Game, GameFlags, GameKey, Store as GameStore, parse_launch_config, parse_policy,
    sort_name_of,
};
use crate::storage::db::{SqlConn, map_orm, map_sqlx};
use crate::storage::{StorageResult, Store};

/// A `games` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "games")]
pub(super) struct GameRow {
    #[ormlite(primary_key)]
    pub id: String,
    pub name: String,
    pub sort_name: String,
    pub name_is_user_set: bool,
    pub root: Option<String>,
    pub added_at: i64,
    pub updated_at: i64,
    pub deleted_at: Option<i64>,
    pub favorite: bool,
    pub hidden: bool,
    pub notes: String,
    pub policy_json: String,
    pub policy_schema: i64,
    pub policy_updated_at: i64,
    pub launch_json: String,
    pub launch_schema: i64,
    pub umu_id: Option<String>,
    pub store: Option<String>,
    pub runtime_ref_json: String,
    pub runtime_follow_updates: bool,
    pub runtime_last_resolved: Option<String>,
    pub prefix_tainted_at: Option<i64>,
    pub prefix_prepared_with: Option<String>,
    pub flags: i64,
    pub last_launched_at: Option<i64>,
}

impl GameRow {
    pub(super) fn from_game(g: &Game) -> StorageResult<Self> {
        Ok(Self {
            id: g.id.to_string(),
            name: g.name.clone(),
            sort_name: g.sort_name.clone(),
            name_is_user_set: g.name_is_user_set,
            root: enc_path_opt(g.root.as_deref())?,
            added_at: g.added_at.as_millis(),
            updated_at: g.updated_at.as_millis(),
            deleted_at: None,
            favorite: g.favorite,
            hidden: g.hidden,
            notes: g.notes.clone(),
            policy_json: json_text(&g.policy)?,
            policy_schema: i64::from(g.policy.schema_version),
            policy_updated_at: g.policy_updated_at.as_millis(),
            launch_json: json_text(&g.launch)?,
            launch_schema: i64::from(g.launch.schema_version),
            umu_id: g.umu_id.clone(),
            store: g.store.as_ref().map(|s| s.as_str().to_owned()),
            runtime_ref_json: json_text(&g.runtime)?,
            runtime_follow_updates: g.runtime_follow_updates,
            runtime_last_resolved: g.runtime_last_resolved.as_ref().map(|r| r.to_string()),
            prefix_tainted_at: ms_opt(g.prefix_tainted_at),
            prefix_prepared_with: g.prefix_prepared_with.as_ref().map(|r| r.to_string()),
            flags: i64::from(g.flags.bits()),
            last_launched_at: ms_opt(g.last_launched_at),
        })
    }

    /// Decode a row. Document errors are kept in the record; any other undecodable column
    /// is an error of the whole row.
    pub(super) fn into_record(
        self,
        executables: Vec<crate::model::Executable>,
    ) -> StorageResult<GameRecord> {
        let id: GameId = id_parse("games.id", &self.id)?;
        let schema = |v: i64| u32::try_from(v).unwrap_or(u32::MAX);
        Ok(GameRecord {
            id,
            name: self.name,
            sort_name: self.sort_name,
            name_is_user_set: self.name_is_user_set,
            root: dec_path_opt(self.root.as_deref())?,
            added_at: ts(self.added_at),
            updated_at: ts(self.updated_at),
            deleted_at: ts_opt(self.deleted_at),
            favorite: self.favorite,
            hidden: self.hidden,
            notes: self.notes,
            policy: parse_policy(&self.policy_json, schema(self.policy_schema)),
            policy_updated_at: ts(self.policy_updated_at),
            launch: parse_launch_config(&self.launch_json, schema(self.launch_schema)),
            umu_id: self.umu_id,
            store: self.store.as_deref().map(GameStore::parse),
            runtime: json_parse("games.runtime_ref_json", &self.runtime_ref_json)?,
            runtime_follow_updates: self.runtime_follow_updates,
            runtime_last_resolved: self
                .runtime_last_resolved
                .as_deref()
                .map(runtime_name)
                .transpose()?,
            prefix_tainted_at: ts_opt(self.prefix_tainted_at),
            prefix_prepared_with: self
                .prefix_prepared_with
                .as_deref()
                .map(runtime_name)
                .transpose()?,
            flags: GameFlags::from_bits_retain(u32::try_from(self.flags).unwrap_or(0)),
            last_launched_at: ts_opt(self.last_launched_at),
            executables,
        })
    }

    fn apply(&mut self, p: &GamePatch, now: Timestamp) -> StorageResult<()> {
        if let Some(name) = &p.name {
            crate::model::validate_name(name)?;
            self.name.clone_from(name);
            self.sort_name = sort_name_of(name);
        }
        if let Some(s) = &p.sort_name {
            self.sort_name.clone_from(s);
        }
        if let Some(v) = p.name_is_user_set {
            self.name_is_user_set = v;
        }
        if let Some(root) = &p.root {
            self.root = enc_path_opt(root.as_deref())?;
        }
        if let Some(v) = p.favorite {
            self.favorite = v;
        }
        if let Some(v) = p.hidden {
            self.hidden = v;
        }
        if let Some(n) = &p.notes {
            crate::model::validate_notes(n)?;
            self.notes.clone_from(n);
        }
        if let Some(policy) = &p.policy {
            policy.validate()?;
            self.policy_json = json_text(policy)?;
            self.policy_schema = i64::from(policy.schema_version);
            self.policy_updated_at = now.as_millis();
        }
        if let Some(launch) = &p.launch {
            launch.validate()?;
            self.launch_json = json_text(launch)?;
            self.launch_schema = i64::from(launch.schema_version);
        }
        if let Some(u) = &p.umu_id {
            if let Some(id) = u {
                crate::model::validate_umu_id(id)?;
            }
            self.umu_id.clone_from(u);
        }
        if let Some(s) = &p.store {
            self.store = s.as_ref().map(|s| s.as_str().to_owned());
        }
        if let Some(r) = &p.runtime {
            self.runtime_ref_json = json_text(r)?;
        }
        if let Some(v) = p.runtime_follow_updates {
            self.runtime_follow_updates = v;
        }
        if let Some(r) = &p.runtime_last_resolved {
            self.runtime_last_resolved = r.as_ref().map(|r| r.to_string());
        }
        if let Some(r) = &p.prefix_prepared_with {
            self.prefix_prepared_with = r.as_ref().map(|r| r.to_string());
        }
        if let Some(f) = p.flags {
            self.flags = i64::from(f.bits());
        }
        self.updated_at = now.as_millis();
        Ok(())
    }
}

/// Sort executables: primary first, then by `added_at`, then ID.
pub(super) fn sort_exes(v: &mut [crate::model::Executable]) {
    v.sort_by(|a, b| {
        (a.role != ExeRole::Primary, a.added_at, a.id).cmp(&(
            b.role != ExeRole::Primary,
            b.added_at,
            b.id,
        ))
    });
}

async fn fetch_game_row(c: &mut SqlConn, id: GameId) -> StorageResult<Option<GameRow>> {
    GameRow::select()
        .where_bind("id = ?", id.to_string())
        .fetch_optional(&mut *c)
        .await
        .map_err(map_orm)
}

async fn fetch_record(c: &mut SqlConn, id: GameId) -> StorageResult<Option<GameRecord>> {
    let Some(row) = fetch_game_row(c, id).await? else {
        return Ok(None);
    };
    let exes = super::executables::fetch_of(c, id).await?;
    row.into_record(exes).map(Some)
}

impl<S: Store + ?Sized> GameRepo for S {
    fn insert_game(&mut self, game: &Game) -> StorageResult<()> {
        game.validate()?;
        let row = GameRow::from_game(game)?;
        let exes = game
            .executables
            .iter()
            .map(|e| ExeRow::from_exe(game.id, e))
            .collect::<StorageResult<Vec<_>>>()?;
        conn(self).atomic(async move |c| {
            row.insert(&mut *c).await.map_err(map_orm)?;
            for e in exes {
                e.insert(&mut *c).await.map_err(map_orm)?;
            }
            Ok(())
        })
    }

    fn game_record(&mut self, id: GameId) -> StorageResult<Option<GameRecord>> {
        conn(self).snapshot(async move |c| fetch_record(c, id).await)
    }

    fn game(&mut self, id: GameId) -> StorageResult<Option<Game>> {
        match self.game_record(id)? {
            Some(r) if r.deleted_at.is_none() => r.into_game().map(Some),
            _ => Ok(None),
        }
    }

    fn update_game(&mut self, id: GameId, patch: &GamePatch, now: Timestamp) -> StorageResult<()> {
        let patch = patch.clone();
        conn(self).atomic(async move |c| {
            let mut row = fetch_game_row(c, id)
                .await?
                .filter(|r| r.deleted_at.is_none())
                .ok_or_else(|| not_found("game", id))?;
            row.apply(&patch, now)?;
            row.update_all_fields(&mut *c).await.map_err(map_orm)?;
            Ok(())
        })
    }

    fn set_game_deleted(&mut self, id: GameId, at: Option<Timestamp>) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE games SET deleted_at = ? WHERE id = ?",
                &[Arg::Int(ms_opt(at)), Arg::Text(Some(id.to_string()))],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn deleted_games(&mut self) -> StorageResult<Vec<GameId>> {
        conn(self).run(async |c| {
            let rows: Vec<(String,)> =
                ormlite::query_as("SELECT id FROM games WHERE deleted_at IS NOT NULL ORDER BY id")
                    .fetch_all(&mut *c)
                    .await
                    .map_err(map_sqlx)?;
            rows.iter().map(|(id,)| id_parse("games.id", id)).collect()
        })
    }

    fn purge_game(&mut self, id: GameId, keep_history: bool) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            let Some(row) = fetch_game_row(c, id).await? else {
                return Ok(false);
            };
            let key = id.to_string();
            if keep_history {
                let removed = GameKey::Removed(row.name.clone()).as_column();
                exec_args(
                    c,
                    "INSERT INTO playtime_daily(game_key, day, ms, sessions) \
                     SELECT ?, day, ms, sessions FROM playtime_daily WHERE game_key = ? \
                     ON CONFLICT(game_key, day) DO UPDATE SET ms = ms + excluded.ms, \
                     sessions = sessions + excluded.sessions",
                    &[Arg::Text(Some(removed)), Arg::Text(Some(key.clone()))],
                )
                .await?;
            } else {
                exec_args(
                    c,
                    "DELETE FROM sessions WHERE game_id = ?",
                    &[Arg::Text(Some(key.clone()))],
                )
                .await?;
            }
            exec_args(
                c,
                "DELETE FROM playtime_daily WHERE game_key = ?",
                &[Arg::Text(Some(key.clone()))],
            )
            .await?;
            exec_args(c, "DELETE FROM games WHERE id = ?", &[Arg::Text(Some(key))]).await?;
            Ok(true)
        })
    }

    fn load_library(&mut self) -> StorageResult<LibraryRows> {
        conn(self).snapshot(async |c| {
            let games = GameRow::query(
                "SELECT * FROM games WHERE deleted_at IS NULL ORDER BY sort_name, id",
            )
            .fetch_all(&mut *c)
            .await
            .map_err(map_sqlx)?;
            let mut exes: HashMap<GameId, Vec<crate::model::Executable>> = HashMap::new();
            for (game, exe) in super::executables::fetch_all(c).await? {
                exes.entry(game).or_default().push(exe);
            }
            let mut records = Vec::with_capacity(games.len());
            for row in games {
                let id: GameId = id_parse("games.id", &row.id)?;
                let mut list = exes.remove(&id).unwrap_or_default();
                sort_exes(&mut list);
                records.push(row.into_record(list)?);
            }
            let live: std::collections::HashSet<GameId> = records.iter().map(|r| r.id).collect();
            let tags = super::tags::fetch_all(c).await?;
            let game_tags = super::tags::fetch_links(c)
                .await?
                .into_iter()
                .filter(|(g, _)| live.contains(g))
                .collect();
            let artwork = super::artwork::fetch_all(c)
                .await?
                .into_iter()
                .filter(|a| live.contains(&a.game_id))
                .collect();
            let stats = super::stats::fetch_all(c)
                .await?
                .into_iter()
                .filter(|s| live.contains(&s.game_id))
                .collect();
            Ok(LibraryRows {
                games: records,
                tags,
                game_tags,
                artwork,
                stats,
            })
        })
    }

    fn set_prefix_tainted_at(&mut self, id: GameId, at: Option<Timestamp>) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE games SET prefix_tainted_at = ? WHERE id = ?",
                &[Arg::Int(ms_opt(at)), Arg::Text(Some(id.to_string()))],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn set_last_launched_at(&mut self, id: GameId, at: Timestamp) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE games SET last_launched_at = ? WHERE id = ?",
                &[
                    Arg::Int(Some(at.as_millis())),
                    Arg::Text(Some(id.to_string())),
                ],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn game_count(&mut self) -> StorageResult<u64> {
        conn(self).run(async |c| {
            let (n,): (i64,) =
                ormlite::query_as("SELECT count(*) FROM games WHERE deleted_at IS NULL")
                    .fetch_one(&mut *c)
                    .await
                    .map_err(map_sqlx)?;
            Ok(uint(n))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DocError;
    use crate::storage::Db;

    /// A minimal valid game, built from JSON so that the unit test needs no fixtures crate.
    fn sample(id: &str, path: &str) -> Game {
        let policy = r#"{"schema_version":1,"preset":"offline","network":"none",
            "game_dir":"read_write","folders":[],"save_data":"internal","removable":"none",
            "devices":{"audio":"full","alsa_devices":false,"controllers":"detected","camera":false},
            "display":{"wayland":"auto","x11":"auto","isolation":"none"},
            "ipc":{"session_bus":"none"},
            "hardening":{"seccomp":"default","mask_hostname":true,"mask_machine_id":true,
                         "mask_hardware_ids":true,"runtime_isolation":"auto"},"exceptions":[]}"#;
        let launch = serde_json::to_string(&crate::model::LaunchConfig::default()).unwrap();
        let exe = format!(
            r#"{{"id":"{id_e}","role":"primary","path":"{path}","canonical_path":"{path}",
            "rel_path":null,"mount_point":null,
            "fingerprint":{{"size":1,"mtime_ns":0,"quick_hash":"{h}","full_hash":null}},
            "pe":{{"product_name":null,"file_description":null,"company":null,
                   "original_filename":null,"file_version":null,"machine":"x86_64",
                   "subsystem":"gui","timestamp":null,"has_icon":false,"is_dll":false,
                   "is_dotnet":false,"notes":[]}},
            "args_override":null,"label":null,"added_at":0}}"#,
            id_e = id.replacen("0192", "0193", 1),
            h = "ab".repeat(32),
        );
        let json = format!(
            r#"{{"id":"{id}","name":"Game","sort_name":"game","name_is_user_set":false,
            "root":null,"added_at":0,"updated_at":0,"favorite":false,"hidden":false,"notes":"",
            "policy":{policy},"policy_updated_at":0,"launch":{launch},"umu_id":null,"store":null,
            "runtime":"default","runtime_follow_updates":true,"runtime_last_resolved":null,
            "prefix_tainted_at":null,"prefix_prepared_with":null,"flags":0,
            "last_launched_at":null,"executables":[{exe}]}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn too_new_or_garbage_document_disables_only_that_game() {
        let mut db = Db::open_in_memory().unwrap();
        let good = sample("0192f0c2-0000-7000-8000-000000000001", "/g/a.exe");
        let newer = sample("0192f0c2-0000-7000-8000-000000000002", "/g/b.exe");
        let broken = sample("0192f0c2-0000-7000-8000-000000000003", "/g/c.exe");
        for g in [&good, &newer, &broken] {
            db.insert_game(g).unwrap();
        }
        let (n, b) = (newer.id.to_string(), broken.id.to_string());
        conn(&mut db)
            .run(async move |c| {
                exec_args(
                    c,
                    "UPDATE games SET policy_schema = 9, \
                     policy_json = json_set(policy_json, '$.schema_version', 9) WHERE id = ?",
                    &[Arg::text(n)],
                )
                .await?;
                exec_args(
                    c,
                    "UPDATE games SET launch_json = 'not json' WHERE id = ?",
                    &[Arg::text(b)],
                )
                .await
            })
            .unwrap();
        let lib = db.load_library().unwrap();
        assert_eq!(lib.games.len(), 3);
        let rec = |id| lib.games.iter().find(|r| r.id == id).unwrap();
        assert_eq!(
            rec(newer.id).policy,
            Err(DocError::TooNew {
                found: 9,
                supported: 1
            })
        );
        assert!(matches!(
            rec(broken.id).launch,
            Err(DocError::Unparsable(_))
        ));
        assert!(rec(good.id).is_readable());
        assert!(matches!(
            db.game(newer.id),
            Err(crate::storage::StorageError::Document {
                doc: crate::storage::DocKind::Policy,
                ..
            })
        ));
        assert_eq!(db.game(good.id).unwrap().unwrap(), good);
    }
}

//! `executables`.

use std::path::Path;

use ormlite::Model;

use super::{
    Arg, ExecutableRepo, conn, dec_path, dec_path_opt, enc_path, enc_path_opt, exec_args, find,
    fingerprint_parts, hash32, id_parse, json_parse, json_text, not_found, ts, uint,
};
use crate::consts::MAX_EXECUTABLES_PER_GAME;
use crate::ids::{ExecutableId, GameId};
use crate::model::{ExeRole, Executable, Fingerprint, Sha256};
use crate::storage::db::SqlConn;
use crate::storage::path_codec;
use crate::storage::{StorageError, StorageResult, Store};

/// An `executables` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "executables")]
pub(super) struct ExeRow {
    #[ormlite(primary_key)]
    pub id: String,
    pub game_id: String,
    pub role: String,
    pub path: String,
    pub canonical_path: String,
    pub rel_path: Option<String>,
    pub mount_point: Option<String>,
    pub size: i64,
    pub mtime_ns: i64,
    pub quick_hash: Vec<u8>,
    pub full_hash: Option<Vec<u8>>,
    pub pe_json: String,
    pub args_override_json: Option<String>,
    pub label: Option<String>,
    pub added_at: i64,
}

impl ExeRow {
    pub(super) fn from_exe(game: GameId, e: &Executable) -> StorageResult<Self> {
        let (size, mtime_ns, quick_hash, full_hash) = fingerprint_parts(&e.fingerprint);
        Ok(Self {
            id: e.id.to_string(),
            game_id: game.to_string(),
            role: e.role.as_str().to_owned(),
            path: enc_path(&e.path)?,
            canonical_path: enc_path(&e.canonical_path)?,
            rel_path: e
                .rel_path
                .as_deref()
                .map(path_codec::encode_relative)
                .transpose()
                .map_err(|_| {
                    StorageError::Invalid(crate::model::ValidationError::new(
                        "executable.rel_path",
                        crate::model::Problem::InvalidFormat,
                    ))
                })?,
            mount_point: enc_path_opt(e.mount_point.as_deref())?,
            size,
            mtime_ns,
            quick_hash,
            full_hash,
            pe_json: json_text(&e.pe)?,
            args_override_json: e.args_override.as_ref().map(json_text).transpose()?,
            label: e.label.clone(),
            added_at: e.added_at.as_millis(),
        })
    }

    pub(super) fn into_exe(self) -> StorageResult<(GameId, Executable)> {
        let game: GameId = id_parse("executables.game_id", &self.game_id)?;
        let exe = Executable {
            id: id_parse("executables.id", &self.id)?,
            role: ExeRole::parse(&self.role)
                .ok_or_else(|| StorageError::Decode(format!("executables.role {:?}", self.role)))?,
            path: dec_path(&self.path)?,
            canonical_path: dec_path(&self.canonical_path)?,
            rel_path: self
                .rel_path
                .as_deref()
                .map(path_codec::decode_relative)
                .transpose()
                .map_err(super::path_err)?,
            mount_point: dec_path_opt(self.mount_point.as_deref())?,
            fingerprint: Fingerprint {
                size: uint(self.size),
                mtime_ns: self.mtime_ns,
                quick_hash: hash32("executables.quick_hash", &self.quick_hash)?,
                full_hash: self
                    .full_hash
                    .as_deref()
                    .map(|h| hash32("executables.full_hash", h))
                    .transpose()?,
            },
            pe: json_parse("executables.pe_json", &self.pe_json)?,
            args_override: self
                .args_override_json
                .as_deref()
                .map(|j| json_parse("executables.args_override_json", j))
                .transpose()?,
            label: self.label,
            added_at: ts(self.added_at),
        };
        Ok((game, exe))
    }
}

/// Executables of one game, primary first.
pub(super) async fn fetch_of(c: &mut SqlConn, game: GameId) -> StorageResult<Vec<Executable>> {
    let rows = ExeRow::select()
        .where_bind("game_id = ?", game.to_string())
        .fetch_all(&mut *c)
        .await?;
    let mut out = rows
        .into_iter()
        .map(|r| r.into_exe().map(|(_, e)| e))
        .collect::<StorageResult<Vec<_>>>()?;
    super::games::sort_exes(&mut out);
    Ok(out)
}

/// Every executable (one scan for the library snapshot).
pub(super) async fn fetch_all(c: &mut SqlConn) -> StorageResult<Vec<(GameId, Executable)>> {
    ExeRow::query("SELECT * FROM executables")
        .fetch_all(&mut *c)
        .await?
        .into_iter()
        .map(ExeRow::into_exe)
        .collect()
}

async fn fetch_one(c: &mut SqlConn, id: ExecutableId) -> StorageResult<Option<ExeRow>> {
    find(c, "id = ?", id.to_string()).await
}

async fn count_of(c: &mut SqlConn, game: GameId) -> StorageResult<usize> {
    let (n,): (i64,) = ormlite::query_as("SELECT count(*) FROM executables WHERE game_id = ?")
        .bind(game.to_string())
        .fetch_one(&mut *c)
        .await?;
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

impl<S: Store + ?Sized> ExecutableRepo for S {
    fn add_executable(&mut self, game: GameId, exe: &Executable) -> StorageResult<()> {
        exe.validate()?;
        let row = ExeRow::from_exe(game, exe)?;
        conn(self).atomic(async move |c| {
            if count_of(c, game).await? >= MAX_EXECUTABLES_PER_GAME {
                return Err(StorageError::Limit {
                    what: "executables per game",
                    max: MAX_EXECUTABLES_PER_GAME,
                });
            }
            row.insert(&mut *c).await?;
            Ok(())
        })
    }

    fn update_executable(&mut self, exe: &Executable) -> StorageResult<()> {
        exe.validate()?;
        let id = exe.id;
        let exe = exe.clone();
        conn(self).atomic(async move |c| {
            let old = fetch_one(c, id)
                .await?
                .ok_or_else(|| not_found("executable", id))?;
            let game: GameId = id_parse("executables.game_id", &old.game_id)?;
            let row = ExeRow::from_exe(game, &exe)?;
            row.update_all_fields(&mut *c).await?;
            Ok(())
        })
    }

    fn remove_executable(&mut self, id: ExecutableId) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            let Some(row) = fetch_one(c, id).await? else {
                return Ok(false);
            };
            if row.role == ExeRole::Primary.as_str() {
                return Err(StorageError::Invalid(crate::model::ValidationError::new(
                    "game.executables",
                    crate::model::Problem::ExactlyOnePrimary { found: 0 },
                )));
            }
            row.delete(&mut *c).await?;
            Ok(true)
        })
    }

    fn executable(&mut self, id: ExecutableId) -> StorageResult<Option<(GameId, Executable)>> {
        conn(self).run(async move |c| fetch_one(c, id).await?.map(ExeRow::into_exe).transpose())
    }

    fn executables_of(&mut self, game: GameId) -> StorageResult<Vec<Executable>> {
        conn(self).run(async move |c| fetch_of(c, game).await)
    }

    fn executable_by_canonical_path(
        &mut self,
        path: &Path,
    ) -> StorageResult<Option<(GameId, Executable)>> {
        let key = enc_path(path)?;
        conn(self).run(async move |c| {
            find::<ExeRow, _>(c, "canonical_path = ?", key)
                .await?
                .map(ExeRow::into_exe)
                .transpose()
        })
    }

    fn executables_by_quick_hash(
        &mut self,
        hash: &Sha256,
    ) -> StorageResult<Vec<(GameId, Executable)>> {
        let key = hash.to_vec();
        conn(self).run(async move |c| {
            ExeRow::query(
                "SELECT e.* FROM executables e JOIN games g ON g.id = e.game_id \
                 WHERE e.quick_hash = ? AND g.deleted_at IS NULL ORDER BY e.added_at, e.id",
            )
            .bind(key)
            .fetch_all(&mut *c)
            .await?
            .into_iter()
            .map(ExeRow::into_exe)
            .collect()
        })
    }

    fn set_primary_executable(&mut self, game: GameId, exe: ExecutableId) -> StorageResult<()> {
        conn(self).atomic(async move |c| {
            let row = fetch_one(c, exe)
                .await?
                .filter(|r| r.game_id == game.to_string())
                .ok_or_else(|| not_found("executable", exe))?;
            if row.role == ExeRole::Primary.as_str() {
                return Ok(());
            }
            exec_args(
                c,
                "UPDATE executables SET role = 'alternate' WHERE game_id = ? AND role = 'primary'",
                &[Arg::text(game.to_string())],
            )
            .await?;
            exec_args(
                c,
                "UPDATE executables SET role = 'primary' WHERE id = ?",
                &[Arg::text(exe.to_string())],
            )
            .await?;
            Ok(())
        })
    }
}

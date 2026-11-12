//! `runtimes`, `slr_installs`, `runtime_audit`, `runtime_events`.

use ormlite::Model;

use super::{
    Arg, AuditState, RuntimeEvent, RuntimeRepo, SlrInstall, conn, dec_path, enc_path, enum_parse,
    exec_args, hash32, int, ms_opt, runtime_name, ts, ts_opt, uint,
};
use crate::clock::Timestamp;
use crate::ids::RuntimeName;
use crate::model::{RuntimeBuild, SlrVariant};
use crate::storage::db::{SqlConn, map_orm, map_sqlx};
use crate::storage::{StorageError, StorageResult, Store};

/// A `runtimes` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "runtimes")]
struct RuntimeRow {
    #[ormlite(primary_key)]
    name: String,
    family: String,
    version_key: String,
    origin: String,
    origin_label: String,
    path: String,
    required_slr: String,
    display_name: String,
    has_fixes: bool,
    size_bytes: Option<i64>,
    discovered_at: i64,
    last_used_at: Option<i64>,
}

impl RuntimeRow {
    fn from_build(b: &RuntimeBuild) -> StorageResult<Self> {
        Ok(Self {
            name: b.name.to_string(),
            family: b.family.as_str().to_owned(),
            version_key: b.version_key.clone(),
            origin: b.origin.as_str().to_owned(),
            origin_label: b.origin_label.clone(),
            path: enc_path(&b.path)?,
            required_slr: b.required_slr.as_str().to_owned(),
            display_name: b.display_name.clone(),
            has_fixes: b.has_fixes,
            size_bytes: b.size_bytes.map(int),
            discovered_at: b.discovered_at.as_millis(),
            last_used_at: ms_opt(b.last_used_at),
        })
    }

    fn into_build(self) -> StorageResult<RuntimeBuild> {
        Ok(RuntimeBuild {
            name: runtime_name(&self.name)?,
            family: enum_parse("runtimes.family", &self.family)?,
            version_key: self.version_key,
            origin: enum_parse("runtimes.origin", &self.origin)?,
            origin_label: self.origin_label,
            path: dec_path(&self.path)?,
            required_slr: variant(&self.required_slr)?,
            display_name: self.display_name,
            has_fixes: self.has_fixes,
            size_bytes: self.size_bytes.map(uint),
            discovered_at: ts(self.discovered_at),
            last_used_at: ts_opt(self.last_used_at),
        })
    }
}

/// A `slr_installs` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "slr_installs")]
struct SlrRow {
    #[ormlite(primary_key)]
    variant: String,
    version: String,
    path: String,
    platform_dir: String,
    manifest_tree_sha256: Option<Vec<u8>>,
    manifest_built_at: Option<i64>,
    size_bytes: Option<i64>,
}

/// A `runtime_audit` row.
#[derive(Model, Clone, Debug, PartialEq)]
#[ormlite(table = "runtime_audit")]
struct AuditRow {
    #[ormlite(primary_key)]
    variant: String,
    pending: bool,
    last_run_at: Option<i64>,
    last_clean_at: Option<i64>,
    last_result: Option<String>,
}

fn variant(s: &str) -> StorageResult<SlrVariant> {
    SlrVariant::parse(s).ok_or_else(|| StorageError::Decode(format!("SLR variant {s:?}")))
}

/// `platform_dir` is a directory name relative to the SLR directory; stored verbatim when
/// it is valid UTF-8 (it always is for Valve's runtimes).
fn platform_text(p: &std::path::Path) -> StorageResult<String> {
    p.to_str().map(str::to_owned).ok_or_else(|| {
        StorageError::Invalid(crate::model::ValidationError::new(
            "slr.platform_dir",
            crate::model::Problem::InvalidCharacters,
        ))
    })
}

async fn fetch_audit(c: &mut SqlConn, v: SlrVariant) -> StorageResult<Option<AuditRow>> {
    AuditRow::select()
        .where_bind("variant = ?", v.as_str())
        .fetch_optional(&mut *c)
        .await
        .map_err(map_orm)
}

impl<S: Store + ?Sized> RuntimeRepo for S {
    fn replace_runtime_catalog(&mut self, builds: &[RuntimeBuild]) -> StorageResult<()> {
        let rows = builds
            .iter()
            .map(RuntimeRow::from_build)
            .collect::<StorageResult<Vec<_>>>()?;
        conn(self).atomic(async move |c| {
            exec_args(c, "DELETE FROM runtimes", &[]).await?;
            for r in rows {
                r.insert(&mut *c).await.map_err(map_orm)?;
            }
            Ok(())
        })
    }

    fn apply_runtime_changes(
        &mut self,
        upsert: &[RuntimeBuild],
        remove: &[RuntimeName],
    ) -> StorageResult<()> {
        let rows = upsert
            .iter()
            .map(RuntimeRow::from_build)
            .collect::<StorageResult<Vec<_>>>()?;
        let remove: Vec<String> = remove.iter().map(ToString::to_string).collect();
        conn(self).atomic(async move |c| {
            for name in remove.iter().chain(rows.iter().map(|r| &r.name)) {
                exec_args(
                    c,
                    "DELETE FROM runtimes WHERE name = ?",
                    &[Arg::text(name.clone())],
                )
                .await?;
            }
            for r in rows {
                r.insert(&mut *c).await.map_err(map_orm)?;
            }
            Ok(())
        })
    }

    fn runtimes(&mut self) -> StorageResult<Vec<RuntimeBuild>> {
        conn(self).run(async |c| {
            RuntimeRow::query("SELECT * FROM runtimes ORDER BY family, version_key DESC, name")
                .fetch_all(&mut *c)
                .await
                .map_err(map_sqlx)?
                .into_iter()
                .map(RuntimeRow::into_build)
                .collect()
        })
    }

    fn runtime(&mut self, name: &RuntimeName) -> StorageResult<Option<RuntimeBuild>> {
        let key = name.to_string();
        conn(self).run(async move |c| {
            RuntimeRow::select()
                .where_bind("name = ?", key)
                .fetch_optional(&mut *c)
                .await
                .map_err(map_orm)?
                .map(RuntimeRow::into_build)
                .transpose()
        })
    }

    fn set_runtime_last_used(&mut self, name: &RuntimeName, at: Timestamp) -> StorageResult<bool> {
        let args = [Arg::int(at.as_millis()), Arg::text(name.to_string())];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "UPDATE runtimes SET last_used_at = ? WHERE name = ?",
                &args,
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn put_slr_install(&mut self, i: &SlrInstall) -> StorageResult<()> {
        let row = SlrRow {
            variant: i.variant.as_str().to_owned(),
            version: i.version.clone(),
            path: enc_path(&i.path)?,
            platform_dir: platform_text(&i.platform_dir)?,
            manifest_tree_sha256: i.manifest_tree_sha256.map(|h| h.to_vec()),
            manifest_built_at: ms_opt(i.manifest_built_at),
            size_bytes: i.size_bytes.map(int),
        };
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "DELETE FROM slr_installs WHERE variant = ?",
                &[Arg::text(row.variant.clone())],
            )
            .await?;
            row.insert(&mut *c).await.map_err(map_orm)?;
            Ok(())
        })
    }

    fn slr_installs(&mut self) -> StorageResult<Vec<SlrInstall>> {
        conn(self).run(async |c| {
            SlrRow::query("SELECT * FROM slr_installs ORDER BY variant")
                .fetch_all(&mut *c)
                .await
                .map_err(map_sqlx)?
                .into_iter()
                .map(|r| {
                    Ok(SlrInstall {
                        variant: variant(&r.variant)?,
                        version: r.version,
                        path: dec_path(&r.path)?,
                        platform_dir: r.platform_dir.into(),
                        manifest_tree_sha256: r
                            .manifest_tree_sha256
                            .as_deref()
                            .map(|h| hash32("slr_installs.manifest_tree_sha256", h))
                            .transpose()?,
                        manifest_built_at: ts_opt(r.manifest_built_at),
                        size_bytes: r.size_bytes.map(uint),
                    })
                })
                .collect()
        })
    }

    fn delete_slr_install(&mut self, v: SlrVariant) -> StorageResult<bool> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "DELETE FROM slr_installs WHERE variant = ?",
                &[Arg::text(v.as_str())],
            )
            .await
            .map(|n| n > 0)
        })
    }

    fn audit_state(&mut self, v: SlrVariant) -> StorageResult<Option<AuditState>> {
        conn(self).run(async move |c| {
            fetch_audit(c, v)
                .await?
                .map(|r| {
                    Ok(AuditState {
                        variant: variant(&r.variant)?,
                        pending: r.pending,
                        last_run_at: ts_opt(r.last_run_at),
                        last_clean_at: ts_opt(r.last_clean_at),
                        last_result: r.last_result,
                    })
                })
                .transpose()
        })
    }

    fn set_audit_pending(&mut self, v: SlrVariant) -> StorageResult<()> {
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO runtime_audit(variant, pending) VALUES (?, 1) \
                 ON CONFLICT(variant) DO UPDATE SET pending = 1",
                &[Arg::text(v.as_str())],
            )
            .await
            .map(|_| ())
        })
    }

    fn record_audit_result(
        &mut self,
        v: SlrVariant,
        at: Timestamp,
        result: &str,
    ) -> StorageResult<()> {
        if !matches!(result, "clean" | "tampered" | "missing") {
            return Err(StorageError::Invalid(crate::model::ValidationError::new(
                "runtime_audit.last_result",
                crate::model::Problem::InvalidFormat,
            )));
        }
        let clean = (result == "clean").then_some(at.as_millis());
        let args = [
            Arg::text(v.as_str()),
            Arg::int(at.as_millis()),
            Arg::Int(clean),
            Arg::text(result),
        ];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO runtime_audit(variant, pending, last_run_at, last_clean_at, last_result) \
                 VALUES (?1, 0, ?2, ?3, ?4) ON CONFLICT(variant) DO UPDATE SET pending = 0, \
                 last_run_at = ?2, last_clean_at = coalesce(?3, last_clean_at), last_result = ?4",
                &args,
            )
            .await
            .map(|_| ())
        })
    }

    fn add_runtime_event(&mut self, e: &RuntimeEvent) -> StorageResult<i64> {
        let args = [
            Arg::int(e.at.as_millis()),
            Arg::Text(e.variant.map(|v| v.as_str().to_owned())),
            Arg::text(e.kind.clone()),
            Arg::text(e.summary_json.clone()),
        ];
        conn(self).atomic(async move |c| {
            exec_args(
                c,
                "INSERT INTO runtime_events(at, variant, kind, summary_json) VALUES (?, ?, ?, ?)",
                &args,
            )
            .await?;
            let (id,): (i64,) = ormlite::query_as("SELECT last_insert_rowid()")
                .fetch_one(&mut *c)
                .await
                .map_err(map_sqlx)?;
            Ok(id)
        })
    }

    fn runtime_events(&mut self, limit: u64) -> StorageResult<Vec<RuntimeEvent>> {
        conn(self).run(async move |c| {
            let rows: Vec<(i64, i64, Option<String>, String, String)> = ormlite::query_as(
                "SELECT id, at, variant, kind, summary_json FROM runtime_events \
                 ORDER BY id DESC LIMIT ?",
            )
            .bind(int(limit))
            .fetch_all(&mut *c)
            .await
            .map_err(map_sqlx)?;
            rows.into_iter()
                .map(|(id, at, v, kind, summary_json)| {
                    Ok(RuntimeEvent {
                        id,
                        at: ts(at),
                        variant: v.as_deref().map(variant).transpose()?,
                        kind,
                        summary_json,
                    })
                })
                .collect()
        })
    }
}

//! The launch inspector's data (DEVELOPER_MODE §3, §8; DEV-001).
//!
//! [`InspectorData`] carries the real structures the launcher executes, never a
//! reconstruction: for the **next launch** the dry run's [`LaunchPlan`] together with the
//! [`SandboxPlan`] it was rendered from (mount sections and reasons, environment origins);
//! for an **active or past session** the plan file the supervisor executed (kept as
//! `plan.json` in the session's log folder, or still in its runtime folder), its
//! `events.jsonl` timeline and the session row. Rows for the tables ([`mount_rows`],
//! [`env_rows`], [`raw_args`]) are derived here so the UI only draws.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::plan_file::{self, LaunchPlan, group_args};
use crate::fsx::Fs;
use crate::model::Session;
use crate::paths::AppPaths;
use crate::sandbox::{AccessStatement, MountKind, MountMode, SandboxPlan};

/// Largest `events.jsonl` read (its head).
pub const MAX_EVENTS_BYTES: usize = 4 * 1024 * 1024;
/// File name of the plan copy kept with the session's logs.
pub const PLAN_COPY: &str = "plan.json";

/// One `events.jsonl` line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineEntry {
    /// Milliseconds since the session started.
    pub t_ms: u64,
    /// `state`, `phase`, `status`, `stop`, `error`, …
    pub kind: String,
    /// The other fields, compact (`phase=launching`).
    pub detail: String,
}

/// Parse `events.jsonl` (lines that are not JSON objects are skipped).
pub fn parse_events(text: &str) -> Vec<TimelineEntry> {
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(l).ok())
        .map(|m| {
            let t_ms = m.get("t").and_then(serde_json::Value::as_u64).unwrap_or(0);
            let kind = m
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("event")
                .to_owned();
            let detail = m
                .iter()
                .filter(|(k, v)| *k != "t" && *k != "kind" && !v.is_null())
                .map(|(k, v)| match v {
                    serde_json::Value::String(s) => format!("{k}={s}"),
                    other => format!("{k}={other}"),
                })
                .collect::<Vec<_>>()
                .join(" ");
            TimelineEntry { t_ms, kind, detail }
        })
        .collect()
}

/// Which launch the inspector shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InspectorSource {
    /// A dry run of the next launch.
    DryRun,
    /// A session that is still open.
    Active,
    /// An ended session.
    Past,
}

/// Everything the inspector sheet shows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InspectorData {
    /// Where the data comes from.
    pub source: InspectorSource,
    /// The plan as executed (or, for a dry run, as it would be).
    pub plan: LaunchPlan,
    /// The sandbox plan it was rendered from (dry runs only: sessions keep the rendering).
    pub sandbox: Option<SandboxPlan>,
    /// The access statements of the policy (dry runs).
    pub explain: Vec<AccessStatement>,
    /// `events.jsonl` (sessions).
    pub timeline: Vec<TimelineEntry>,
    /// The session row (sessions).
    pub session: Option<Session>,
    /// Planner and dry-run notes ("This is the plan of the preparation step").
    pub notes: Vec<String>,
}

/// Why a session's plan cannot be shown.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum InspectError {
    /// No plan file is left (logs removed, or a session of an older version).
    #[error("the launch plan of this session is not available")]
    NoPlan,
    /// The plan file has a format this build does not read.
    #[error("the launch plan was written by another version")]
    OtherVersion,
}

/// Where the plan of `session` may be, best first.
pub fn plan_locations(paths: &AppPaths, session: &Session) -> Vec<PathBuf> {
    vec![
        paths.session_dir(session.id).join(PLAN_COPY),
        paths.plan_file(session.id),
    ]
}

/// Load the inspector data of a session (active or past).
pub fn load_session(
    fs: &dyn Fs,
    paths: &AppPaths,
    session: &Session,
) -> Result<InspectorData, InspectError> {
    let mut plan = None;
    let mut other_version = false;
    for p in plan_locations(paths, session) {
        match plan_file::read(fs, &p) {
            Ok(pl) => {
                plan = Some(pl);
                break;
            }
            Err(plan_file::PlanFileError::UnknownFormat { .. }) => other_version = true,
            Err(_) => {}
        }
    }
    let plan = match plan {
        Some(p) => p,
        None if other_version => return Err(InspectError::OtherVersion),
        None => return Err(InspectError::NoPlan),
    };
    let events = fs
        .metadata(&paths.session_dir(session.id).join("events.jsonl"))
        .ok()
        .and_then(|_| {
            fs.read_range(
                &paths.session_dir(session.id).join("events.jsonl"),
                0,
                MAX_EVENTS_BYTES,
            )
            .ok()
        })
        .map(|b| parse_events(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    Ok(InspectorData {
        source: if session.is_open() {
            InspectorSource::Active
        } else {
            InspectorSource::Past
        },
        plan,
        sandbox: None,
        explain: Vec::new(),
        timeline: events,
        session: Some(session.clone()),
        notes: Vec::new(),
    })
}

/// One row of the *Sandbox* mounts table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MountRow {
    /// Position in the argument list (1-based).
    pub index: usize,
    /// `bind`, `ro-bind`, `tmpfs`, …
    pub kind: String,
    /// Host source.
    pub source: String,
    /// Sandbox destination.
    pub destination: String,
    /// `rw`, `ro` or empty.
    pub mode: &'static str,
    /// Planner section (dry runs).
    pub section: Option<String>,
    /// Why it exists (dry runs).
    pub reason: Option<String>,
    /// It implements one of the user's choices (a policy field).
    pub from_choice: bool,
}

impl MountRow {
    /// Writable from inside the sandbox.
    pub fn writable(&self) -> bool {
        self.mode == "rw"
    }
}

fn mode_of(kind: &MountKind) -> &'static str {
    match kind {
        MountKind::Bind {
            mode: MountMode::Rw,
            ..
        }
        | MountKind::DevBind { .. }
        | MountKind::Tmpfs { .. }
        | MountKind::OverlayRw { .. }
        | MountKind::OverlayTmp { .. } => "rw",
        MountKind::Bind { .. } | MountKind::Symlink { .. } | MountKind::FileData { .. } => "ro",
        _ => "",
    }
}

fn shown(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// The mounts table: from the sandbox plan when there is one (with sections and reasons),
/// otherwise from the rendered options.
pub fn mount_rows(data: &InspectorData) -> Vec<MountRow> {
    if let Some(sb) = &data.sandbox {
        return sb
            .mounts
            .iter()
            .enumerate()
            .map(|(i, m)| MountRow {
                index: i + 1,
                kind: m.kind.label().to_owned(),
                source: match &m.kind {
                    MountKind::Symlink { target, .. } => shown(target),
                    MountKind::FileData { fd, .. } => format!("descriptor {fd}"),
                    k => k.src().map(shown).unwrap_or_default(),
                },
                destination: shown(m.kind.dst()),
                mode: mode_of(&m.kind),
                section: Some(format!("{:?}", m.origin.section).to_lowercase()),
                reason: Some(m.origin.reason.to_string()),
                from_choice: m.origin.policy_field.is_some(),
            })
            .collect();
    }
    let mut out = Vec::new();
    for g in group_args(&data.plan.sandbox.args) {
        // Drop the `--perms N` / `--size N` modifiers.
        let mut core: Vec<String> = Vec::new();
        let mut it = g.iter().map(|a| a.to_string_lossy().into_owned());
        while let Some(a) = it.next() {
            if a == "--perms" || a == "--size" {
                it.next();
                continue;
            }
            core.push(a);
        }
        let (kind, src, dst) = match core.as_slice() {
            [o, s, d] if o.contains("bind") || o == "--symlink" => {
                (o.clone(), s.clone(), d.clone())
            }
            [o, fd, d] if o == "--ro-bind-data" || o == "--file" || o == "--bind-data" => {
                (o.clone(), format!("descriptor {fd}"), d.clone())
            }
            [o, d] if matches!(o.as_str(), "--tmpfs" | "--dir" | "--proc" | "--dev") => {
                (o.clone(), String::new(), d.clone())
            }
            _ => continue,
        };
        let k = kind.trim_start_matches("--").to_owned();
        let mode = if k.starts_with("ro") || k == "symlink" || k == "file" {
            "ro"
        } else if k.starts_with("bind") || k.starts_with("dev-bind") || k == "tmpfs" {
            "rw"
        } else {
            ""
        };
        out.push(MountRow {
            index: out.len() + 1,
            kind: k,
            source: src,
            destination: dst,
            mode,
            section: None,
            reason: None,
            from_choice: false,
        });
    }
    out
}

/// One row of the *Environment* table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnvRow {
    /// Name.
    pub name: String,
    /// Value.
    pub value: String,
    /// `fixed`, `host`, `option:<name>`, `global`, `game`, `exception`; `set` when only the
    /// rendering is known.
    pub origin: String,
}

/// The environment table.
pub fn env_rows(data: &InspectorData) -> Vec<EnvRow> {
    use crate::sandbox::plan::EnvOrigin;
    if let Some(sb) = &data.sandbox {
        return sb
            .env
            .set
            .iter()
            .map(|(name, e)| EnvRow {
                name: name.clone(),
                value: e.value.clone(),
                origin: match &e.origin {
                    EnvOrigin::Fixed => "fixed".to_owned(),
                    EnvOrigin::Host => "host".to_owned(),
                    EnvOrigin::Option(o) => format!("option:{o}"),
                    EnvOrigin::Global => "global".to_owned(),
                    EnvOrigin::Game => "game".to_owned(),
                    EnvOrigin::Exception => "exception".to_owned(),
                },
            })
            .collect();
    }
    let mut rows: Vec<EnvRow> = group_args(&data.plan.sandbox.args)
        .into_iter()
        .filter(|g| g.first().is_some_and(|o| o == "--setenv"))
        .filter_map(|g| match g.as_slice() {
            [_, n, v] => Some(EnvRow {
                name: n.to_string_lossy().into_owned(),
                value: v.to_string_lossy().into_owned(),
                origin: "set".to_owned(),
            }),
            _ => None,
        })
        .collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    rows
}

/// *Raw*: the exact vector passed through `--args`, one entry per line with its index.
pub fn raw_args(plan: &LaunchPlan) -> Vec<(usize, String)> {
    plan.sandbox
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| (i, a.to_string_lossy().into_owned()))
        .collect()
}

/// *Command*: the `umu-run` path and arguments after the init shim's `--`.
pub fn umu_command(plan: &LaunchPlan) -> Vec<String> {
    let cmd = &plan.sandbox.command;
    let start = cmd.iter().position(|a| a == "--").map_or(0, |i| i + 1);
    cmd[start..]
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_parse_and_skip_garbage() {
        let t = parse_events(
            "{\"t\":12,\"kind\":\"state\",\"state\":\"starting\"}\nnot json\n{\"t\":40,\"kind\":\"phase\",\"phase\":\"launching\",\"code\":null}\n",
        );
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].t_ms, 12);
        assert_eq!(t[0].detail, "state=starting");
        assert_eq!(t[1].kind, "phase");
        assert_eq!(t[1].detail, "phase=launching");
    }
}

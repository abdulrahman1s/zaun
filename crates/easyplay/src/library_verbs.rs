//! Headless `add`, `remove`, `list` and `show` (CLI-002, CLI-003 partial).
//!
//! Mutating verbs run the same `service` flows as the GUI on a headless AppService, under
//! `library.lock` held exclusively (PERSISTENCE §6: with a GUI running the lock is held
//! shared and the verb answers "busy"; forwarding arrives with DESKTOP-002). Prompts are
//! not asked on the terminal yet: the flow is cancelled and the question printed.
//! Read-only verbs use a read-only connection and change nothing.

use std::path::Path;
use std::time::Duration;

use easyplay_core::clock::{Clock, SystemClock};
use easyplay_core::error::{Error, ErrorCode, UserError};
use easyplay_core::fsx::{Fs, RealFs};
use easyplay_core::ids::{ExecutableId, GameId};
use easyplay_core::launch::LaunchState;
use easyplay_core::library::index::build as build_snapshot;
use easyplay_core::library::{DefaultRuntimeInfo, LibrarySnapshot, Overlays, RemoveOptions};
use easyplay_core::model::{Game, GameSummary, PromptAnswer};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::sandbox::presets;
use easyplay_core::service::{AfterCreate, AppService, Command, Event, Notice, OpenOrigin};
use easyplay_core::storage::lock::{LibraryLock, LockMode};
use easyplay_core::storage::{Db, GameRepo, ReadDb, SessionRepo, StorageError, TagRepo};

use crate::cli::{PresetArg, exit};
use crate::wiring;

/// How long a headless flow may take (draft, create, hand-off).
const FLOW_DEADLINE: Duration = Duration::from_secs(120);

fn lock_exclusive(paths: &AppPaths) -> Result<LibraryLock, Error> {
    RealFs::new().create_dir_all(&paths.data_dir, 0o700)?;
    LibraryLock::try_acquire(&paths.library_lock, LockMode::Exclusive).map_err(|e| match e {
        StorageError::LockHeld => Error::new(ErrorCode::LibBusy).with(
            "detail",
            "EasyPlay is running; use the window (or close it)",
        ),
        other => other.into(),
    })
}

fn headless() -> Result<(AppService, std::sync::mpsc::Receiver<Event>, LibraryLock), Error> {
    let mut w = wiring::build()?;
    let lock = lock_exclusive(&w.paths)?;
    let mut deps = w.take_deps()?;
    // Desktop integration is the GUI's (shortcuts follow the library when it next starts).
    deps.desktop = None;
    let (svc, events) = AppService::open_headless(deps)?;
    Ok((svc, events, lock))
}

#[allow(clippy::print_stderr)]
fn print_user_error(u: &UserError) {
    eprintln!("easyplay: {}. {} [{}]", u.title, u.message, u.code);
    if let Some(d) = &u.details
        && !d.is_empty()
    {
        eprintln!("  {}", d.replace('\n', "\n  "));
    }
}

/// Pump the headless service until `done` says the flow ended; returns its exit code.
fn drive(
    svc: &mut AppService,
    events: &std::sync::mpsc::Receiver<Event>,
    mut done: impl FnMut(&mut AppService, Event) -> Option<u8>,
) -> u8 {
    let clock = SystemClock::new();
    let start = clock.mono();
    loop {
        for ev in events.try_iter().collect::<Vec<_>>() {
            if let Some(code) = done(svc, ev) {
                return code;
            }
        }
        if clock.mono().saturating_duration_since(start) > FLOW_DEADLINE {
            return exit::FAILED;
        }
        svc.pump(Duration::from_millis(200));
    }
}

/// `easyplay add FILE [--name N] [--preset P] [--no-launch]`.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn add(
    file: &Path,
    name: Option<&str>,
    preset: Option<PresetArg>,
    no_launch: bool,
) -> Result<u8, Error> {
    let file = std::path::absolute(file)
        .map_err(|e| Error::new(ErrorCode::FsReadDenied).with_source(e))?;
    let (mut svc, events, _lock) = headless()?;
    svc.handle(Command::OpenPaths {
        paths: vec![file],
        origin: OpenOrigin::Picker,
    });
    let code = drive(&mut svc, &events, |svc, ev| match ev {
        Event::SetupRequested { mut draft, .. } => {
            if let Some(n) = name {
                draft.set_name(n);
            }
            if let Some(p) = preset {
                let base = match p {
                    PresetArg::Offline => easyplay_core::model::BasePreset::Offline,
                    PresetArg::Online => easyplay_core::model::BasePreset::Online,
                    PresetArg::Standard => easyplay_core::model::BasePreset::Standard,
                };
                let facts = svc.hub().host.load();
                let mut policy = match facts.as_ref() {
                    Some(h) => presets::preset_policy(base, h),
                    None => presets::preset_policy(
                        base,
                        &easyplay_core::host::HostFacts::empty(SystemClock::new().now()),
                    ),
                };
                policy.game_dir = draft.policy.game_dir;
                policy.save_data = draft.policy.save_data;
                draft.policy = policy;
            }
            if draft.is_blocked() {
                for i in &draft.issues {
                    eprintln!("{} {:?}: {}", i.code.as_str(), i.severity, i.message);
                }
                return Some(exit::FAILED);
            }
            svc.handle(Command::CreateGame {
                draft,
                then: if no_launch {
                    AfterCreate::Nothing
                } else {
                    AfterCreate::Launch
                },
            });
            None
        }
        Event::Notice(Notice::GameAdded { game, name }) => {
            println!("added {name} ({game})");
            no_launch.then_some(exit::OK)
        }
        Event::Notice(Notice::AlreadyInLibrary { game }) => {
            let name = svc
                .hub()
                .library
                .load()
                .get(game)
                .map(|g| g.name.to_string())
                .unwrap_or_default();
            println!("already in the library: {name} ({game})");
            Some(exit::OK)
        }
        Event::Prompt(p) => {
            eprintln!(
                "easyplay: this needs a decision in the EasyPlay window: {:?}",
                p.kind
            );
            svc.handle(Command::AnswerPrompt {
                prompt: p.id,
                answer: PromptAnswer::Cancel,
            });
            Some(exit::FAILED)
        }
        Event::Failure(u) => {
            print_user_error(&u);
            Some(exit::FAILED)
        }
        Event::LaunchProgress { state, .. } => match state {
            LaunchState::Starting { .. } => {
                println!("started");
                Some(exit::OK)
            }
            LaunchState::Failed(u) => {
                print_user_error(&u);
                Some(exit::FAILED)
            }
            _ => None,
        },
        _ => None,
    });
    Ok(code)
}

/// `easyplay remove GAME [--keep-storage]`.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn remove(game: &str, keep_storage: bool) -> Result<u8, Error> {
    let (mut svc, events, _lock) = headless()?;
    let snap = svc.hub().library.load();
    let id = resolve(&snap, game)?;
    svc.handle(Command::RemoveGame {
        id,
        options: RemoveOptions {
            delete_private_storage: !keep_storage,
            ..RemoveOptions::default()
        },
    });
    Ok(drive(&mut svc, &events, |svc, ev| match ev {
        Event::Notice(Notice::GameRemoved { name, saved_data }) => {
            println!("removed {name}");
            if let Some(p) = saved_data {
                println!("save data kept in {}", p.display());
            }
            Some(exit::OK)
        }
        Event::Prompt(p) => {
            eprintln!("easyplay: the game is running; stop it first (easyplay stop)");
            svc.handle(Command::AnswerPrompt {
                prompt: p.id,
                answer: PromptAnswer::Cancel,
            });
            Some(exit::BUSY)
        }
        Event::Failure(u) => {
            print_user_error(&u);
            Some(exit::FAILED)
        }
        _ => None,
    }))
}

/// The library as a snapshot, read-only.
pub(crate) fn read_snapshot() -> Result<(LibrarySnapshot, ReadDb, AppPaths), Error> {
    let env = PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    if !RealFs::new().exists(&paths.db_file) {
        return Err(Error::new(ErrorCode::LibInvalidField).with("detail", "the library is empty"));
    }
    let mut db = Db::open_ro(&paths.db_file)?;
    let rows = db.load_library()?;
    let snap = build_snapshot(
        &rows,
        &DefaultRuntimeInfo,
        &Overlays::default(),
        1,
        SystemClock::new().now(),
    );
    Ok((snap, db, paths))
}

/// `<GAME>`: an ID, an ID prefix of at least 6 characters, or a unique name
/// (CONFIGURATION §6; ambiguity lists the candidates).
pub(crate) fn resolve(snap: &LibrarySnapshot, arg: &str) -> Result<GameId, Error> {
    easyplay_core::library::resolve::resolve_game(snap.games.iter().map(|g| (g.id, &*g.name)), arg)
}

/// `--exe <LABEL|ID>`: a program of `game` by ID (or a unique prefix of at least 6
/// characters), label or file name, case-insensitively.
pub(crate) fn resolve_exe(game: &Game, arg: &str) -> Result<ExecutableId, Error> {
    let lower = arg.trim().to_lowercase();
    let hits: Vec<&easyplay_core::model::Executable> = game
        .executables
        .iter()
        .filter(|e| {
            let id = e.id.to_string();
            id == lower
                || (lower.len() >= 6 && id.starts_with(&lower))
                || e.label
                    .as_deref()
                    .is_some_and(|l| l.to_lowercase() == lower)
                || e.path
                    .file_name()
                    .is_some_and(|f| f.to_string_lossy().to_lowercase() == lower)
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok(one.id),
        [] => Err(Error::new(ErrorCode::LibInvalidField).with(
            "detail",
            format!(
                "{} has no program {arg:?}; its programs: {}",
                game.name,
                game.executables
                    .iter()
                    .map(|e| e.label.clone().unwrap_or_else(|| e
                        .path
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
        many => Err(Error::new(ErrorCode::LibInvalidField)
            .with("detail", format!("{arg:?} matches {} programs", many.len()))),
    }
}

fn hours(ms: u64) -> String {
    let m = ms / 60_000;
    if m >= 60 {
        format!("{}h {:02}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

/// `easyplay list [--json] [--running] [--tag TAG]`.
#[allow(clippy::print_stdout)]
pub fn list(json: bool, running: bool, tag: Option<&str>) -> Result<u8, Error> {
    let (snap, mut db, _) = match read_snapshot() {
        Ok(v) => v,
        Err(e) if e.code == ErrorCode::LibInvalidField => {
            if json {
                println!("{}", serde_json::json!({"format": 1, "games": []}));
            } else {
                println!("No games yet.");
            }
            return Ok(exit::OK);
        }
        Err(e) => return Err(e),
    };
    let open = db.open_sessions()?;
    let tag_id = tag.and_then(|t| {
        snap.tags
            .iter()
            .find(|x| x.name.eq_ignore_ascii_case(t))
            .map(|x| x.id)
    });
    if tag.is_some() && tag_id.is_none() {
        return Err(Error::new(ErrorCode::LibInvalidField).with("detail", "no such tag"));
    }
    let games: Vec<&GameSummary> = snap
        .games
        .iter()
        .filter(|g| !running || open.iter().any(|s| s.game_id == Some(g.id)))
        .filter(|g| tag_id.is_none_or(|t| g.tags.contains(&t)))
        .collect();
    if json {
        let v: Vec<serde_json::Value> = games
            .iter()
            .map(|g| {
                let session = open.iter().find(|s| s.game_id == Some(g.id));
                serde_json::json!({
                    "id": g.id.to_string(),
                    "name": &*g.name,
                    "exe": g.primary_exe_path.display().to_string(),
                    "root": g.root.as_ref().map(|r| r.display().to_string()),
                    "favorite": g.favorite,
                    "hidden": g.hidden,
                    "runtime": &*g.runtime_label,
                    "total_playtime_ms": g.total_playtime_ms,
                    "sessions": g.session_count,
                    "launches": g.launch_count,
                    "last_played_at": g.last_played_at.map(|t| t.as_millis()),
                    "running": session.map(|s| serde_json::json!({
                        "session": s.id.to_string(),
                        "state": s.state.as_str(),
                        "supervisor_pid": s.supervisor.as_ref().map(|v| v.pid),
                        "sandbox_pid": s.sandbox_pid,
                    })),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"format": 1, "games": v}))
                .unwrap_or_default()
        );
        return Ok(exit::OK);
    }
    if games.is_empty() {
        println!("No games.");
    }
    for g in games {
        let run = if open.iter().any(|s| s.game_id == Some(g.id)) {
            "  running"
        } else {
            ""
        };
        println!(
            "{}  {:<32} {:>8}  {}{}",
            g.id.short(),
            g.name,
            hours(g.total_playtime_ms),
            g.runtime_label,
            run
        );
    }
    Ok(exit::OK)
}

/// `easyplay show GAME [--json]`.
#[allow(clippy::print_stdout)]
pub fn show(arg: &str, json: bool) -> Result<u8, Error> {
    let (snap, mut db, paths) = read_snapshot()?;
    let id = resolve(&snap, arg)?;
    let game = db
        .game(id)?
        .ok_or_else(|| Error::new(ErrorCode::LibInvalidField).with("detail", "no such game"))?;
    let summary = snap.get(id);
    let sessions = db.session_history(Some(id), &[], 0, 10)?;
    if json {
        let v = serde_json::json!({
            "format": 1,
            "game": game,
            "paths": {
                "game_dir": paths.game_dir(id),
                "prefix": paths.prefix_dir(id),
                "home": paths.home_dir(id),
            },
            "stats": summary.map(|s| serde_json::json!({
                "total_playtime_ms": s.total_playtime_ms,
                "sessions": s.session_count,
                "launches": s.launch_count,
                "first_played_at": s.first_played_at.map(|t| t.as_millis()),
                "last_played_at": s.last_played_at.map(|t| t.as_millis()),
            })),
            "recent_sessions": sessions.iter().map(|s| serde_json::json!({
                "session": s.id.to_string(),
                "state": s.state.as_str(),
                "started_at": s.started_at.as_millis(),
                "duration_ms": s.duration_ms,
                "end_reason": s.end_reason.map(|r| r.as_str()),
                "end_detail": s.end_reason.and_then(|r| r.detail()),
                "exit_status": s.exit_status,
                "counted": s.counted,
            })).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(exit::OK);
    }
    println!("{} ({})", game.name, game.id);
    for e in &game.executables {
        println!("  {:?}: {}", e.role, e.path.display());
    }
    if let Some(r) = &game.root {
        println!("  folder: {}", r.display());
    }
    println!("  protection: {:?}", game.policy.preset);
    println!("  compatibility layer: {:?}", game.runtime);
    if let Some(s) = summary {
        println!(
            "  played {} in {} session(s), {} launch(es)",
            hours(s.total_playtime_ms),
            s.session_count,
            s.launch_count
        );
    }
    for s in &sessions {
        println!(
            "  session {}  {}  {}",
            s.id.short(),
            s.end_reason.map(|r| r.as_str()).unwrap_or(s.state.as_str()),
            s.duration_ms.map(hours).unwrap_or_default()
        );
    }
    Ok(exit::OK)
}

/// A file name from a game name (exports folder).
fn export_file_name(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect();
    let s = s.trim_matches('_');
    format!("{}.toml", if s.is_empty() { "game" } else { s })
}

/// `easyplay export GAME [-o FILE]`: read-only (works while the GUI runs).
#[allow(clippy::print_stdout)]
pub fn export(arg: &str, output: Option<&Path>) -> Result<u8, Error> {
    let (snap, mut db, paths) = read_snapshot()?;
    let id = resolve(&snap, arg)?;
    let game = db
        .game(id)?
        .ok_or_else(|| Error::new(ErrorCode::LibInvalidField).with("detail", "no such game"))?;
    let mine = db.tags_of(id)?;
    let tags: Vec<_> = db
        .tags_with_counts()?
        .into_iter()
        .filter(|(t, _)| mine.contains(&t.id))
        .map(|(t, _)| t.name)
        .collect();
    let text = easyplay_core::library::export::export(&game, &tags, SystemClock::new().now())?;
    let fs = RealFs::new();
    let out = match output {
        Some(p) if p.as_os_str() == "-" => {
            print!("{text}");
            return Ok(exit::OK);
        }
        Some(p) => std::path::absolute(p)
            .map_err(|e| Error::new(ErrorCode::FsWriteDenied).with_source(e))?,
        None => {
            fs.create_dir_all(&paths.exports_dir, 0o700)?;
            paths.exports_dir.join(export_file_name(&game.name))
        }
    };
    fs.atomic_write(&out, text.as_bytes(), 0o600)?;
    println!("exported {} to {}", game.name, out.display());
    Ok(exit::OK)
}

/// `easyplay import FILE --exe EXE`: apply exported settings to the game of `EXE`, adding it
/// first (with the setup defaults) when it is not in the library. Validated exactly like an
/// edit on the game page; folder grants with absolute paths are listed, never applied.
#[allow(clippy::print_stdout, clippy::print_stderr)]
pub fn import(file: &Path, exe: &Path) -> Result<u8, Error> {
    let fs = RealFs::new();
    let text = fs.read_follow(
        &std::path::absolute(file)
            .map_err(|e| Error::new(ErrorCode::FsReadDenied).with_source(e))?,
        easyplay_core::library::export::EXPORT_MAX_BYTES as u64,
    )?;
    let text = String::from_utf8(text).map_err(|_| {
        Error::new(ErrorCode::CfgInvalidValue).with("reason", "the file is not UTF-8 text")
    })?;
    // Validate before touching the library.
    easyplay_core::library::export::parse(&text)?;
    let exe = fs.canonicalize(
        &std::path::absolute(exe)
            .map_err(|e| Error::new(ErrorCode::FsReadDenied).with_source(e))?,
    )?;
    let (mut svc, events, _lock) = headless()?;
    let find = |svc: &mut AppService| {
        svc.hub()
            .library
            .load()
            .games
            .iter()
            .find(|g| *g.primary_exe_path == *exe)
            .map(|g| g.id)
    };
    let id = match find(&mut svc) {
        Some(id) => id,
        None => {
            svc.handle(Command::OpenPaths {
                paths: vec![exe.clone()],
                origin: OpenOrigin::Picker,
            });
            let mut added = None;
            let code = drive(&mut svc, &events, |svc, ev| match ev {
                Event::SetupRequested { draft, .. } => {
                    if draft.is_blocked() {
                        for i in &draft.issues {
                            eprintln!("{} {:?}: {}", i.code.as_str(), i.severity, i.message);
                        }
                        return Some(exit::FAILED);
                    }
                    svc.handle(Command::CreateGame {
                        draft,
                        then: AfterCreate::Nothing,
                    });
                    None
                }
                Event::Notice(Notice::GameAdded { game, .. })
                | Event::Notice(Notice::AlreadyInLibrary { game }) => {
                    added = Some(game);
                    Some(exit::OK)
                }
                Event::Prompt(p) => {
                    svc.handle(Command::AnswerPrompt {
                        prompt: p.id,
                        answer: PromptAnswer::Cancel,
                    });
                    Some(exit::FAILED)
                }
                Event::Failure(u) => {
                    print_user_error(&u);
                    Some(exit::FAILED)
                }
                _ => None,
            });
            match added {
                Some(id) if code == exit::OK => id,
                _ => return Ok(exit::FAILED),
            }
        }
    };
    let facts = svc
        .hub()
        .host
        .load()
        .as_ref()
        .clone()
        .unwrap_or_else(|| easyplay_core::host::HostFacts::empty(SystemClock::new().now()));
    let preset = |b| presets::preset_policy(b, &facts);
    let (settings, warnings) = svc.library().import_settings(id, &text, &preset, &[])?;
    println!("imported settings into {} ({id})", settings.name);
    for n in &settings.notes {
        println!("  note: {n:?}");
    }
    for w in &warnings {
        println!("  warning: {w:?}");
    }
    Ok(exit::OK)
}

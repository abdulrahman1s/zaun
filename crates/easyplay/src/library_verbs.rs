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
use easyplay_core::ids::GameId;
use easyplay_core::launch::LaunchState;
use easyplay_core::library::index::build as build_snapshot;
use easyplay_core::library::{DefaultRuntimeInfo, LibrarySnapshot, Overlays, RemoveOptions};
use easyplay_core::model::{GameSummary, PromptAnswer};
use easyplay_core::paths::{AppPaths, PathEnv};
use easyplay_core::sandbox::presets;
use easyplay_core::service::{AfterCreate, AppService, Command, Event, Notice, OpenOrigin};
use easyplay_core::storage::lock::{LibraryLock, LockMode};
use easyplay_core::storage::{Db, GameRepo, ReadDb, SessionRepo, StorageError};

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
    let (svc, events) = AppService::open_headless(w.take_deps()?)?;
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
fn read_snapshot() -> Result<(LibrarySnapshot, ReadDb, AppPaths), Error> {
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

/// `<GAME>`: an ID, an ID prefix of at least 6 characters, or a unique name.
fn resolve(snap: &LibrarySnapshot, arg: &str) -> Result<GameId, Error> {
    if let Ok(id) = arg.parse::<GameId>()
        && snap.get(id).is_some()
    {
        return Ok(id);
    }
    let lower = arg.to_lowercase();
    let m: Vec<&GameSummary> = snap
        .games
        .iter()
        .filter(|g| {
            (arg.len() >= 6 && g.id.to_string().starts_with(&lower))
                || g.name.to_lowercase() == lower
        })
        .collect();
    match m.as_slice() {
        [one] => Ok(one.id),
        [] => {
            Err(Error::new(ErrorCode::LibInvalidField).with("detail", format!("no game {arg:?}")))
        }
        many => Err(Error::new(ErrorCode::LibInvalidField)
            .with("detail", format!("{arg:?} matches {} games", many.len()))),
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

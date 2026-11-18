//! Steam games on the command line (ADR-0031, CONFIGURATION §6): `steam scan|on|off|update|
//! open`, and the Steam part of `list`, `show` and `launch`.
//!
//! Everything here reads Steam's files (nothing is written under a Steam root) and hands
//! requests to the Steam client detached (`session::RealHandoff`). `steam on|off` writes
//! `config.toml` and is refused while Zaun is open (its window owns the preferences).

use std::collections::BTreeMap;
use std::sync::Arc;

use zaun_core::clock::{Clock, SystemClock};
use zaun_core::config::PreferencePatch;
use zaun_core::desktop::instance::InstanceTransport;
use zaun_core::error::{Error, ErrorCode};
use zaun_core::fsx::{Fs, RealFs};
use zaun_core::ids::GameId;
use zaun_core::library::steam::steam_summaries;
use zaun_core::library::{LibrarySnapshot, Overlays};
use zaun_core::model::{GameSummary, SteamCompat};
use zaun_core::paths::AppPaths;
use zaun_core::procfs::RealProc;
use zaun_core::steam::client::{SteamRequest, command, running_games};
use zaun_core::steam::{Handoff, SteamScan};
use zaun_core::storage::{ReadDb, SteamRepo};

use crate::cli::{SteamVerb, exit};
use crate::wiring;

/// Whether "Show my Steam games" is on.
pub fn enabled(paths: &AppPaths) -> bool {
    wiring::load_prefs(&RealFs::new(), paths, &SystemClock::new())
        .steam
        .show_games
}

fn scan_now(paths: &AppPaths) -> Option<SteamScan> {
    let home = paths.home.as_deref()?;
    Some(zaun_core::steam::scan(&RealFs::new(), home))
}

/// `snap` with the Steam games when the option is on (rows read with `db` when there is a
/// library; none otherwise).
pub fn with_steam(
    snap: LibrarySnapshot,
    db: Option<&mut ReadDb>,
    paths: &AppPaths,
) -> LibrarySnapshot {
    if !enabled(paths) {
        return snap;
    }
    let Some(scan) = scan_now(paths) else {
        return snap;
    };
    let rows = db.and_then(|d| d.steam_overlays().ok()).unwrap_or_default();
    let now = SystemClock::new().now();
    let summaries = steam_summaries(&scan, &rows, &Overlays::default(), now);
    snap.with_steam(summaries, now)
}

/// The Steam games alone (no Zaun library yet); empty when the option is off.
pub fn steam_only() -> Result<LibrarySnapshot, Error> {
    let env = zaun_core::paths::PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    let now = SystemClock::new().now();
    Ok(with_steam(LibrarySnapshot::empty(now), None, &paths))
}

/// The Steam app IDs running now.
pub fn running() -> BTreeMap<u32, zaun_core::procfs::ProcIdentity> {
    running_games(&RealProc::new(Arc::new(RealFs::new())))
}

/// `steam:<appid>` for a Steam game, the short ID otherwise.
pub fn short_id(g: &GameSummary) -> String {
    match g.steam_app_id.filter(|_| g.is_steam()) {
        Some(app) => format!("steam:{app}"),
        None => g.id.short(),
    }
}

/// `steam:<appid>` → the game's ID.
pub fn parse_steam_arg(arg: &str) -> Option<GameId> {
    let app: u32 = arg.trim().strip_prefix("steam:")?.parse().ok()?;
    (app > 0).then(|| GameId::steam(app))
}

fn not_for_steam(what: &str) -> Error {
    Error::new(ErrorCode::StmNotForSteam).with(
        "detail",
        format!("{what} is not possible for a game Steam runs"),
    )
}

/// Hand `request` to the Steam installation listing `game` (or the first one).
fn ask(game: Option<&GameSummary>, request: SteamRequest, paths: &AppPaths) -> Result<(), Error> {
    let scan = scan_now(paths).ok_or_else(|| Error::new(ErrorCode::StmNotFound))?;
    let (kind, root) = match game.and_then(|g| g.steam_app_id) {
        Some(app) => {
            let g = scan
                .game(app)
                .ok_or_else(|| Error::new(ErrorCode::StmGameGone).with("app_id", app))?;
            (g.facts.install, g.root.clone())
        }
        None => {
            let r = scan
                .roots
                .first()
                .ok_or_else(|| Error::new(ErrorCode::StmNotFound))?;
            (r.kind, r.path.clone())
        }
    };
    let env: BTreeMap<String, String> = std::env::vars().collect();
    let spec = command(&RealFs::new(), kind, &root, &env, request).ok_or_else(|| {
        Error::new(ErrorCode::StmClientUnavailable).with("install", kind.as_str())
    })?;
    zaun_core::session::RealHandoff.start_detached(&spec)
}

/// The library with Steam games and the game `arg` names, when it is a Steam game.
fn steam_game(arg: &str) -> Result<Option<GameSummary>, Error> {
    let snap = match crate::library_verbs::read_snapshot() {
        Ok((s, _, _)) => s,
        Err(e) if e.code == ErrorCode::LibInvalidField => steam_only()?,
        Err(e) => return Err(e),
    };
    let id = match parse_steam_arg(arg) {
        Some(id) => id,
        None => match crate::library_verbs::resolve(&snap, arg) {
            Ok(id) => id,
            Err(_) => return Ok(None),
        },
    };
    Ok(snap.get(id).filter(|g| g.is_steam()).cloned())
}

/// `launch <GAME>` for a Steam game: handed to Steam. `None` when `game` is not one.
#[allow(clippy::print_stdout)]
pub fn try_launch(
    game: &str,
    exe: Option<&str>,
    wait: bool,
    extra: &[String],
) -> Result<Option<u8>, Error> {
    let Some(g) = steam_game(game)? else {
        return Ok(None);
    };
    if exe.is_some() || wait || !extra.is_empty() {
        return Err(not_for_steam("--exe, --wait or extra arguments"));
    }
    let env = zaun_core::paths::PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    ask(
        Some(&g),
        SteamRequest::Run(g.steam_app_id.unwrap_or(0)),
        &paths,
    )?;
    println!("Asked Steam to start {}", g.name);
    Ok(Some(exit::OK))
}

/// `show <GAME>` for a Steam game. `None` when `game` is not one.
#[allow(clippy::print_stdout)]
pub fn try_show(game: &str, json: bool) -> Result<Option<u8>, Error> {
    let Some(g) = steam_game(game)? else {
        return Ok(None);
    };
    let Some(f) = g.steam() else {
        return Ok(None);
    };
    let running = running().contains_key(&f.app_id);
    let (tool, prefix) = match &f.compat {
        SteamCompat::Proton { tool, prefix, .. } => (
            Some(tool.clone().unwrap_or_else(|| "default".into())),
            prefix.clone(),
        ),
        SteamCompat::NotProton => (None, None),
    };
    if json {
        let v = serde_json::json!({
            "format": 1,
            "origin": "steam",
            "id": g.id.to_string(),
            "steam_app_id": f.app_id,
            "name": &*g.name,
            "install": f.install.as_str(),
            "install_dir": f.install_dir.display().to_string(),
            "library_folder": f.library_folder.display().to_string(),
            "size_on_disk": f.size_on_disk,
            "build_id": f.build_id,
            "last_updated_at": f.last_updated.map(|t| t.as_millis()),
            "update": f.update.as_str(),
            "download": f.download.map(|d| serde_json::json!({"done": d.done, "total": d.total})),
            "files_missing": f.files_missing,
            "proton": tool,
            "prefix": prefix.map(|p| p.display().to_string()),
            "playtime_ms": g.total_playtime_ms,
            "playtime_source": "steam",
            "last_played_at": g.last_played_at.map(|t| t.as_millis()),
            "favorite": g.favorite,
            "hidden": g.hidden,
            "running": running,
            "protection": "run_by_steam",
        });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return Ok(Some(exit::OK));
    }
    println!("{} (steam:{})", g.name, f.app_id);
    println!("  run by Steam, outside Zaun's protection");
    println!("  folder: {}", f.install_dir.display());
    println!("  update: {}", f.update.as_str().replace('_', " "));
    match tool {
        Some(t) => println!("  Proton: {t}"),
        None => println!("  Proton: not used"),
    }
    if let Some(p) = prefix {
        println!("  Windows storage: {}", p.display());
    }
    let m = g.total_playtime_ms / 60_000;
    println!("  played {}h {:02}m (from Steam)", m / 60, m % 60);
    if running {
        println!("  running now");
    }
    Ok(Some(exit::OK))
}

pub(crate) fn gui_running(paths: &AppPaths) -> bool {
    zaun_core::desktop::instance::SocketTransport::new(paths.instance_socket.clone()).ping()
}

/// `steam scan|on|off|update|open`.
#[allow(clippy::print_stdout)]
pub fn run(verb: &SteamVerb) -> Result<u8, Error> {
    let env = zaun_core::paths::PathEnv::from_process();
    let paths = AppPaths::resolve(&env)
        .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("paths", e))?;
    match verb {
        SteamVerb::Scan { json } => {
            // Nothing of Steam's is read while the option is off (ADR-0031 §1).
            if !enabled(&paths) {
                if *json {
                    println!(
                        "{}",
                        serde_json::json!({"format": 1, "enabled": false, "roots": [], "games": [], "skipped": []})
                    );
                } else {
                    println!(
                        "Steam games are not shown, so Steam's files are not read. Turn it on with `zaun steam on`."
                    );
                }
                return Ok(exit::OK);
            }
            let scan = scan_now(&paths).unwrap_or_default();
            if *json {
                let v = serde_json::json!({
                    "format": 1,
                    "enabled": enabled(&paths),
                    "roots": scan.roots.iter().map(|r| serde_json::json!({
                        "path": r.path.display().to_string(),
                        "install": r.kind.as_str(),
                    })).collect::<Vec<_>>(),
                    "games": scan.games.iter().map(|g| serde_json::json!({
                        "steam_app_id": g.facts.app_id,
                        "name": &g.name,
                        "update": g.facts.update.as_str(),
                        "playtime_minutes": g.facts.playtime_minutes,
                        "proton": matches!(g.facts.compat, SteamCompat::Proton { .. }),
                    })).collect::<Vec<_>>(),
                    "skipped": scan.skipped.iter().map(|s| serde_json::json!({
                        "steam_app_id": s.app_id,
                        "name": &s.name,
                        "reason": s.reason.as_str(),
                    })).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
                return Ok(exit::OK);
            }
            if scan.roots.is_empty() {
                println!("Steam was not found.");
                return Ok(exit::OK);
            }
            for r in &scan.roots {
                println!("Steam ({}) at {}", r.kind.as_str(), r.path.display());
            }
            println!(
                "{} games, {} other apps left out",
                scan.games.len(),
                scan.skipped.len()
            );
            for g in &scan.games {
                println!("  steam:{:<10} {}", g.facts.app_id, g.name);
            }
            Ok(exit::OK)
        }
        SteamVerb::On | SteamVerb::Off => {
            let on = matches!(verb, SteamVerb::On);
            if gui_running(&paths) {
                return Err(Error::new(ErrorCode::LibBusy).with(
                    "detail",
                    "Zaun is open; change \"Show my Steam games\" in its settings",
                ));
            }
            let fs = RealFs::new();
            let prefs = wiring::load_prefs(&fs, &paths, &SystemClock::new());
            let next = prefs
                .apply(
                    &PreferencePatch::new().set("steam.show_games", on),
                    &fs,
                    &zaun_core::config::LoadOptions { env_rule: None },
                )
                .map_err(Error::from)?;
            fs.create_dir_all(&paths.config_dir, 0o700)?;
            zaun_core::config::save(&fs, &paths.config_file, &next).map_err(Error::from)?;
            println!(
                "{}",
                if on {
                    "Steam games are shown."
                } else {
                    "Steam games are not shown."
                }
            );
            Ok(exit::OK)
        }
        SteamVerb::Update { game } | SteamVerb::Open { game } => {
            let g = steam_game(game)?.ok_or_else(|| {
                Error::new(ErrorCode::StmGameGone).with("detail", format!("no Steam game {game:?}"))
            })?;
            let app = g.steam_app_id.unwrap_or(0);
            let (request, said) = match verb {
                SteamVerb::Update { .. } => (
                    SteamRequest::Downloads,
                    format!(
                        "Opened Steam's downloads, where the update of {} can be started",
                        g.name
                    ),
                ),
                _ => (
                    SteamRequest::Details(app),
                    format!("Opened {} in Steam", g.name),
                ),
            };
            ask(Some(&g), request, &paths)?;
            println!("{said}");
            Ok(exit::OK)
        }
        SteamVerb::LaunchOptions {
            game,
            set,
            mangohud,
        } => launch_options(&paths, game, set.as_deref(), *mangohud),
    }
}

/// `steam launch-options <GAME> [--set TEXT] [--mangohud on|off]` (ADR-0031 §6a): prints
/// the launch options as Steam has them, or changes them while Steam is closed.
#[allow(clippy::print_stdout)]
fn launch_options(
    paths: &AppPaths,
    game: &str,
    set: Option<&str>,
    mangohud: Option<crate::cli::SwitchArg>,
) -> Result<u8, Error> {
    use zaun_core::steam::launch_options as lo;
    if !enabled(paths) {
        println!(
            "Steam games are not shown, so Steam's files are not read. Turn it on with `zaun steam on`."
        );
        return Ok(exit::USAGE);
    }
    let g = steam_game(game)?.ok_or_else(|| {
        Error::new(ErrorCode::StmGameGone).with("detail", format!("no Steam game {game:?}"))
    })?;
    let app = g.steam_app_id.unwrap_or(0);
    let scan = scan_now(paths).ok_or_else(|| Error::new(ErrorCode::StmNotFound))?;
    let root = scan
        .game(app)
        .map(|s| s.root.clone())
        .ok_or_else(|| Error::new(ErrorCode::StmGameGone).with("app_id", app))?;
    let fs = RealFs::new();
    let current = lo::read(&fs, &root, app);
    if set.is_none() && mangohud.is_none() {
        let v = current.ok_or_else(|| {
            Error::new(ErrorCode::StmSettingsUnwritable).with("reason", "no account settings")
        })?;
        println!("{v}");
        return Ok(exit::OK);
    }
    let mut next = set.map_or_else(|| current.unwrap_or_default(), str::to_owned);
    if let Some(m) = mangohud {
        next = lo::with_mangohud(&next, m == crate::cli::SwitchArg::On);
    }
    let proc = RealProc::new(Arc::new(RealFs::new()));
    match lo::write(&fs, &proc, &root, app, &next)? {
        lo::Written::Unchanged => println!("The launch options of {} are already so.", g.name),
        lo::Written::Changed { .. } => println!(
            "Launch options of {} saved. Steam uses them the next time it starts the game.",
            g.name
        ),
    }
    Ok(exit::OK)
}

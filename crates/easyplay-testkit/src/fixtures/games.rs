//! Game, session and library generators, and a semantic dump of a library database
//! (TESTING_STRATEGY §5, §12).
//!
//! Everything is deterministic: identifiers are derived from small integers and the
//! "random" choices come from a fixed linear congruential generator, so generated libraries
//! and their dumps are identical on every run.

use std::fmt::Write as _;
use std::path::PathBuf;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::{ExecutableId, GameId, SessionId, TagName};
use easyplay_core::model::{
    AudioAccess, ControllerAccess, DeviceAccess, DisplayAccess, DisplayIsolation, EndReason,
    ExeRole, Executable, Fingerprint, Game, GameDirAccess, GameFlags, Hardening, IpcAccess,
    LaunchConfig, NetworkAccess, PeInfo, PeMachine, PeSubsystem, Preset, RemovableAccess,
    RuntimeIsolation, RuntimeRef, SandboxPolicy, SaveDataLocation, SeccompMode, Session,
    SessionBusAccess, SessionContext, SessionKind, SessionState, SocketAccess, TagColor,
};
use easyplay_core::storage::{
    ExecutableRepo, GameRepo, KvRepo, PlaytimePiece, RuntimeRepo, SessionClose, SessionRepo,
    StatsRepo, Store, TagRepo,
};

/// 2026-11-12T10:40:00Z, the baseline used by the fixtures.
pub const BASE_MS: i64 = 1_794_480_000_000;
const DAY_MS: i64 = 86_400_000;

/// UUIDv7-shaped constant text: version nibble 7, variant 10.
fn uuid_text(tag: u32, n: u64) -> String {
    format!("0192f0c2-0000-7000-8{tag:03x}-{n:012x}")
}

/// The `n`-th game ID.
pub fn game_id(n: u32) -> GameId {
    uuid_text(1, u64::from(n)).parse().expect("uuid")
}

/// The `k`-th executable ID of game `n`.
pub fn exe_id(n: u32, k: u32) -> ExecutableId {
    uuid_text(2, (u64::from(n) << 8) | u64::from(k))
        .parse()
        .expect("uuid")
}

/// The `k`-th session ID of game `n`.
pub fn session_id(n: u32, k: u32) -> SessionId {
    uuid_text(3, (u64::from(n) << 16) | u64::from(k))
        .parse()
        .expect("uuid")
}

/// The *Offline* preset policy (SECURITY_MODEL §5).
pub fn offline_policy() -> SandboxPolicy {
    SandboxPolicy {
        schema_version: 1,
        preset: Preset::Offline,
        network: NetworkAccess::None,
        game_dir: GameDirAccess::ReadWrite,
        folders: vec![],
        save_data: SaveDataLocation::Internal,
        removable: RemovableAccess::None,
        devices: DeviceAccess {
            audio: AudioAccess::Full,
            alsa_devices: false,
            controllers: ControllerAccess::Detected,
            camera: false,
        },
        display: DisplayAccess {
            wayland: SocketAccess::Auto,
            x11: SocketAccess::Auto,
            isolation: DisplayIsolation::None,
        },
        ipc: IpcAccess {
            session_bus: SessionBusAccess::None,
        },
        hardening: Hardening {
            seccomp: SeccompMode::Default,
            mask_hostname: true,
            mask_machine_id: true,
            mask_hardware_ids: true,
            runtime_isolation: RuntimeIsolation::Auto,
        },
        exceptions: vec![],
    }
}

/// Executable `k` of game `n` (`k = 0` is the primary).
pub fn executable(n: u32, k: u32, root: &str) -> Executable {
    let file = if k == 0 {
        "game.exe".to_owned()
    } else {
        format!("tool{k}.exe")
    };
    let byte = u8::try_from((n * 7 + k) % 251).unwrap_or(0);
    Executable {
        id: exe_id(n, k),
        role: if k == 0 {
            ExeRole::Primary
        } else {
            ExeRole::Tool
        },
        path: PathBuf::from(format!("{root}/{file}")),
        canonical_path: PathBuf::from(format!("{root}/{file}")),
        rel_path: Some(PathBuf::from(&file)),
        mount_point: Some(PathBuf::from("/")),
        fingerprint: Fingerprint {
            size: 1_048_576 + u64::from(n),
            mtime_ns: 1_700_000_000_000_000_000 + i64::from(n),
            quick_hash: [byte; 32],
            full_hash: (k == 0).then_some([byte.wrapping_add(1); 32]),
        },
        pe: PeInfo {
            product_name: Some(format!("Product {n}")),
            machine: PeMachine::X86_64,
            subsystem: PeSubsystem::Gui,
            has_icon: true,
            ..PeInfo::default()
        },
        args_override: (k == 1).then(|| vec!["-config".to_owned()]),
        label: (k > 0).then(|| format!("Tool {k}")),
        added_at: Timestamp::from_millis(BASE_MS),
    }
}

const WORDS: &[&str] = &[
    "Hollow", "Crimson", "Silent", "Iron", "Last", "Star", "Deep", "Lost", "Frozen", "Golden",
];
const NOUNS: &[&str] = &[
    "Knight", "Harbor", "Engine", "Forest", "Signal", "Kingdom", "Voyage", "Archive", "Tower",
];

/// A realistic, unique game name for index `n`.
pub fn game_name(n: u32) -> String {
    let w = WORDS[n as usize % WORDS.len()];
    let o = NOUNS[(n as usize / WORDS.len()) % NOUNS.len()];
    let cycle = n as usize / (WORDS.len() * NOUNS.len());
    if cycle == 0 {
        format!("{w} {o}")
    } else {
        format!("{w} {o} {}", cycle + 1)
    }
}

/// Game `n` with `exes` executables (at least the primary).
pub fn game(n: u32, exes: u32) -> Game {
    let name = game_name(n);
    let root = format!("/games/{n:04}");
    Game {
        id: game_id(n),
        sort_name: easyplay_core::model::sort_name_of(&name),
        name,
        name_is_user_set: n.is_multiple_of(3),
        root: Some(PathBuf::from(&root)),
        added_at: Timestamp::from_millis(BASE_MS + i64::from(n)),
        updated_at: Timestamp::from_millis(BASE_MS + i64::from(n)),
        favorite: n.is_multiple_of(5),
        hidden: n.is_multiple_of(11),
        notes: if n.is_multiple_of(4) {
            format!("Notes for game {n}")
        } else {
            String::new()
        },
        policy: offline_policy(),
        policy_updated_at: Timestamp::from_millis(BASE_MS),
        launch: LaunchConfig {
            args: if n.is_multiple_of(2) {
                vec!["-windowed".into()]
            } else {
                vec![]
            },
            ..LaunchConfig::default()
        },
        umu_id: (n % 2 == 1).then(|| format!("umu-{}", 100_000 + n)),
        store: None,
        runtime: RuntimeRef::Default,
        runtime_follow_updates: true,
        runtime_last_resolved: None,
        prefix_tainted_at: None,
        prefix_prepared_with: None,
        flags: GameFlags::empty(),
        last_launched_at: None,
        executables: (0..exes.max(1)).map(|k| executable(n, k, &root)).collect(),
    }
}

/// A started session of `game` (state `starting`).
pub fn session(game: &Game, n: u32, k: u32, started_at: Timestamp) -> Session {
    Session {
        id: session_id(n, k),
        game_id: Some(game.id),
        game_name: game.name.clone(),
        executable_id: game.executables.first().map(|e| e.id),
        kind: SessionKind::Game,
        state: SessionState::Starting,
        started_at,
        ended_at: None,
        duration_ms: None,
        active_ms: 0,
        last_heartbeat_at: None,
        end_reason: None,
        exit_status: None,
        counted: false,
        supervisor: None,
        sandbox_pid: None,
        pid_ns: None,
        context: SessionContext {
            preset: Some(Preset::Offline),
            network: Some(NetworkAccess::None),
            ..SessionContext::default()
        },
        tz_offset_min: 0,
    }
}

/// `YYYYMMDD` of a UTC timestamp (the fixtures use offset 0).
pub fn utc_day(t: Timestamp) -> u32 {
    // Days-to-civil (H. Hinnant).
    let z = t.as_millis().div_euclid(DAY_MS) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    u32::try_from(y * 10_000 + m * 100 + d).expect("date")
}

/// Split a session at UTC midnights (a minimal stand-in for `stats::rollup::split`).
pub fn pieces(started_at: Timestamp, duration_ms: u64) -> Vec<PlaytimePiece> {
    let mut out = Vec::new();
    let mut t = started_at.as_millis();
    let mut left = i64::try_from(duration_ms).expect("duration");
    let mut first = true;
    while left > 0 {
        let next_midnight = (t.div_euclid(DAY_MS) + 1) * DAY_MS;
        let chunk = left.min(next_midnight - t);
        out.push(PlaytimePiece {
            day: utc_day(Timestamp::from_millis(t)),
            ms: u64::try_from(chunk).expect("chunk"),
            starts_here: first,
        });
        first = false;
        left -= chunk;
        t += chunk;
    }
    out
}

/// A fixed linear congruential generator.
#[derive(Clone, Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// Seeded generator.
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9E37_79B9_7F4A_7C15)
    }
    /// Next value in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n.max(1)
    }
}

/// What [`populate`] created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Populated {
    /// Games inserted.
    pub games: Vec<GameId>,
    /// Sessions closed (counted or not).
    pub sessions: usize,
    /// Total counted playtime.
    pub counted_ms: u64,
}

/// Fill `db` with `games` games (1–3 executables each), five tags, artwork-free, and up to
/// `sessions_per_game` closed sessions per game spread over the preceding days, entirely
/// through the repositories.
pub fn populate(
    db: &mut (impl Store + ?Sized),
    games: u32,
    sessions_per_game: u32,
    seed: u64,
) -> Populated {
    let mut rng = Lcg::new(seed);
    let base = Timestamp::from_millis(BASE_MS);
    let tags: Vec<_> = ["Finished", "Co-op", "Metroidvania", "Backlog", "Favourites"]
        .iter()
        .enumerate()
        .map(|(i, t)| {
            db.create_tag(
                &TagName::new(t).expect("tag"),
                TagColor::new(u8::try_from(i).expect("color")).expect("color"),
                base,
            )
            .expect("create tag")
        })
        .collect();
    let mut ids = Vec::new();
    let mut sessions = 0;
    let mut counted_ms = 0;
    for n in 0..games {
        let exes = 1 + u32::try_from(rng.below(3)).expect("small");
        let g = game(n, exes);
        db.insert_game(&g).expect("insert game");
        let picked: Vec<_> = tags
            .iter()
            .filter(|_| rng.below(3) == 0)
            .map(|t| t.id)
            .collect();
        db.set_game_tags(g.id, &picked).expect("tags");
        let count = u32::try_from(rng.below(u64::from(sessions_per_game) + 1)).expect("small");
        for k in 0..count {
            let days_ago = i64::try_from(rng.below(900)).expect("small") + 1;
            let start = Timestamp::from_millis(
                BASE_MS - days_ago * DAY_MS + i64::try_from(rng.below(86_000_000)).expect("small"),
            );
            let mut s = session(&g, n, k, start);
            s.executable_id = Some(g.executables[0].id);
            db.insert_session(&s).expect("insert session");
            let failed = rng.below(10) == 0;
            let duration = if failed {
                1_500
            } else {
                60_000 + rng.below(7_200_000)
            };
            let close = SessionClose {
                id: s.id,
                ended_at: start.saturating_add(std::time::Duration::from_millis(duration)),
                duration_ms: duration,
                end_reason: if failed {
                    EndReason::Crashed { signal: 11 }
                } else {
                    EndReason::Exited { code: 0 }
                },
                exit_status: Some(if failed { 139 } else { 0 }),
                counted: !failed,
                pieces: pieces(start, duration),
            };
            db.close_session(&close).expect("close session");
            sessions += 1;
            if !failed {
                counted_ms += duration;
            }
        }
        ids.push(g.id);
    }
    db.kv_set("library.view", "grid", base).expect("kv");
    db.kv_set("sidebar.tag_order", "[1,2,3,4,5]", base)
        .expect("kv");
    Populated {
        games: ids,
        sessions,
        counted_ms,
    }
}

/// A human-readable, order-stable dump of everything the repositories return: used to
/// compare fixture databases across migrations without depending on physical layout.
pub fn semantic_dump(db: &mut (impl Store + ?Sized)) -> String {
    let mut out = String::new();
    let lib = db.load_library().expect("load library");
    let _ = writeln!(
        out,
        "schema_version {}",
        db.schema_version().expect("version")
    );
    for g in &lib.games {
        let _ = writeln!(out, "game {g:?}");
        for t in db.tags_of(g.id).expect("tags of") {
            let _ = writeln!(out, "  tag {t}");
        }
        for e in db.executables_of(g.id).expect("executables") {
            let _ = writeln!(out, "  exe {}", e.id);
        }
        for s in db
            .session_history(Some(g.id), &[], 0, 10_000)
            .expect("history")
        {
            let _ = writeln!(out, "  session {s:?}");
        }
    }
    for t in db.tags_with_counts().expect("tags") {
        let _ = writeln!(out, "tag {t:?}");
    }
    for a in &lib.artwork {
        let _ = writeln!(out, "artwork {a:?}");
    }
    for s in db.all_game_stats().expect("stats") {
        let _ = writeln!(out, "stats {s:?}");
    }
    for d in db.playtime_range(None, 0, 99_999_999).expect("playtime") {
        let _ = writeln!(out, "day {d:?}");
    }
    for r in db.runtimes().expect("runtimes") {
        let _ = writeln!(out, "runtime {r:?}");
    }
    for s in db.slr_installs().expect("slr") {
        let _ = writeln!(out, "slr {s:?}");
    }
    for (k, v) in db.kv_with_prefix("").expect("kv") {
        let _ = writeln!(out, "kv {k} = {v}");
    }
    out
}

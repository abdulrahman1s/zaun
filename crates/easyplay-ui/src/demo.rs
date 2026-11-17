//! Sample data and a preview [`Backend`] used when no application service is connected
//! (`easyplay` before the service lands) and by snapshot tests. Titles are invented. The
//! preview simulates launches (phases, a running session, one failure), keeps edits in memory
//! and answers every query, so the whole UI is clickable.
//!
//! The sentences produced here stand in for data the service supplies (`sandbox::explain`,
//! the error catalogue, the doctor), so they live with the data rather than in `strings`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use easyplay_core::artwork::SearchCandidate;
use easyplay_core::clock::Timestamp;
use easyplay_core::config::Preferences;
use easyplay_core::error::{ErrorCode, RecoveryAction, Severity, UserError};
use easyplay_core::host::facts::HostFacts;
use easyplay_core::identity::{GameRootProposal, RootKind, RootReason, SetupHints};
use easyplay_core::ids::{ExecutableId, GameId, PromptId, RuntimeName, SessionId, TagId, TagName};
use easyplay_core::launch::LaunchState;
use easyplay_core::library::{GameDraft, LibrarySnapshot};
use easyplay_core::model::{
    Activity, ArtKey, Availability, BasePreset, ControllerAccess, EndReason, EntryKind, ExeRole,
    Executable, Fingerprint, FolderAccess, Game, GameDirAccess, GameFlags, GameKey, GameStats,
    GameSummary, GrantOrigin, LaunchConfig, LaunchStateTag, NetworkAccess, PeInfo, Preset,
    PromptKind, PromptRequest, RemovableAccess, RuntimeBuild, RuntimeFamily, RuntimeOrigin,
    RuntimeRef, SandboxPolicy, SaveDataLocation, SaveLocationKind, SeccompMode, SessionKind,
    SlrVariant, SummaryFlags, TagColor, UserDir, sort_name_of,
};
use easyplay_core::sandbox::{
    AccessCategory, AccessStatement, IssueCode, PolicyIssue, StatementSeverity, apply_preset,
    classify,
};
use easyplay_core::stats::rollup::{day_key, key_days, local_day};
use easyplay_core::stats::{
    Bucket, DateRange, GameOverview, HistoryPage, HistoryRow, LibraryOverview, Ranking, Series,
    StatsRequest, StatsResult, Totals,
};
use easyplay_core::umu::UmuPhase;

use crate::bridge::{
    AfterCreate, Backend, CheckState, DownloadInfo, FixEntry, FolderScan, GameDetail, GameEdit,
    HandlerState, HostCheck, HostStatus, HostView, LayerInfo, PolicyAssessment, ProgramFound,
    QueryPayload, RuntimesView, SetupContext, SetupOrigin, ShortcutInfo, ToastLevel, ToastMsg,
    UiCommand, UiEvent, library_from,
};
use crate::strings;

/// 2026-11-12T19:30:00Z: the specification baseline, evening.
pub fn baseline_now() -> Timestamp {
    Timestamp::from_millis(1_794_441_600_000 + 19 * 3_600_000 + 30 * 60_000)
}

const H: u64 = 3_600_000;
const M: u64 = 60_000;
const DAY: u64 = 24 * H;

struct Spec {
    name: &'static str,
    preset: Preset,
    played_ms: u64,
    last_days: Option<u64>,
    favorite: bool,
    art: bool,
    tags: &'static [i64],
    activity: Activity,
    availability: Availability,
    kind: EntryKind,
    attention: bool,
}

const fn spec(name: &'static str, preset: Preset, played_ms: u64, last_days: Option<u64>) -> Spec {
    Spec {
        name,
        preset,
        played_ms,
        last_days,
        favorite: false,
        art: true,
        tags: &[],
        activity: Activity::Idle,
        availability: Availability::Present,
        kind: EntryKind::Game,
        attention: false,
    }
}

fn specs(now: Timestamp) -> Vec<Spec> {
    let running_since = now.saturating_sub(Duration::from_millis(42 * M + 10_000));
    vec![
        Spec {
            favorite: true,
            tags: &[1],
            ..spec("Lantern Keep", Preset::Offline, 12 * H + 5 * M, Some(1))
        },
        Spec {
            activity: Activity::Running {
                since: running_since,
            },
            tags: &[2],
            ..spec("Starfall Tactics", Preset::Online, 31 * H + 40 * M, Some(0))
        },
        Spec {
            favorite: true,
            ..spec("Moss & Iron", Preset::Offline, 4 * H + 12 * M, Some(3))
        },
        Spec {
            activity: Activity::Launching(LaunchStateTag::Preparing),
            ..spec("Harbor Lights", Preset::Standard, 55 * M, Some(9))
        },
        spec(
            "Quiet Orbit",
            Preset::Custom {
                base: BasePreset::Offline,
            },
            128 * H,
            Some(16),
        ),
        Spec {
            art: false,
            ..spec("Vale of Cinders", Preset::Offline, 0, None)
        },
        Spec {
            availability: Availability::Missing,
            ..spec("Paper Kingdoms", Preset::Online, 7 * H, Some(40))
        },
        Spec {
            tags: &[1],
            ..spec(
                "The Long Winter Road: Director's Edition",
                Preset::Offline,
                22 * H + 30 * M,
                Some(5),
            )
        },
        Spec {
            availability: Availability::DriveDisconnected,
            ..spec("Ember Run", Preset::Standard, 3 * H, Some(70))
        },
        Spec {
            kind: EntryKind::Installer,
            art: false,
            ..spec("Setup Glassworks", Preset::Offline, 0, None)
        },
        Spec {
            attention: true,
            ..spec("Tidewater", Preset::Online, 18 * M, Some(2))
        },
        Spec {
            favorite: true,
            tags: &[2, 3],
            ..spec("Ninefold Arena", Preset::Online, 410 * H, Some(4))
        },
        spec("Clockwork Garden", Preset::Offline, 2 * H + 2 * M, Some(12)),
        Spec {
            art: false,
            ..spec("Driftwood", Preset::Offline, 40 * M, Some(20))
        },
        Spec {
            tags: &[3],
            ..spec("Signal Lost", Preset::Online, 9 * H, Some(28))
        },
        spec("Amber Sky", Preset::Standard, 1240 * H, Some(1)),
        spec("Northbound", Preset::Offline, 6 * H, Some(100)),
        Spec {
            tags: &[1],
            ..spec("Rune & Rail", Preset::Offline, 0, None)
        },
        spec(
            "Hollow Bell",
            Preset::Custom {
                base: BasePreset::Online,
            },
            15 * H,
            Some(6),
        ),
        spec("Kestrel", Preset::Offline, H, Some(33)),
        spec("Low Tide Motel", Preset::Offline, 3 * H + 30 * M, Some(2)),
        spec("Graywater", Preset::Online, 0, None),
        spec("Saltmarsh", Preset::Offline, 26 * H, Some(11)),
        spec("Wildflower Pass", Preset::Offline, 8 * H, Some(18)),
    ]
}

fn art_key(seed: u64) -> ArtKey {
    let mut h = [0u8; 32];
    for (i, b) in h.iter_mut().enumerate() {
        *b = (seed.rotate_left(i as u32 * 7) ^ (i as u64 * 0x9E37)) as u8;
    }
    h[0] = seed as u8;
    h[1] = (seed >> 8) as u8;
    ArtKey(h)
}

fn folder_of(name: &str) -> String {
    name.split(':')
        .next()
        .unwrap_or(name)
        .replace(['&', '\''], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn exe_of(name: &str) -> String {
    format!(
        "{}.exe",
        folder_of(name)
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("")
    )
}

/// Where a sample game lives.
pub fn root_of(name: &str) -> PathBuf {
    PathBuf::from(format!("/home/sam/Games/{}", folder_of(name)))
}

fn summary(i: usize, s: Spec, now: Timestamp) -> GameSummary {
    let last = s
        .last_days
        .map(|d| now.saturating_sub(Duration::from_millis(d * DAY + (i as u64 % 5) * H)));
    let sessions = if s.played_ms > 0 {
        (s.played_ms / H) as u32 + 1
    } else {
        0
    };
    let root = root_of(s.name);
    GameSummary {
        id: GameId::from_uuid(uuid::Uuid::from_u128(0x1000 + i as u128)),
        name: Arc::from(s.name),
        sort_name: Arc::from(sort_name_of(s.name)),
        cover: s.art.then(|| art_key(i as u64 + 1)),
        icon: None,
        hero: None,
        steam_app_id: None,
        accent: None,
        favorite: s.favorite,
        hidden: false,
        tags: s.tags.iter().map(|t| TagId::new(*t)).collect(),
        added_at: now.saturating_sub(Duration::from_millis((i as u64 + 3) * 9 * DAY)),
        last_played_at: last,
        first_played_at: last.map(|l| l.saturating_sub(Duration::from_millis(30 * DAY))),
        total_playtime_ms: s.played_ms,
        session_count: sessions,
        launch_count: sessions,
        preset: s.preset,
        network: if s.preset.base() == BasePreset::Offline {
            NetworkAccess::None
        } else {
            NetworkAccess::Full
        },
        runtime_label: Arc::from("UMU-Proton 10.0-4"),
        primary_exe_path: Arc::from(root.join(exe_of(s.name)).as_path()),
        root: Some(Arc::from(root.as_path())),
        save_location: SaveLocationKind::Internal,
        kind: s.kind,
        availability: s.availability,
        activity: s.activity,
        flags: SummaryFlags {
            needs_attention: s.attention,
            ..SummaryFlags::default()
        },
        origin: easyplay_core::model::GameOrigin::EasyPlay,
    }
}

/// One sample Steam game: its app ID, title, play time and last play (days ago), how it
/// runs, its update and whether Steam runs it now.
struct SteamSpec {
    app_id: u32,
    name: &'static str,
    played_min: u64,
    last_days: Option<u64>,
    compat: SteamKind,
    update: easyplay_core::model::SteamUpdate,
    download: Option<(u64, u64)>,
    running_min: Option<u64>,
    tags: &'static [i64],
    favorite: bool,
}

/// How a sample Steam game runs.
#[derive(Clone, Copy)]
enum SteamKind {
    /// Through Proton, the one named (`None`: Steam's default), last run with a version.
    Proton(Option<&'static str>, &'static str),
    /// Natively on Linux.
    Native,
}

fn steam_specs() -> [SteamSpec; 3] {
    use easyplay_core::model::SteamUpdate;
    [
        SteamSpec {
            app_id: 2_000_001,
            name: "Ridgeline Rally",
            played_min: 74 * 60 + 10,
            last_days: Some(3),
            compat: SteamKind::Proton(Some("proton_experimental"), "10.0-200"),
            update: SteamUpdate::Waiting,
            download: Some((0, 1_400_000_000)),
            running_min: None,
            tags: &[2],
            favorite: false,
        },
        SteamSpec {
            app_id: 2_000_002,
            name: "Brass Lighthouse",
            played_min: 9 * 60 + 25,
            last_days: Some(6),
            compat: SteamKind::Native,
            update: SteamUpdate::UpToDate,
            download: None,
            running_min: None,
            tags: &[],
            favorite: true,
        },
        SteamSpec {
            app_id: 2_000_003,
            name: "Glass Garden",
            played_min: 31 * 60 + 5,
            last_days: Some(0),
            compat: SteamKind::Proton(None, "9.0-4"),
            update: SteamUpdate::UpToDate,
            download: None,
            running_min: Some(18),
            tags: &[1],
            favorite: false,
        },
    ]
}

fn steam_summary(s: &SteamSpec, now: Timestamp) -> GameSummary {
    use easyplay_core::model::{
        GameOrigin, SteamCompat, SteamDownload, SteamFacts, SteamInstallKind,
    };
    let lib = PathBuf::from("/home/sam/.local/share/Steam");
    let install = lib.join("steamapps/common").join(s.name);
    let compat_data = lib.join(format!("steamapps/compatdata/{}", s.app_id));
    let compat = match s.compat {
        SteamKind::Proton(tool, version) => SteamCompat::Proton {
            tool: tool.map(str::to_owned),
            version: Some(version.to_owned()),
            prefix: Some(compat_data.join("pfx")),
            compat_data,
            proton_dir: Some(lib.join("steamapps/common/Proton - Experimental")),
        },
        SteamKind::Native => SteamCompat::NotProton,
    };
    let facts = SteamFacts {
        app_id: s.app_id,
        install: SteamInstallKind::Native,
        library_folder: lib,
        install_dir: install.clone(),
        size_on_disk: 23_500_000_000 / u64::from(s.app_id % 7 + 1),
        build_id: Some(18_000_000 + u64::from(s.app_id % 1000)),
        last_updated: Some(now.saturating_sub(Duration::from_millis(12 * DAY))),
        update: s.update,
        download: s
            .download
            .map(|(done, total)| SteamDownload { done, total }),
        files_missing: false,
        compat,
        playtime_minutes: Some(s.played_min),
    };
    let install: Arc<Path> = Arc::from(install.as_path());
    let activity = s.running_min.map_or(Activity::Idle, |m| Activity::Running {
        since: now.saturating_sub(Duration::from_millis(m * M)),
    });
    GameSummary {
        id: GameId::steam(s.app_id),
        name: Arc::from(s.name),
        sort_name: Arc::from(sort_name_of(s.name)),
        cover: Some(art_key(9000 + u64::from(s.app_id % 1000))),
        icon: None,
        hero: None,
        steam_app_id: Some(s.app_id),
        accent: None,
        favorite: s.favorite,
        hidden: false,
        tags: s.tags.iter().map(|t| TagId::new(*t)).collect(),
        added_at: now.saturating_sub(Duration::from_millis(40 * DAY)),
        last_played_at: s
            .last_days
            .map(|d| now.saturating_sub(Duration::from_millis(d * DAY + 2 * H))),
        first_played_at: None,
        total_playtime_ms: s.played_min * M,
        session_count: 0,
        launch_count: 0,
        preset: Preset::Standard,
        network: NetworkAccess::Full,
        runtime_label: Arc::from(easyplay_core::library::steam::layer_label(&facts)),
        primary_exe_path: install.clone(),
        root: Some(install),
        save_location: SaveLocationKind::Internal,
        kind: EntryKind::Game,
        availability: Availability::Present,
        activity,
        flags: SummaryFlags::default(),
        origin: GameOrigin::Steam(Arc::new(facts)),
    }
}

/// The sample Steam games (ADR-0031), as the preview shows them with "Show my Steam games"
/// on: "Ridgeline Rally" (Proton, an update waiting), "Brass Lighthouse" (runs natively on
/// Linux) and "Glass Garden" (Proton, Steam runs it now). Not part of [`library`], whose
/// content the existing snapshot tests fix; [`preview_library`] has them.
pub fn steam_samples(now: Timestamp) -> Vec<GameSummary> {
    steam_specs()
        .iter()
        .map(|s| steam_summary(s, now))
        .collect()
}

/// What the page of the Steam game `g` asks for (`SteamAction::Details`), as the preview
/// answers it: Steam is installed; the Windows-storage tools are possible for a Proton game
/// with storage that is not running.
pub fn steam_detail(g: &GameSummary) -> Option<crate::bridge::SteamDetail> {
    let facts = match &g.origin {
        easyplay_core::model::GameOrigin::Steam(f) => f.clone(),
        easyplay_core::model::GameOrigin::EasyPlay => return None,
    };
    let running = !matches!(g.activity, Activity::Idle | Activity::Launching(_));
    let proton = matches!(
        &facts.compat,
        easyplay_core::model::SteamCompat::Proton {
            proton_dir: Some(_),
            ..
        }
    );
    Some(crate::bridge::SteamDetail {
        game: g.id,
        prefix_tools: proton && facts.prefix().is_some() && !running,
        facts,
        notes: String::new(),
        running,
        client_found: true,
        // The first sample has options Steam was given, the others none.
        launch_options: Some(if g.name.starts_with("Ridgeline") {
            "PROTON_LOG=1 %command%".to_owned()
        } else {
            String::new()
        }),
        steam_open: false,
    })
}

/// The preview's library: the sample library and the sample Steam games.
pub fn preview_library(now: Timestamp) -> LibrarySnapshot {
    let mut games: Vec<GameSummary> = specs(now)
        .into_iter()
        .enumerate()
        .map(|(i, s)| summary(i, s, now))
        .collect();
    games.extend(steam_samples(now));
    library_from(games, sample_tags(), 1, now)
}

/// The sample tags.
pub fn sample_tags() -> Vec<(TagId, &'static str, TagColor)> {
    let c = |n: u8| TagColor::new(n).unwrap_or_default();
    vec![
        (TagId::new(2), "co-op", c(0)),
        (TagId::new(3), "competitive", c(6)),
        (TagId::new(1), "rpg", c(1)),
    ]
}

/// The sample library at `now`.
pub fn library(now: Timestamp) -> LibrarySnapshot {
    let games = specs(now)
        .into_iter()
        .enumerate()
        .map(|(i, s)| summary(i, s, now))
        .collect();
    library_from(games, sample_tags(), 1, now)
}

/// Preferences of the sample setup (defaults, onboarding done).
pub fn preferences() -> Preferences {
    let mut p = Preferences::default();
    p.onboarding.completed = true;
    p
}

/// Host facts where every check passes.
pub fn host_view() -> HostView {
    let check = |id: &str, name: &str, result: &str| HostCheck {
        id: id.into(),
        state: CheckState::Pass,
        name: name.into(),
        result: result.into(),
        detail: None,
        fix_command: None,
    };
    HostView {
        status: HostStatus::Ready,
        checks: vec![
            check("D-01", "Sandbox tool", "bubblewrap 0.11"),
            check("D-04", "Private namespaces", "Available"),
            check("D-06", "Compatibility launcher", "umu 1.4.4"),
            check("D-09", "Graphics", "Vulkan, 32-bit drivers found"),
        ],
        handler: HandlerState::Other("Wine Windows Program Loader".into()),
        distro: "Fedora Linux 43".into(),
        jobs: Vec::new(),
        shortcuts: vec![ShortcutInfo {
            game: GameId::from_uuid(uuid::Uuid::from_u128(0x1000)),
            name: "Lantern Keep".into(),
        }],
        reflinks: true,
        input_readable: false,
        mangohud: false,
        x11: false,
        sound_includes_microphone: false,
        version: "0.1.0".into(),
        home: PathBuf::from(HOME),
        user_dirs: [
            UserDir::Desktop,
            UserDir::Documents,
            UserDir::Downloads,
            UserDir::Music,
            UserDir::Pictures,
            UserDir::Videos,
        ]
        .into_iter()
        .map(|d| (d, PathBuf::from(HOME).join(user_dir_name(d))))
        .collect(),
    }
}

/// Host facts with private namespaces turned off (onboarding failure path).
pub fn host_view_without_namespaces() -> HostView {
    let mut h = host_view();
    h.status = HostStatus::NeedsAttention;
    if let Some(c) = h.checks.iter_mut().find(|c| c.id == "D-04") {
        c.state = CheckState::Fail;
        c.result = "Turned off on this system".into();
        c.detail = Some("Games run in a private space. This system does not allow it yet.".into());
        c.fix_command = Some("sudo sysctl -w kernel.unprivileged_userns_clone=1".into());
    }
    if let Some(c) = h.checks.iter_mut().find(|c| c.id == "D-09") {
        c.state = CheckState::Warn;
        c.result = "No 32-bit drivers found".into();
        c.detail = Some(
            "Older games may not start. Install your distribution's 32-bit Vulkan drivers.".into(),
        );
    }
    h
}

fn build(
    name: &str,
    family: RuntimeFamily,
    origin: RuntimeOrigin,
    label: &str,
    size: u64,
) -> LayerInfo {
    LayerInfo {
        build: RuntimeBuild {
            name: RuntimeName::new(name).unwrap_or_else(|_| {
                RuntimeName::new("UMU-Proton-10.0-4").unwrap_or_else(|_| unreachable!())
            }),
            family,
            version_key: name.into(),
            origin,
            origin_label: label.into(),
            path: PathBuf::from(format!("/home/sam/.local/share/easyplay/runtimes/{name}")),
            required_slr: SlrVariant::Steamrt3,
            display_name: name.replace('-', " ").replacen("Proton ", "Proton-", 0),
            has_fixes: family != RuntimeFamily::ValveProton,
            size_bytes: Some(size),
            discovered_at: baseline_now(),
            last_used_at: Some(baseline_now()),
        },
        size: Some(size),
        used_by: 0,
    }
}

/// The sample runtime catalogue.
pub fn runtimes_view() -> RuntimesView {
    let mut umu = build(
        "UMU-Proton-10.0-4",
        RuntimeFamily::UmuProton,
        RuntimeOrigin::Store,
        "EasyPlay",
        1_120_000_000,
    );
    umu.build.display_name = "UMU-Proton 10.0-4".into();
    umu.used_by = 21;
    let mut ge = build(
        "GE-Proton10-25",
        RuntimeFamily::GeProton,
        RuntimeOrigin::Store,
        "EasyPlay",
        1_480_000_000,
    );
    ge.build.display_name = "GE-Proton 10-25".into();
    ge.used_by = 3;
    let mut valve = build(
        "Proton-9.0",
        RuntimeFamily::ValveProton,
        RuntimeOrigin::External,
        "Steam",
        980_000_000,
    );
    valve.build.display_name = "Proton 9.0".into();
    RuntimesView {
        layers: vec![umu, ge, valve],
        default_label: Some("UMU-Proton 10.0-4".into()),
        slr: vec![(
            "Steam Linux Runtime 3.0 (sniper)".into(),
            "3.0.20251020".into(),
            Some(baseline_now().saturating_sub(Duration::from_millis(2 * DAY))),
        )],
        last_checked: Some(baseline_now().saturating_sub(Duration::from_millis(5 * H))),
        downloads: Vec::<DownloadInfo>::new(),
        adoptable: Vec::new(),
    }
}

fn statement(
    category: AccessCategory,
    key: &str,
    text: impl Into<String>,
    severity: StatementSeverity,
) -> AccessStatement {
    AccessStatement {
        category,
        key: key.into(),
        args: Default::default(),
        text: text.into(),
        detail: None,
        severity,
        mounts: Vec::new(),
        flags: Vec::new(),
        env: Vec::new(),
    }
}

/// English name of a shared folder: the well-known folder's, else the folder's own.
pub fn grant_name(g: &easyplay_core::model::FolderGrant) -> String {
    match g.origin {
        GrantOrigin::WellKnown(d) => user_dir_name(d).to_owned(),
        GrantOrigin::UserChosen => g.path.file_name().map_or_else(
            || g.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
    }
}

/// English name of a well-known folder.
pub fn user_dir_name(d: UserDir) -> &'static str {
    match d {
        UserDir::Desktop => "Desktop",
        UserDir::Documents => "Documents",
        UserDir::Downloads => "Downloads",
        UserDir::Music => "Music",
        UserDir::Pictures => "Pictures",
        UserDir::Videos => "Videos",
    }
}

/// The sample home folder.
pub const HOME: &str = "/home/sam";

/// Statements and issues for `policy`, following `sandbox::explain` and the rules of
/// `sandbox::validate` closely enough for the preview.
pub fn assess(name: &str, root: Option<&Path>, policy: &SandboxPolicy) -> PolicyAssessment {
    assess_in(Path::new(HOME), name, root, policy)
}

/// [`assess`] for a given home folder (sample data only; the service backend uses core's
/// `sandbox::explain::explain_policy`).
pub fn assess_in(
    home: &Path,
    name: &str,
    root: Option<&Path>,
    policy: &SandboxPolicy,
) -> PolicyAssessment {
    use AccessCategory as C;
    use StatementSeverity as S;
    let mut out = Vec::new();
    out.push(match policy.network {
        NetworkAccess::None => statement(
            C::Network,
            "access.network.none",
            "No internet or local network",
            S::Info,
        ),
        NetworkAccess::Full => statement(
            C::Network,
            "access.network.full",
            "Can use the internet and your local network",
            S::Notice,
        ),
    });
    let folder = root
        .and_then(|r| r.file_name())
        .map_or_else(|| name.to_owned(), |n| n.to_string_lossy().into_owned());
    let (key, text) = match policy.game_dir {
        GameDirAccess::FileOnly => (
            "access.own.file_only",
            "Can read only its program file".to_owned(),
        ),
        GameDirAccess::ReadOnly => (
            "access.own.read_only",
            format!("Can read files in {folder} (its own folder)"),
        ),
        GameDirAccess::ReadWrite => (
            "access.own.read_write",
            format!("Can read and change files in {folder} (its own folder)"),
        ),
        GameDirAccess::Protected => (
            "access.own.protected",
            "Can change its game folder; changes are kept separately".to_owned(),
        ),
    };
    out.push(statement(C::OwnFolder, key, text, S::Info));
    let mut personal = false;
    for g in &policy.folders {
        let n = grant_name(g);
        let (key, text) = match g.access {
            FolderAccess::ReadWrite => (
                "access.folders.read_write",
                format!("Can read and change {n}"),
            ),
            FolderAccess::ReadOnly => ("access.folders.read_only", format!("Can read {n}")),
        };
        let mut s = statement(C::YourFolders, key, text, S::Notice);
        s.args
            .insert("path".into(), g.canonical.display().to_string());
        out.push(s);
        personal = true;
    }
    if let SaveDataLocation::External { .. } = policy.save_data {
        out.push(statement(
            C::YourFolders,
            "access.folders.save_data",
            "Keeps its save data in your folder",
            S::Notice,
        ));
        personal = true;
    }
    match policy.removable {
        RemovableAccess::None => {}
        RemovableAccess::ReadOnly => {
            out.push(statement(
                C::YourFolders,
                "access.folders.removable_ro",
                "Can read removable drives",
                S::Notice,
            ));
            personal = true;
        }
        RemovableAccess::ReadWrite => {
            out.push(statement(
                C::YourFolders,
                "access.folders.removable_rw",
                "Can read and change removable drives",
                S::Notice,
            ));
            personal = true;
        }
    }
    if !personal {
        out.push(statement(
            C::YourFolders,
            "access.folders.none",
            "Cannot see your documents, pictures or other personal files",
            S::Info,
        ));
    }
    match policy.devices.controllers {
        ControllerAccess::None => {}
        ControllerAccess::Detected => out.push(statement(
            C::Devices,
            "access.devices.controllers",
            "Can use connected controllers",
            S::Info,
        )),
        ControllerAccess::AllInput => out.push(statement(
            C::Devices,
            "access.devices.all_input",
            "Can read all input devices, including keyboards",
            S::Warning,
        )),
    }
    if policy.devices.audio != easyplay_core::model::AudioAccess::None {
        out.push(statement(
            C::Devices,
            "access.devices.sound",
            "Can play sound",
            S::Info,
        ));
    }
    if policy.devices.audio == easyplay_core::model::AudioAccess::Full {
        out.push(statement(
            C::Devices,
            "access.devices.microphone",
            "Can use your microphone",
            S::Warning,
        ));
    }
    if policy.devices.camera {
        out.push(statement(
            C::Devices,
            "access.devices.camera",
            "Can use the camera",
            S::Notice,
        ));
    }
    if !policy.hardening.mask_hostname || !policy.hardening.mask_machine_id {
        out.push(statement(
            C::Identity,
            "access.identity.visible",
            "Can see this computer's name and ID",
            S::Notice,
        ));
    }
    if policy.hardening.seccomp == SeccompMode::Off {
        out.push(statement(
            C::HardeningChanges,
            "access.hardening.seccomp_off",
            "Risky system calls are not filtered",
            S::Warning,
        ));
    }
    let mut issues = Vec::new();
    for g in &policy.folders {
        let p = g.canonical.as_path();
        if p == home || p == Path::new("/") || p == Path::new("/home") {
            issues.push(PolicyIssue::error(
                IssueCode::PathDenied,
                Some(p.to_path_buf()),
                "sec.path.home|Your whole home folder can't be shared. Choose a folder inside it.",
            ));
        } else if p.starts_with(home.join(".ssh")) {
            issues.push(PolicyIssue::error(
                IssueCode::PathDenied,
                Some(p.to_path_buf()),
                "sec.path.denied|This folder holds sign-in keys and can't be shared.",
            ));
        }
        if !p.starts_with(home) && p != Path::new("/") {
            issues.push(PolicyIssue::warning(
                IssueCode::PathMissing,
                Some(p.to_path_buf()),
                "sec.path.missing|Not found",
            ));
        }
    }
    if policy.network == NetworkAccess::Full
        && policy
            .folders
            .iter()
            .any(|g| g.access == FolderAccess::ReadWrite)
    {
        issues.push(PolicyIssue::new(
            easyplay_core::sandbox::IssueSeverity::Info,
            IssueCode::NetworkWithWritableGrant,
            None,
            "sec.net.writable|With internet access, a program can upload or alter these files.",
        ));
    }
    PolicyAssessment {
        statements: out,
        issues,
    }
}

/// `apply_preset` on an empty host.
pub fn preset_policy(preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
    let host = HostFacts::empty(baseline_now());
    let mut p = apply_preset(policy, preset, &host);
    p.preset = classify(&p);
    if p.preset.is_custom() {
        p.preset = Preset::Custom { base: preset };
    }
    p
}

fn policy_for(preset: Preset) -> SandboxPolicy {
    let base = easyplay_core::sandbox::offline();
    let mut p = preset_policy(preset.base(), &base);
    if let Preset::Custom { .. } = preset {
        p.folders.push(easyplay_core::model::FolderGrant {
            path: PathBuf::from(format!("{HOME}/Music")),
            canonical: PathBuf::from(format!("{HOME}/Music")),
            access: FolderAccess::ReadOnly,
            origin: GrantOrigin::WellKnown(UserDir::Music),
            drive_letter: None,
            granted_at: baseline_now(),
        });
        p.preset = classify(&p);
    }
    p
}

fn fingerprint(seed: u64) -> Fingerprint {
    Fingerprint {
        size: 30_000_000 + seed * 1_000_003 % 90_000_000,
        mtime_ns: 0,
        quick_hash: [seed as u8; 32],
        full_hash: None,
    }
}

fn executable(
    id: u128,
    role: ExeRole,
    path: PathBuf,
    label: Option<&str>,
    now: Timestamp,
) -> Executable {
    Executable {
        id: ExecutableId::from_uuid(uuid::Uuid::from_u128(id)),
        role,
        canonical_path: path.clone(),
        path,
        rel_path: None,
        mount_point: None,
        fingerprint: fingerprint(id as u64),
        pe: PeInfo::default(),
        args_override: None,
        label: label.map(str::to_owned),
        added_at: now,
    }
}

/// What a sample game's files tell: Lantern Keep is a Unity game on Steam, Hollow Bell an
/// Unreal game with anti-cheat; the others take an engine by their name.
pub fn game_about(name: &str) -> easyplay_core::identity::GameAbout {
    use easyplay_core::identity::{AntiCheat, Engine, EngineGuess, ExeHints, GameAbout, Variant};
    let guess = |engine, variant| Some(EngineGuess { engine, variant });
    match name {
        "Lantern Keep" => GameAbout {
            engine: guess(Engine::Unity, Some(Variant::Il2Cpp)),
            hints: ExeHints {
                steam_api: true,
                ..ExeHints::default()
            },
        },
        "Hollow Bell" => GameAbout {
            engine: guess(Engine::Unreal, None),
            hints: ExeHints {
                anti_cheat: vec![AntiCheat::EasyAntiCheat],
                steam_api: true,
                ..ExeHints::default()
            },
        },
        _ => {
            let engines = [
                guess(Engine::Godot, None),
                guess(Engine::GameMaker, None),
                None,
                guess(Engine::RpgMaker, Some(Variant::Mz)),
                guess(Engine::RenPy, None),
            ];
            GameAbout {
                engine: engines[name.len() % engines.len()],
                hints: ExeHints::default(),
            }
        }
    }
}

/// The full record of a sample game.
pub fn game_detail(g: &GameSummary, _now: Timestamp) -> GameDetail {
    let root = g.root.as_deref().map(Path::to_path_buf);
    let seed = g.id.as_uuid().as_u128();
    let mut primary = executable(
        seed ^ 0xE0,
        ExeRole::Primary,
        g.primary_exe_path.to_path_buf(),
        None,
        g.added_at,
    );
    primary.pe.machine = easyplay_core::model::PeMachine::X86_64;
    if &*g.name == "Lantern Keep" {
        primary.pe.company = Some("Ember & Ash".into());
        primary.pe.product_name = Some("Lantern Keep".into());
        primary.pe.file_version = Some("1.4.2.0".into());
    }
    let mut exes = vec![primary];
    if let Some(r) = &root {
        exes.push(executable(
            seed ^ 0xC0FF,
            ExeRole::Tool,
            r.join("Config.exe"),
            Some(strings::details::SAMPLE_TOOL_LABEL()),
            g.added_at,
        ));
    }
    let mut launch = LaunchConfig::default();
    if &*g.name == "Lantern Keep" {
        launch.args = vec!["-screen-fullscreen".into(), "1".into()];
        launch.env.push(easyplay_core::model::EnvVar {
            name: "DXVK_HUD".into(),
            value: "fps".into(),
        });
    }
    let game = Game {
        id: g.id,
        name: g.name.to_string(),
        sort_name: g.sort_name.to_string(),
        name_is_user_set: false,
        root: root.clone(),
        added_at: g.added_at,
        updated_at: g.added_at,
        favorite: g.favorite,
        hidden: g.hidden,
        notes: if &*g.name == "Lantern Keep" {
            strings::details::SAMPLE_NOTE().to_owned()
        } else {
            String::new()
        },
        policy: policy_for(g.preset),
        policy_updated_at: g.added_at,
        launch,
        umu_id: Some(format!("umu-{}", 300_000 + g.name.len() * 1117)),
        store: None,
        runtime: RuntimeRef::Default,
        runtime_follow_updates: true,
        runtime_last_resolved: RuntimeName::new("UMU-Proton-10.0-4").ok(),
        prefix_tainted_at: g.last_played_at,
        prefix_prepared_with: RuntimeName::new("UMU-Proton-10.0-4").ok(),
        flags: GameFlags::default(),
        last_launched_at: g.last_played_at,
        steam_app_id: None,
        steam_app_id_is_user_set: false,
        executables: exes,
    };
    GameDetail {
        game,
        save_location: None,
        prefix_created_at: Some(g.added_at),
        runtime_resolved: Some("UMU-Proton 10.0-4".into()),
        runtime_missing: false,
        runtime_older: false,
        runtime_without_fixes: false,
        fix_title: Some(format!("{} (GOG)", g.name)),
        has_shortcut: &*g.name == "Lantern Keep",
    }
}

/// A setup draft for a sample program at `path`.
pub fn draft(path: &Path, now: Timestamp) -> GameDraft {
    let file = path
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    let parent = path.parent().map(Path::to_path_buf);
    let loose = parent
        .as_deref()
        .is_some_and(|p| p.ends_with("Downloads") || p == Path::new(HOME));
    let installer =
        file.to_lowercase().starts_with("setup") || file.to_lowercase().contains("install");
    let name = parent
        .as_deref()
        .filter(|_| !loose)
        .and_then(|p| p.file_name())
        .map_or(file.clone(), |n| n.to_string_lossy().into_owned());
    let root = if loose { None } else { parent.clone() };
    let mut policy = easyplay_core::sandbox::offline();
    policy.game_dir = if root.is_some() {
        GameDirAccess::ReadWrite
    } else {
        GameDirAccess::FileOnly
    };
    let candidates = parent
        .iter()
        .flat_map(|p| p.ancestors().take(3))
        .filter(|p| p.starts_with(HOME) && *p != Path::new(HOME))
        .map(Path::to_path_buf)
        .collect();
    let _ = now;
    GameDraft {
        path: path.to_path_buf(),
        canonical_path: path.to_path_buf(),
        mount_point: None,
        fingerprint: fingerprint(file.len() as u64),
        pe: PeInfo {
            product_name: Some(name.clone()),
            ..PeInfo::default()
        },
        imports: Vec::new(),
        name,
        name_is_user_set: false,
        proposal: GameRootProposal {
            kind: if loose {
                RootKind::LooseFile
            } else {
                RootKind::Folder
            },
            root: root.clone(),
            candidates,
            reason: RootReason::ExecutableFolder,
        },
        root,
        policy,
        launch: LaunchConfig::default(),
        runtime: RuntimeRef::Default,
        kind: if installer {
            EntryKind::Installer
        } else {
            EntryKind::Game
        },
        icon: None,
        hints: SetupHints {
            installer_like: installer,
            online_capable: file.to_lowercase().contains("online"),
            ..SetupHints::default()
        },
        umu_id: None,
        tags: Vec::new(),
        issues: Vec::new(),
    }
}

/// Store search results for `term` (none for a term containing "nothing").
pub fn steam_apps(term: &str) -> QueryPayload {
    let candidates = if term.to_lowercase().contains("nothing") {
        Vec::new()
    } else {
        vec![
            SearchCandidate {
                app_id: 1_547_000,
                name: format!("{term}: The Definitive Edition"),
            },
            SearchCandidate {
                app_id: 1_546_990,
                name: format!("{term} Trilogy"),
            },
        ]
    };
    QueryPayload::SteamApps {
        detected: None,
        candidates,
    }
}

/// Programs found in a sample folder.
pub fn folder_scan(folder: &Path) -> FolderScan {
    let name = folder
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let main = exe_of(&name);
    let p = |f: &str, role, filtered, size| ProgramFound {
        path: folder.join(f),
        role,
        filtered,
        size,
    };
    FolderScan {
        folder: folder.to_path_buf(),
        programs: vec![
            p(&main, ExeRole::Primary, false, 32_400_000),
            p("Config.exe", ExeRole::Tool, false, 1_100_000),
            p("unins000.exe", ExeRole::Tool, true, 3_200_000),
            p("vc_redist.x64.exe", ExeRole::Installer, true, 24_800_000),
        ],
        truncated: false,
        scanning: false,
        error: None,
    }
}

/// A launch failure the preview shows for games that need attention.
pub fn launch_failure(game: GameId, name: &str) -> UserError {
    UserError {
        code: ErrorCode::SbxGrantMissing,
        severity: Severity::Error,
        title: "This game needs a folder that isn't there".into(),
        message: format!(
            "{name} is allowed to use “Mods”, but that folder can't be found. It may have been moved or its drive is not connected."
        ),
        actions: vec![RecoveryAction::EditPermissions, RecoveryAction::Retry],
        details: Some("grant ~/Games/Mods: not found (ENOENT)".into()),
        game: Some(game),
        session: None,
    }
}

// ---- Statistics ----------------------------------------------------------------------------

struct PlayLog {
    /// (game index into the library, day number, start minute, duration ms)
    rows: Vec<(usize, i64, u32, u64)>,
}

fn play_log(lib: &LibrarySnapshot, now: Timestamp) -> PlayLog {
    let today = key_days(local_day(now, 0));
    let mut rows = Vec::new();
    for (i, g) in lib.games.iter().enumerate() {
        // Steam games have no sessions of EasyPlay's: Steam counts their time (ADR-0031).
        let Some(last) = g.last_played_at.filter(|_| !g.is_steam()) else {
            continue;
        };
        let last_day = key_days(local_day(last, 0));
        let n = g.session_count.clamp(1, 40) as i64;
        let per = g.total_playtime_ms.min(60 * H) / n as u64;
        for k in 0..n {
            let day = last_day - (k * 7 + (i as i64 % 3) * k) % 120;
            if day > today {
                continue;
            }
            let start = 18 * 60 + ((i as u32 * 37 + k as u32 * 53) % 240);
            let dur = per / 2 + (per * ((i as u64 + k as u64 * 13) % 10)) / 10;
            rows.push((i, day, start, dur.max(6 * M)));
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse((r.1, r.2)));
    PlayLog { rows }
}

fn range_days(range: &DateRange, earliest: i64) -> (i64, i64) {
    let to = key_days(easyplay_core::stats::rollup::date_key(range.to));
    let from = range.from.map_or(earliest.min(to), |d| {
        key_days(easyplay_core::stats::rollup::date_key(d))
    });
    (from, to)
}

fn series(log: &PlayLog, games: Option<usize>, from: i64, to: i64) -> Series {
    let days = to - from + 1;
    let bucket = easyplay_core::stats::queries::bucket_for(days);
    let start_of = |d: i64| match bucket {
        Bucket::Day => d,
        Bucket::Week => d - (d + 3).rem_euclid(7),
        Bucket::Month => {
            let k = day_key(d);
            key_days(k / 100 * 100 + 1)
        }
    };
    let mut points: Vec<(i32, u64)> = Vec::new();
    let mut d = start_of(from);
    while d <= to {
        points.push((day_key(d) as i32, 0));
        d = match bucket {
            Bucket::Day => d + 1,
            Bucket::Week => d + 7,
            Bucket::Month => {
                let k = day_key(d);
                let (y, m) = (k / 10_000, (k / 100) % 100);
                let (y, m) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                key_days(y * 10_000 + m * 100 + 1)
            }
        };
    }
    for (gi, day, _, ms) in &log.rows {
        if games.is_some_and(|g| g != *gi) || *day < from || *day > to {
            continue;
        }
        let key = day_key(start_of(*day)) as i32;
        if let Some(p) = points.iter_mut().find(|p| p.0 == key) {
            p.1 += ms;
        }
    }
    Series { bucket, points }
}

fn history_row(lib: &LibrarySnapshot, r: &(usize, i64, u32, u64), n: usize) -> HistoryRow {
    let g = &lib.games[r.0];
    let started = Timestamp::from_millis(r.1 * DAY as i64 + i64::from(r.2) * M as i64);
    let reason = match n % 11 {
        3 => EndReason::StoppedByUser,
        7 => EndReason::Crashed { signal: 11 },
        _ => EndReason::Exited { code: 0 },
    };
    HistoryRow {
        id: SessionId::from_uuid(uuid::Uuid::from_u128(0x5E55_0000 + n as u128)),
        game_id: Some(g.id),
        game_name: g.name.to_string(),
        kind: SessionKind::Game,
        started_at: started,
        duration_ms: r.3,
        end_reason: Some(reason),
        counted: true,
    }
}

/// Answer a statistics request over the sample library.
pub fn stats(req: &StatsRequest, lib: &LibrarySnapshot, now: Timestamp) -> StatsResult {
    let log = play_log(lib, now);
    let earliest = log.rows.last().map_or(0, |r| r.1);
    let totals = |gi: Option<usize>, from: i64, to: i64| {
        let mut t = Totals::default();
        let mut seen = std::collections::BTreeSet::new();
        for r in &log.rows {
            if gi.is_some_and(|g| g != r.0) || r.1 < from || r.1 > to {
                continue;
            }
            t.ms += r.3;
            t.sessions += 1;
            seen.insert(r.0);
        }
        t.games_played = seen.len() as u32;
        t
    };
    match req {
        StatsRequest::LibraryOverview { range } => {
            let (from, to) = range_days(range, earliest);
            let mut per: HashMap<usize, u64> = HashMap::new();
            for r in log.rows.iter().filter(|r| r.1 >= from && r.1 <= to) {
                *per.entry(r.0).or_default() += r.3;
            }
            let mut top: Vec<(usize, u64)> = per.into_iter().collect();
            top.sort_by_key(|t| std::cmp::Reverse(t.1));
            StatsResult::Library(LibraryOverview {
                totals: totals(None, from, to),
                series: series(&log, None, from, to),
                top: top
                    .into_iter()
                    .take(10)
                    .map(|(gi, ms)| Ranking {
                        key: GameKey::Game(lib.games[gi].id),
                        ms,
                    })
                    .collect(),
                current_streak: 4,
                longest_streak: 11,
            })
        }
        StatsRequest::GameOverview { game, range } => {
            let gi = lib.games.iter().position(|g| g.id == *game);
            let (from, to) = range_days(range, earliest);
            let recent: Vec<HistoryRow> = log
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| Some(r.0) == gi)
                .take(5)
                .map(|(n, r)| history_row(lib, r, n))
                .collect();
            let g = gi.map(|i| &lib.games[i]);
            let all = totals(gi, i64::MIN / 4, i64::MAX / 4);
            StatsResult::Game(GameOverview {
                stats: g.map(|g| GameStats {
                    game_id: g.id,
                    total_ms: g.total_playtime_ms,
                    sessions: g.session_count,
                    failed_launches: 0,
                    first_played_at: g.first_played_at,
                    last_played_at: g.last_played_at,
                    longest_ms: log
                        .rows
                        .iter()
                        .filter(|r| Some(r.0) == gi)
                        .map(|r| r.3)
                        .max()
                        .unwrap_or(0),
                    last_launch_at: g.last_played_at,
                }),
                totals: totals(gi, from, to),
                series: series(&log, gi, from, to),
                last_session_ms: recent.first().map(|r| r.duration_ms),
                average_ms: (all.sessions > 0).then(|| all.ms / u64::from(all.sessions)),
                recent,
            })
        }
        StatsRequest::SessionHistory {
            game,
            page,
            page_size,
            ..
        } => {
            let gi = game.and_then(|id| lib.games.iter().position(|g| g.id == id));
            let rows: Vec<HistoryRow> = log
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| game.is_none() || Some(r.0) == gi)
                .skip((*page * *page_size) as usize)
                .take(*page_size as usize + 1)
                .map(|(n, r)| history_row(lib, r, n))
                .collect();
            let has_more = rows.len() > *page_size as usize;
            StatsResult::History(HistoryPage {
                rows: rows.into_iter().take(*page_size as usize).collect(),
                has_more,
            })
        }
        StatsRequest::TopGames { range, limit } => {
            let StatsResult::Library(o) =
                stats(&StatsRequest::LibraryOverview { range: *range }, lib, now)
            else {
                return StatsResult::Top(Vec::new());
            };
            StatsResult::Top(o.top.into_iter().take(*limit as usize).collect())
        }
        StatsRequest::WeekdayDistribution { range } => {
            let (from, to) = range_days(range, earliest);
            let mut w = [0u64; 7];
            for r in log.rows.iter().filter(|r| r.1 >= from && r.1 <= to) {
                w[(r.1 + 3).rem_euclid(7) as usize] += r.3;
            }
            StatsResult::Weekdays(w)
        }
    }
}

// ---- The preview backend ----------------------------------------------------------------------

struct DemoState {
    library: Arc<LibrarySnapshot>,
    prefs: Arc<Preferences>,
    host: Arc<HostView>,
    runtimes: Arc<RuntimesView>,
    details: HashMap<GameId, GameDetail>,
    events: VecDeque<UiEvent>,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    now: Timestamp,
    next: u128,
    /// Launches cancelled before they reached the game.
    cancelled: Vec<GameId>,
    /// Pending setup queue (paths after the current one).
    setup_queue: VecDeque<PathBuf>,
    /// Batch size of the current setup queue.
    batch: u32,
    /// The Steam games kept aside while "Show my Steam games" is off (ADR-0031).
    steam_off: Vec<GameSummary>,
    /// The notes of Steam games (the preview keeps no record for them).
    steam_notes: HashMap<GameId, String>,
    /// The launch options of Steam games, as saved in the preview (ADR-0031 §6a).
    steam_launch: HashMap<GameId, String>,
}

/// A preview backend over the sample library. Edits stay in memory; launches are simulated
/// on a helper thread (never on the UI thread).
#[derive(Clone)]
pub struct DemoBackend {
    state: Arc<Mutex<DemoState>>,
    /// Real time between simulated launch phases.
    step: Duration,
}

impl DemoBackend {
    /// The preview (`EASYPLAY_PREVIEW=1`): the sample library with the sample Steam games,
    /// and "Show my Steam games" on.
    pub fn new(now: Timestamp) -> Self {
        let b = Self::with_library(preview_library(now), now);
        {
            let mut s = b.lock();
            let mut p = (*s.prefs).clone();
            p.steam.show_games = true;
            s.prefs = Arc::new(p);
        }
        b
    }

    /// A preview backend over `library`.
    pub fn with_library(library: LibrarySnapshot, now: Timestamp) -> Self {
        Self {
            state: Arc::new(Mutex::new(DemoState {
                library: Arc::new(library),
                prefs: Arc::new(preferences()),
                host: Arc::new(host_view()),
                runtimes: Arc::new(runtimes_view()),
                details: HashMap::new(),
                events: VecDeque::new(),
                waker: None,
                now,
                next: 1,
                cancelled: Vec::new(),
                setup_queue: VecDeque::new(),
                batch: 0,
                steam_off: Vec::new(),
                steam_notes: HashMap::new(),
                steam_launch: HashMap::new(),
            })),
            step: Duration::from_millis(900),
        }
    }

    /// Start at onboarding (preferences say it is not completed).
    pub fn first_run(self) -> Self {
        {
            let mut s = self.lock();
            let mut p = (*s.prefs).clone();
            p.onboarding.completed = false;
            s.prefs = Arc::new(p);
        }
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, DemoState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn wake(s: &DemoState) {
        if let Some(w) = &s.waker {
            w();
        }
    }

    fn update_game(s: &mut DemoState, id: GameId, f: impl FnOnce(&mut GameSummary)) {
        if let Some(g) = s.library.get(id) {
            let mut g = g.clone();
            f(&mut g);
            let now = s.now;
            s.library = Arc::new(s.library.with_game(g, now));
            s.events.push_back(UiEvent::LibraryChanged);
        }
    }

    fn toast(s: &mut DemoState, text: String, undo: Option<UiCommand>) {
        s.events.push_back(UiEvent::Toast(ToastMsg {
            text,
            level: ToastLevel::Success,
            undo: undo.map(Box::new),
        }));
    }

    fn detail(s: &mut DemoState, id: GameId) -> Option<&mut GameDetail> {
        if !s.details.contains_key(&id) {
            let g = s.library.get(id)?.clone();
            let d = game_detail(&g, s.now);
            s.details.insert(id, d);
        }
        s.details.get_mut(&id)
    }

    fn next_setup(s: &mut DemoState) {
        if let Some(p) = s.setup_queue.pop_front() {
            let index = s.batch - s.setup_queue.len() as u32;
            let d = draft(&p, s.now);
            s.events.push_back(UiEvent::SetupRequested {
                draft: Box::new(d),
                context: SetupContext {
                    origin: SetupOrigin::InsideApp,
                    queue: (index, s.batch),
                },
            });
        }
    }

    /// "Show my Steam games" turned `on` or off: the Steam games join or leave the library.
    fn steam_enabled(s: &mut DemoState, on: bool) {
        let now = s.now;
        if on {
            let games = if s.steam_off.is_empty() {
                steam_samples(now)
            } else {
                std::mem::take(&mut s.steam_off)
            };
            let mut lib = (*s.library).clone();
            for g in games {
                if lib.get(g.id).is_none() {
                    lib = lib.with_game(g, now);
                }
            }
            s.library = Arc::new(lib);
        } else {
            let steam: Vec<GameSummary> = s
                .library
                .games
                .iter()
                .filter(|g| g.is_steam())
                .cloned()
                .collect();
            let mut lib = (*s.library).clone();
            for g in &steam {
                lib = lib.without_game(g.id, now);
            }
            s.library = Arc::new(lib);
            s.steam_off = steam;
        }
        s.events.push_back(UiEvent::LibraryChanged);
    }

    /// A request to Steam, as the preview answers it: what the real app would say.
    fn steam(s: &mut DemoState, a: crate::bridge::SteamAction) {
        use crate::bridge::SteamAction as A;
        let name = |s: &DemoState, id: GameId| s.library.get(id).map(|g| g.name.to_string());
        match a {
            A::SetEnabled(on) => {
                let mut p = (*s.prefs).clone();
                p.steam.show_games = on;
                s.prefs = Arc::new(p);
                s.events.push_back(UiEvent::PreferencesChanged);
                Self::steam_enabled(s, on);
            }
            A::Rescan => {
                if s.prefs.steam.show_games {
                    let n = s.library.games.iter().filter(|g| g.is_steam()).count();
                    Self::toast(s, strings::steam::found(n), None);
                }
            }
            A::Update(id) => {
                let n = name(s, id);
                Self::toast(s, strings::steam::opened_downloads(n.as_deref()), None);
            }
            A::OpenInSteam(id) => {
                let n = name(s, id).unwrap_or_default();
                Self::toast(s, strings::steam::opened_in_steam(&n), None);
            }
            A::Details { id, reply_to } => {
                let payload = match s.library.get(id).and_then(steam_detail) {
                    Some(mut d) => {
                        d.notes = s.steam_notes.get(&id).cloned().unwrap_or_default();
                        if let Some(l) = s.steam_launch.get(&id) {
                            d.launch_options = Some(l.clone());
                        }
                        QueryPayload::Steam(Box::new(d))
                    }
                    None => QueryPayload::Failed(launch_failure(id, "")),
                };
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload,
                });
            }
            // The preview's Steam is closed: the options are kept for the page.
            A::SetLaunchOptions {
                id,
                options,
                reply_to,
            } => {
                s.steam_launch.insert(id, options);
                let n = name(s, id).unwrap_or_default();
                Self::toast(s, strings::steam::launch::saved(&n), None);
                Self::steam(s, A::Details { id, reply_to });
            }
            A::CloseSteam(_) => Self::toast(s, strings::steam::asked_to_close().to_owned(), None),
            // A file manager or a program in the game's Windows storage: nothing to show in
            // the preview, as for the folders of the other games.
            A::OpenStorageFolder(_) | A::WindowsSettings(_) | A::RunProgram { .. } => {}
        }
    }

    fn simulate_launch(&self, id: GameId) {
        let state = self.state.clone();
        let step = self.step;
        std::thread::spawn(move || {
            let push = |st: LaunchState, activity: Option<Activity>| -> bool {
                let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
                if s.cancelled.contains(&id) {
                    return false;
                }
                s.events.push_back(UiEvent::LaunchProgress {
                    game: id,
                    state: st,
                });
                if let Some(a) = activity {
                    Self::update_game(&mut s, id, |g| g.activity = a);
                }
                Self::wake(&s);
                true
            };
            let (name, fails) = {
                let s = state.lock().unwrap_or_else(PoisonError::into_inner);
                s.library
                    .get(id)
                    .map(|g| (g.name.to_string(), g.flags.needs_attention))
                    .unwrap_or_default()
            };
            let steps = [
                (LaunchState::Validating, LaunchStateTag::Validating),
                (
                    LaunchState::Preparing {
                        phase: Some(UmuPhase::UpgradingPrefix),
                    },
                    LaunchStateTag::Preparing,
                ),
                (
                    LaunchState::Starting { phase: None },
                    LaunchStateTag::Starting,
                ),
            ];
            for (st, tag) in steps {
                if !push(st, Some(Activity::Launching(tag))) {
                    return;
                }
                std::thread::sleep(step);
            }
            if fails {
                push(
                    LaunchState::Failed(Box::new(launch_failure(id, &name))),
                    Some(Activity::Idle),
                );
                return;
            }
            let since = state.lock().unwrap_or_else(PoisonError::into_inner).now;
            push(
                LaunchState::Running {
                    session: SessionId::from_uuid(uuid::Uuid::from_u128(0xAB)),
                },
                Some(Activity::Running { since }),
            );
        });
    }

    fn handle(&self, s: &mut DemoState, cmd: UiCommand) {
        let name_of = |s: &DemoState, id| {
            s.library
                .get(id)
                .map(|g| g.name.to_string())
                .unwrap_or_default()
        };
        match cmd {
            UiCommand::SetFavorite { id, favorite } => {
                Self::update_game(s, id, |g| g.favorite = favorite);
            }
            UiCommand::SetHidden { id, hidden } => {
                Self::update_game(s, id, |g| g.hidden = hidden);
            }
            UiCommand::SetTags { id, tags } => {
                let mut infos = s.library.tags.clone();
                let mut ids = Vec::new();
                for t in &tags {
                    let found = infos.iter().find(|i| *i.name == *t.as_str()).map(|i| i.id);
                    let tid = found.unwrap_or_else(|| {
                        let tid = TagId::new(100 + infos.len() as i64);
                        infos.push(easyplay_core::library::TagInfo {
                            id: tid,
                            name: Arc::from(t.as_str()),
                            color: TagColor::new((infos.len() % 8) as u8).unwrap_or_default(),
                            count: 0,
                        });
                        tid
                    });
                    ids.push(tid);
                }
                let now = s.now;
                s.library = Arc::new(s.library.with_tags(infos, now));
                Self::update_game(s, id, |g| g.tags = ids.into_iter().collect());
            }
            UiCommand::ApplyPreset { ids, preset } => {
                let mut changed = 0;
                let mut skipped = 0;
                for id in ids {
                    let running = s
                        .library
                        .get(id)
                        .is_some_and(|g| g.activity != Activity::Idle);
                    if running {
                        skipped += 1;
                        continue;
                    }
                    changed += 1;
                    if let Some(d) = Self::detail(s, id) {
                        d.game.policy = preset_policy(preset, &d.game.policy);
                    }
                    Self::update_game(s, id, |g| {
                        g.preset = preset.into();
                        g.network = if preset == BasePreset::Offline {
                            NetworkAccess::None
                        } else {
                            NetworkAccess::Full
                        };
                    });
                }
                Self::toast(s, strings::library::bulk_summary(changed, skipped), None);
            }
            UiCommand::UpdateGame { id, edits } if id.is_steam() => {
                // Only the notes are EasyPlay's to change (ADR-0031).
                for e in edits {
                    if let GameEdit::Notes(n) = e {
                        s.steam_notes.insert(id, n);
                    }
                }
            }
            UiCommand::UpdateGame { id, edits } => {
                let mut summary_edits: Vec<GameEdit> = Vec::new();
                if let Some(d) = Self::detail(s, id) {
                    for e in edits {
                        match &e {
                            GameEdit::Name(n) => {
                                d.game.name.clone_from(n);
                                d.game.name_is_user_set = true;
                            }
                            GameEdit::ResetName => d.game.name_is_user_set = false,
                            GameEdit::Notes(n) => d.game.notes.clone_from(n),
                            GameEdit::Policy(p) => d.game.policy = (**p).clone(),
                            GameEdit::Launch(l) => d.game.launch = (**l).clone(),
                            GameEdit::Runtime { reference, follow } => {
                                d.game.runtime = reference.clone();
                                d.game.runtime_follow_updates = *follow;
                            }
                            GameEdit::UmuId(u) => d.game.umu_id.clone_from(u),
                            GameEdit::Kind(_) => {}
                            GameEdit::SteamAppId(a) => {
                                d.game.steam_app_id = *a;
                                d.game.steam_app_id_is_user_set = a.is_some();
                            }
                            GameEdit::ExeRole(x, r) => {
                                if *r == ExeRole::Primary {
                                    for e in &mut d.game.executables {
                                        if e.role == ExeRole::Primary {
                                            e.role = ExeRole::Alternate;
                                        }
                                    }
                                }
                                if let Some(e) = d.game.executables.iter_mut().find(|e| e.id == *x)
                                {
                                    e.role = *r;
                                }
                            }
                            GameEdit::ExeLabel(x, l) => {
                                if let Some(e) = d.game.executables.iter_mut().find(|e| e.id == *x)
                                {
                                    e.label.clone_from(l);
                                }
                            }
                            GameEdit::ExeArgs(x, a) => {
                                if let Some(e) = d.game.executables.iter_mut().find(|e| e.id == *x)
                                {
                                    e.args_override.clone_from(a);
                                }
                            }
                            GameEdit::RemoveExe(x) => d.game.executables.retain(|e| e.id != *x),
                        }
                        summary_edits.push(e);
                    }
                }
                Self::update_game(s, id, |g| {
                    for e in &summary_edits {
                        match e {
                            GameEdit::Name(n) => {
                                g.name = Arc::from(n.as_str());
                                g.sort_name = Arc::from(sort_name_of(n));
                            }
                            GameEdit::Policy(p) => {
                                g.preset = p.preset;
                                g.network = p.network;
                            }
                            GameEdit::Kind(k) => g.kind = *k,
                            _ => {}
                        }
                    }
                });
            }
            UiCommand::RemoveGame { id, .. } => {
                let name = name_of(s, id);
                let now = s.now;
                s.library = Arc::new(s.library.without_game(id, now));
                s.events.push_back(UiEvent::LibraryChanged);
                Self::toast(s, strings::feedback::removed(&name), None);
            }
            UiCommand::Launch { id, .. } if id.is_steam() => {
                // Handed to Steam: no phases of EasyPlay's.
                let name = name_of(s, id);
                Self::toast(s, strings::steam::asked_to_play(&name), None);
            }
            UiCommand::Launch { id, .. } => {
                s.cancelled.retain(|g| *g != id);
                self.simulate_launch(id);
            }
            UiCommand::CancelLaunch { id } if id.is_steam() => {}
            UiCommand::CancelLaunch { id } => {
                s.cancelled.push(id);
                s.events.push_back(UiEvent::LaunchProgress {
                    game: id,
                    state: LaunchState::Cancelled,
                });
                Self::update_game(s, id, |g| g.activity = Activity::Idle);
            }
            // Steam runs its games: EasyPlay has no Stop for them.
            UiCommand::Stop { id, .. } if id.is_steam() => {}
            UiCommand::Stop { id, .. } => {
                let now = s.now;
                s.events.push_back(UiEvent::LaunchProgress {
                    game: id,
                    state: LaunchState::Finished {
                        reason: EndReason::StoppedByUser,
                    },
                });
                Self::update_game(s, id, |g| {
                    if let Activity::Running { since } = g.activity {
                        g.total_playtime_ms += now.since(since).map_or(0, |d| d.as_millis() as u64);
                    }
                    g.activity = Activity::Idle;
                    g.last_played_at = Some(now);
                });
                let name = name_of(s, id);
                Self::toast(s, strings::feedback::stopped(&name), None);
            }
            UiCommand::OpenPaths { paths, origin } => {
                let _ = origin;
                if paths.is_empty() {
                    return;
                }
                if paths.len() == 1 && paths[0].extension().is_none() {
                    // A folder: the add-game sheet scans it.
                    s.events.push_back(UiEvent::QueryResult {
                        id: easyplay_core::ids::QueryId::from_uuid(uuid::Uuid::nil()),
                        payload: QueryPayload::Folder(folder_scan(&paths[0])),
                    });
                    return;
                }
                if let Some(bad) = paths.iter().find(|p| {
                    !p.extension().is_some_and(|e| {
                        e.eq_ignore_ascii_case("exe") || e.eq_ignore_ascii_case("msi")
                    })
                }) {
                    s.events.push_back(UiEvent::Failure(UserError {
                        code: ErrorCode::IdNotWindowsProgram,
                        severity: Severity::Error,
                        title: "This isn't a Windows program EasyPlay can run".into(),
                        message: format!(
                            "“{}” is not a Windows program. EasyPlay opens .exe and .msi files.",
                            bad.file_name()
                                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
                        ),
                        actions: vec![],
                        details: None,
                        game: None,
                        session: None,
                    }));
                    return;
                }
                s.setup_queue = paths.into_iter().collect();
                s.batch = s.setup_queue.len() as u32;
                Self::next_setup(s);
            }
            UiCommand::ScanFolder { folder, reply_to } => {
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Folder(folder_scan(&folder)),
                });
            }
            UiCommand::CreateGame { draft, then } => {
                let i = s.library.games.len() + 0x100 + s.next as usize;
                s.next += 1;
                let now = s.now;
                let mut g = summary(i, spec("", draft.policy.preset, 0, None), now);
                g.name = Arc::from(draft.name.as_str());
                g.sort_name = Arc::from(sort_name_of(&draft.name));
                g.cover = None;
                g.preset = draft.policy.preset;
                g.network = draft.policy.network;
                g.kind = draft.kind;
                g.primary_exe_path = Arc::from(draft.path.as_path());
                g.root = draft.root.as_deref().map(Arc::from);
                g.added_at = now;
                let id = g.id;
                s.library = Arc::new(s.library.with_game(g, now));
                s.events.push_back(UiEvent::LibraryChanged);
                match then {
                    AfterCreate::Launch => self.simulate_launch(id),
                    AfterCreate::Nothing | AfterCreate::ApplyToRest => {
                        Self::toast(s, strings::feedback::added(&draft.name), None);
                    }
                }
                Self::next_setup(s);
            }
            UiCommand::LoadGame { id, reply_to } => {
                let payload = match Self::detail(s, id) {
                    Some(d) => QueryPayload::Game(Box::new(d.clone())),
                    None => QueryPayload::Failed(launch_failure(id, "")),
                };
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload,
                });
            }
            UiCommand::AboutGame { id, reply_to } => {
                let name = name_of(s, id);
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::About(Box::new(game_about(&name))),
                });
            }
            UiCommand::MeasurePrefix { reply_to, .. } => {
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::PrefixSize(2_254_857_830),
                });
            }
            UiCommand::FindSteamApp { id, term, reply_to } => {
                let name = term.unwrap_or_else(|| name_of(s, id));
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: steam_apps(&name),
                });
            }
            UiCommand::SearchFixes { text, reply_to } => {
                let all = [
                    ("umu-367520", "Hollow Knight", "GOG"),
                    ("umu-504230", "Celeste", "EGS"),
                    ("umu-1145360", "Hades", "Steam"),
                    ("umu-311690", "Lantern Keep", "GOG"),
                    ("umu-311777", "Lantern Keep: Remastered", "GOG"),
                ];
                let q = text.to_lowercase();
                let found = all
                    .iter()
                    .filter(|(_, t, _)| q.is_empty() || t.to_lowercase().contains(&q))
                    .map(|(id, t, st)| FixEntry {
                        umu_id: (*id).into(),
                        title: (*t).into(),
                        store: (*st).into(),
                    })
                    .collect();
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Fixes(found),
                });
            }
            UiCommand::QueryStats { request, reply_to } => {
                let r = stats(&request, &s.library, s.now);
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Stats(r),
                });
            }
            UiCommand::SetPreference { preview, .. } => {
                let steam = preview.steam.show_games;
                let was = s.prefs.steam.show_games;
                s.prefs = Arc::new(*preview);
                s.events.push_back(UiEvent::PreferencesChanged);
                if steam != was {
                    Self::steam_enabled(s, steam);
                }
            }
            UiCommand::SetDefaultHandler { enable } => {
                let mut h = (*s.host).clone();
                h.handler = if enable {
                    HandlerState::EasyPlay
                } else {
                    HandlerState::Other("Wine Windows Program Loader".into())
                };
                s.host = Arc::new(h);
                s.events.push_back(UiEvent::HostChanged);
            }
            UiCommand::CompleteOnboarding { .. } => {
                let mut p = (*s.prefs).clone();
                p.onboarding.completed = true;
                s.prefs = Arc::new(p);
                s.events.push_back(UiEvent::PreferencesChanged);
            }
            UiCommand::RefreshHostFacts => {
                s.events.push_back(UiEvent::HostChanged);
            }
            UiCommand::AnswerPrompt { .. } => {}
            UiCommand::DiscardDraft => Self::next_setup(s),
            UiCommand::ImportArtwork { id, slot, source } => {
                if let crate::bridge::ArtworkSource::File(p) = &source
                    && p.to_string_lossy().contains("huge")
                {
                    s.events.push_back(UiEvent::ArtworkFailed {
                        game: id,
                        slot,
                        error: UserError {
                            code: ErrorCode::ArtTooLarge,
                            severity: Severity::Error,
                            title: "This image is too large".into(),
                            message: "This image is too large (max 64 megapixels).".into(),
                            actions: vec![],
                            details: None,
                            game: Some(id),
                            session: None,
                        },
                    });
                    return;
                }
                let seed = s.next as u64 + 500;
                s.next += 1;
                Self::update_game(s, id, |g| g.cover = Some(art_key(seed)));
            }
            UiCommand::ClearArtwork { id, slot } => {
                if slot == easyplay_core::model::ArtSlot::Cover {
                    Self::update_game(s, id, |g| g.cover = None);
                }
            }
            UiCommand::FetchSteamArtwork { id, .. } => {
                let seed = s.next as u64 + 900;
                s.next += 1;
                Self::update_game(s, id, |g| g.cover = Some(art_key(seed)));
            }
            UiCommand::ResetPrefix { id, .. } => {
                let name = name_of(s, id);
                Self::toast(s, strings::feedback::prefix_reset(&name), None);
            }
            UiCommand::ClearStatistics => {
                Self::toast(s, strings::stats::CLEARED().to_owned(), None);
            }
            UiCommand::RemoveRuntime { name } => {
                let mut r = (*s.runtimes).clone();
                r.layers.retain(|l| l.build.name != name);
                s.runtimes = Arc::new(r);
                s.events.push_back(UiEvent::RuntimesChanged);
            }
            UiCommand::RunTool { id, .. } if id.is_steam() => {}
            UiCommand::RunTool { id, .. } => {
                // A tool that needs the internet asks first (UF-18).
                let prompt = PromptRequest {
                    id: PromptId::from_uuid(uuid::Uuid::from_u128(0x9000 + s.next)),
                    kind: PromptKind::ToolNeedsNetwork {
                        game: id,
                        tool: easyplay_core::model::Tool::Winetricks {
                            verbs: vec!["vcrun2019".into()],
                        },
                        prefix_tainted: true,
                    },
                };
                s.next += 1;
                s.events.push_back(UiEvent::Prompt(prompt));
            }
            UiCommand::SetShortcut { id, enable } => {
                let mut h = (*s.host).clone();
                if enable {
                    let name = name_of(s, id);
                    h.shortcuts.push(ShortcutInfo { game: id, name });
                } else {
                    h.shortcuts.retain(|x| x.game != id);
                }
                s.host = Arc::new(h);
                s.events.push_back(UiEvent::HostChanged);
            }
            UiCommand::AddExecutable { .. }
            | UiCommand::LocateExecutable { .. }
            | UiCommand::RescanMissing
            | UiCommand::ProvisionRuntime { .. }
            | UiCommand::AdoptRuntime { .. }
            | UiCommand::RefreshRuntimes
            | UiCommand::VerifyRuntime
            | UiCommand::ResetGameStatistics { .. }
            | UiCommand::RemoveSession { .. }
            | UiCommand::OpenFolder(_)
            | UiCommand::ViewLog { .. }
            | UiCommand::RunDoctor
            | UiCommand::SaveReport { .. }
            | UiCommand::DismissCrash
            | UiCommand::ResolveDatabase(_)
            | UiCommand::Quit => {}
            UiCommand::Steam(a) => Self::steam(s, a),
            // Sample Friends data comes with the Friends page (ADR-0033).
            UiCommand::Friends(_) => {}
            UiCommand::ListSessions { reply_to, .. } => {
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Sessions(Vec::new()),
                });
            }
            UiCommand::Inspect { reply_to, .. }
            | UiCommand::ReadLog { reply_to, .. }
            | UiCommand::ListProcesses { reply_to, .. }
            | UiCommand::BuildReport { reply_to, .. } => {
                let e = easyplay_core::error::catalog::describe(
                    &easyplay_core::error::Error::new(ErrorCode::IntNotAvailable),
                    &easyplay_core::error::catalog::DescribeCtx::plain(),
                );
                s.events.push_back(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Failed(e),
                });
            }
        }
    }
}

impl Backend for DemoBackend {
    fn send(&self, cmd: UiCommand) {
        let mut s = self.lock();
        self.handle(&mut s, cmd);
        Self::wake(&s);
    }

    fn try_event(&self) -> Option<UiEvent> {
        self.lock().events.pop_front()
    }

    fn library(&self) -> Arc<LibrarySnapshot> {
        self.lock().library.clone()
    }

    fn preferences(&self) -> Arc<Preferences> {
        self.lock().prefs.clone()
    }

    fn host(&self) -> Arc<HostView> {
        self.lock().host.clone()
    }

    fn runtimes(&self) -> Arc<RuntimesView> {
        self.lock().runtimes.clone()
    }

    fn assess(
        &self,
        _game: Option<GameId>,
        name: &str,
        root: Option<&Path>,
        policy: &SandboxPolicy,
    ) -> PolicyAssessment {
        assess(name, root, policy)
    }

    fn preset_policy(&self, preset: BasePreset, policy: &SandboxPolicy) -> SandboxPolicy {
        preset_policy(preset, policy)
    }

    fn set_waker(&self, waker: Arc<dyn Fn() + Send + Sync>) {
        self.lock().waker = Some(waker);
    }

    fn is_preview(&self) -> bool {
        true
    }
}

/// Tag names as `TagName`s (drops invalid ones).
pub fn tag_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<TagName> {
    names
        .into_iter()
        .filter_map(|n| TagName::new(n).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_library_is_sorted_and_indexed() {
        let lib = library(baseline_now());
        assert_eq!(lib.games.len(), 24);
        assert!(
            lib.games
                .windows(2)
                .all(|w| w[0].sort_name <= w[1].sort_name)
        );
        assert_eq!(lib.tags.len(), 3);
        assert!(lib.totals.favorites >= 3);
    }

    #[test]
    fn assess_explains_network_and_folders() {
        let p = easyplay_core::sandbox::online();
        let a = assess("X", Some(Path::new("/home/sam/Games/X")), &p);
        assert!(a.statements.iter().any(|s| s.key == "access.network.full"));
        assert!(a.issues.is_empty());
    }

    #[test]
    fn stats_answer_every_request() {
        let now = baseline_now();
        let lib = library(now);
        let today = easyplay_core::stats::rollup::key_date(local_day(now, 0)).unwrap();
        let r = stats(
            &StatsRequest::LibraryOverview {
                range: DateRange::last_days(today, 30),
            },
            &lib,
            now,
        );
        let StatsResult::Library(o) = r else { panic!() };
        assert_eq!(o.series.points.len(), 30);
        assert!(o.totals.ms > 0);
        assert!(!o.top.is_empty());
    }
}

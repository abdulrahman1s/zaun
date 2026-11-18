//! The only place where product names, identifiers and tunable constants are written.
//!
//! Layer 1. Contains no logic and never reads the environment at run time (values injected at
//! build time through `option_env!` are constants).

use std::time::Duration;

/// Product name shown to users (OQ-1).
pub const APP_NAME: &str = "Zaun";
/// Reverse-DNS application identifier (OQ-1).
pub const APP_ID: &str = "io.github.zaun.Zaun";
/// Name of Zaun's directory under each XDG root.
pub const DIR_NAME: &str = "zaun";
/// Name of the main binary.
pub const BIN_NAME: &str = "zaun";
/// Name of the init shim binary.
pub const INIT_BIN_NAME: &str = "zaun-init";
/// Hostname inside every sandbox (`--hostname`).
pub const SANDBOX_HOSTNAME: &str = "zaun";
/// Version of this build (the workspace version).
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Directory holding `zaun-init`, compiled in at build time from `ZAUN_LIBEXEC_DIR`
/// (RELEASE_STRATEGY §2.1). Default `/usr/libexec/zaun`.
pub const LIBEXEC_DIR: &str = match option_env!("ZAUN_LIBEXEC_DIR") {
    Some(dir) => dir,
    None => "/usr/libexec/zaun",
};

// ---- Stored-format versions -------------------------------------------------------------

/// Version of the SQLite schema written by this build.
pub const DB_SCHEMA_VERSION: u32 = 2;
/// `PRAGMA application_id` of `library.db` ("EPLY").
pub const DB_APPLICATION_ID: u32 = 0x4550_4C59;
/// Version of the `SandboxPolicy` JSON document.
pub const POLICY_SCHEMA_VERSION: u32 = 1;
/// Version of the `LaunchConfig` JSON document.
pub const LAUNCH_SCHEMA_VERSION: u32 = 1;
/// Version of `config.toml`.
pub const CONFIG_SCHEMA_VERSION: u32 = 1;
/// Version of the `plan.json` launch plan file.
pub const PLAN_FORMAT_VERSION: u32 = 1;

// ---- Session lifecycle ------------------------------------------------------------------

/// Interval between supervisor heartbeats.
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
/// Default grace period between a graceful stop and a forced one.
pub const STOP_GRACE_DEFAULT: Duration = Duration::from_secs(10);
/// Minimum stop grace period a game may configure, in seconds.
pub const STOP_GRACE_MIN_SECS: u8 = 3;
/// Maximum stop grace period a game may configure, in seconds.
pub const STOP_GRACE_MAX_SECS: u8 = 60;
/// Sessions shorter than this are not counted in statistics.
pub const MIN_COUNTED_SESSION: Duration = Duration::from_secs(5);
/// Time after the first launch phase after which a session is considered `Running`.
pub const STARTING_TIMEOUT: Duration = Duration::from_secs(120);

// ---- Identity ---------------------------------------------------------------------------

/// Bytes hashed at the start and end of a file for the quick hash.
pub const QUICK_HASH_WINDOW: u64 = 1024 * 1024;
/// Smallest file that can be a PE executable.
pub const MIN_PE_SIZE: u64 = 97;

// ---- Per-game limits (DATA_MODEL §2–4, CONFIGURATION §3) --------------------------------

/// Maximum length of a game name, in characters.
pub const GAME_NAME_MAX_CHARS: usize = 80;
/// Maximum length of a game's notes, in characters.
pub const GAME_NOTES_MAX_CHARS: usize = 4000;
/// Maximum number of executables per game.
pub const MAX_EXECUTABLES_PER_GAME: usize = 32;
/// Maximum number of launch arguments.
pub const MAX_LAUNCH_ARGS: usize = 128;
/// Maximum length of one launch argument, in bytes.
pub const MAX_LAUNCH_ARG_BYTES: usize = 4096;
/// Maximum number of per-game environment variables.
pub const MAX_LAUNCH_ENV: usize = 64;
/// Maximum length of an environment variable name, in bytes.
pub const MAX_ENV_NAME_BYTES: usize = 128;
/// Maximum length of an environment variable value, in bytes.
pub const MAX_ENV_VALUE_BYTES: usize = 4096;
/// Maximum length of `WINEDLLOVERRIDES`, in bytes.
pub const MAX_DLL_OVERRIDES_BYTES: usize = 1024;
/// Maximum length of `DXVK_HUD`, in bytes.
pub const MAX_DXVK_HUD_BYTES: usize = 128;
/// Maximum length of `WINEDEBUG`, in bytes.
pub const MAX_WINE_DEBUG_BYTES: usize = 256;
/// Maximum length of an umu `GAMEID`.
pub const MAX_UMU_ID_BYTES: usize = 64;
/// Maximum length of a tag name, in characters.
pub const TAG_NAME_MAX_CHARS: usize = 32;
/// Number of tag colours (`0..TAG_COLORS`).
pub const TAG_COLORS: u8 = 8;
/// Maximum length of a runtime name.
pub const RUNTIME_NAME_MAX_BYTES: usize = 96;
/// Maximum length of a policy exception reason, in characters.
pub const MAX_EXCEPTION_REASON_CHARS: usize = 200;

// ---- Logging (LOGGING §2–3) -------------------------------------------------------------

/// Size at which the application log rotates.
pub const APP_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// Number of application log files kept (`zaun.log` plus `.1` … `.4`).
pub const APP_LOG_FILES: usize = 5;
/// Maximum length of one log line in bytes; longer lines are cut.
pub const LOG_LINE_MAX_BYTES: usize = 4096;
/// Bounded queue of the non-blocking application-log writer.
pub const APP_LOG_QUEUE: usize = 8192;
/// Lines kept in memory for crash records.
pub const CRASH_RING_LINES: usize = 50;
/// Default session log cap.
pub const SESSION_LOG_CAP_DEFAULT: u64 = 32 * 1024 * 1024;
/// Identical consecutive lines written before the rest are collapsed.
pub const SESSION_LOG_REPEAT_LIMIT: u32 = 20;
/// Environment variable selecting the log level for one run.
pub const ENV_LOG: &str = "ZAUN_LOG";
/// Environment variable selecting JSON log output.
pub const ENV_LOG_FORMAT: &str = "ZAUN_LOG_FORMAT";

// ---- Environment variables read by Zaun (CONFIGURATION §5) --------------------------

/// Override of the configuration root.
pub const ENV_CONFIG_DIR: &str = "ZAUN_CONFIG_DIR";
/// Override of the data root.
pub const ENV_DATA_DIR: &str = "ZAUN_DATA_DIR";
/// Override of the cache root.
pub const ENV_CACHE_DIR: &str = "ZAUN_CACHE_DIR";
/// Override of the state root.
pub const ENV_STATE_DIR: &str = "ZAUN_STATE_DIR";
/// Path to `zaun-init` (development).
pub const ENV_INIT: &str = "ZAUN_INIT";
/// Override of `umu-run` detection.
pub const ENV_UMU_RUN: &str = "ZAUN_UMU_RUN";
/// Override of `bwrap` detection.
pub const ENV_BWRAP: &str = "ZAUN_BWRAP";
/// Renderer for this run (`wgpu` or `glow`).
pub const ENV_RENDERER: &str = "ZAUN_RENDERER";
/// Start a second GUI (development only).
pub const ENV_NO_SINGLE_INSTANCE: &str = "ZAUN_NO_SINGLE_INSTANCE";
/// Enables end-to-end tests.
pub const ENV_E2E: &str = "ZAUN_E2E";

// ---- Jobs (ARCHITECTURE §5) -------------------------------------------------------------

/// Minimum number of job-pool workers.
pub const JOB_WORKERS_MIN: usize = 2;
/// Maximum number of job-pool workers.
pub const JOB_WORKERS_MAX: usize = 6;
/// Default bound of queued jobs per label (replace-oldest beyond it).
pub const JOB_LABEL_QUEUE_DEFAULT: usize = 256;

// ---- Chat GIF search (ADR-0033, PRIVACY §3) ---------------------------------------------

/// `chat.gif_key`'s default: the public Giphy key Signal ships in its apps.
pub const GIF_DEFAULT_KEY: &str = "ApVVlSyeBfNKK6UWtnBRq9CvAkWsxayB";
/// `chat.gif_proxy`'s default: Signal's content proxy (HTTP CONNECT; it sees only host
/// names).
pub const GIF_DEFAULT_PROXY: &str = "http://contentproxy.signal.org:443";

// ---- Online artwork host allow-list (ADR-0018 §4–5, PRIVACY §3) -------------------------

/// Steam CDN host artwork is requested from.
pub const STEAM_CDN_HOST: &str = "cdn.cloudflare.steamstatic.com";
/// Steam store host (app search, `/api/storesearch`).
pub const STEAM_STORE_HOST: &str = "store.steampowered.com";
/// Hosts `artwork::http` may send a first request to.
pub const ARTWORK_REQUEST_HOSTS: [&str; 2] = [STEAM_CDN_HOST, STEAM_STORE_HOST];
/// Hosts reachable only as redirect targets of an allowed request.
pub const ARTWORK_REDIRECT_HOSTS: [&str; 2] = [
    "shared.steamstatic.com",
    "shared.cloudflare.steamstatic.com",
];

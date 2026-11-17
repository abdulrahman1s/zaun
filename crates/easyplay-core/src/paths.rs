//! Resolve every on-disk location from XDG variables once at start (PERSISTENCE §2).
//!
//! Layer 1. Pure: reads only the [`PathEnv`] it is given, never touches the filesystem and
//! never creates directories (that is `fsx`). Relative XDG values are ignored as the XDG
//! specification requires; `EASYPLAY_*_DIR` overrides must be absolute. Without
//! `XDG_RUNTIME_DIR` the runtime root falls back to `/tmp/easyplay-<uid>`;
//! [`AppPaths::runtime_dir_is_fallback`] tells the caller to verify it with
//! `fsx::ensure_private_dir` before use.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use crate::consts::{DIR_NAME, ENV_CACHE_DIR, ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_STATE_DIR};
use crate::ids::{GameId, SessionId};
use crate::model::SlrVariant;

/// Why paths could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathsError {
    /// Neither `HOME` nor all XDG roots are usable.
    #[error("HOME is not set to an absolute path and {0} has no absolute XDG value")]
    NoHome(&'static str),
    /// An `EASYPLAY_*_DIR` override is not absolute.
    #[error("{0} must be an absolute path")]
    RelativeOverride(&'static str),
}

/// The environment inputs of path resolution: variables and the user ID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathEnv {
    vars: BTreeMap<OsString, OsString>,
    uid: u32,
}

impl PathEnv {
    /// An empty environment for user `uid` (tests).
    pub fn new(uid: u32) -> Self {
        Self {
            vars: BTreeMap::new(),
            uid,
        }
    }

    /// The real process environment and user ID.
    pub fn from_process() -> Self {
        Self {
            vars: std::env::vars_os().collect(),
            uid: rustix::process::getuid().as_raw(),
        }
    }

    /// Builder: set a variable.
    #[must_use]
    pub fn with(mut self, key: impl Into<OsString>, value: impl Into<OsString>) -> Self {
        self.vars.insert(key.into(), value.into());
        self
    }

    /// Set a variable.
    pub fn set(&mut self, key: impl Into<OsString>, value: impl Into<OsString>) {
        self.vars.insert(key.into(), value.into());
    }

    /// A variable's value.
    pub fn get(&self, key: &str) -> Option<&OsStr> {
        self.vars.get(OsStr::new(key)).map(OsString::as_os_str)
    }

    /// The user ID.
    pub fn uid(&self) -> u32 {
        self.uid
    }

    /// A variable as an absolute, normalised path; empty and relative values are ignored.
    fn abs(&self, key: &str) -> Option<PathBuf> {
        let v = self.get(key)?;
        if v.is_empty() {
            return None;
        }
        let p = Path::new(v);
        p.is_absolute().then(|| normalize(p))
    }

    fn override_dir(&self, key: &'static str) -> Result<Option<PathBuf>, PathsError> {
        match self.get(key) {
            None => Ok(None),
            Some(v) if v.is_empty() => Ok(None),
            Some(v) => {
                let p = Path::new(v);
                if p.is_absolute() {
                    Ok(Some(normalize(p)))
                } else {
                    Err(PathsError::RelativeOverride(key))
                }
            }
        }
    }
}

/// Lexically normalise an absolute path: remove `.` components, resolve `..` against the
/// preceding component (never above `/`), drop repeated and trailing separators. Symlinks are
/// not consulted.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Prefix(_) | Component::CurDir => {}
            Component::RootDir => out.push("/"),
            Component::ParentDir => {
                if out.parent().is_some() {
                    out.pop();
                } else if !path.is_absolute() {
                    out.push("..");
                }
            }
            Component::Normal(n) => out.push(n),
        }
    }
    out
}

/// Every location EasyPlay uses. All paths are absolute and lexically normalised.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// The user's home directory (used for redaction and well-known folders), when known.
    pub home: Option<PathBuf>,
    /// `<config>` = `$XDG_CONFIG_HOME/easyplay`.
    pub config_dir: PathBuf,
    /// `<data>` = `$XDG_DATA_HOME/easyplay`.
    pub data_dir: PathBuf,
    /// `<cache>` = `$XDG_CACHE_HOME/easyplay`.
    pub cache_dir: PathBuf,
    /// `<state>` = `$XDG_STATE_HOME/easyplay`.
    pub state_dir: PathBuf,
    /// `<run>` = `$XDG_RUNTIME_DIR/easyplay`, or `/tmp/easyplay-<uid>`.
    pub runtime_dir: PathBuf,
    /// Whether `runtime_dir` is the `/tmp` fallback that must be verified before use.
    pub runtime_dir_is_fallback: bool,
    /// `$XDG_RUNTIME_DIR` itself (sockets of the user session), when set.
    pub xdg_runtime_dir: Option<PathBuf>,

    /// `<config>/config.toml`.
    pub config_file: PathBuf,
    /// `<data>/library.db`.
    pub db_file: PathBuf,
    /// `<data>/library.lock`.
    pub library_lock: PathBuf,
    /// `<data>/backups`.
    pub backups_dir: PathBuf,
    /// `<data>/games`.
    pub games_dir: PathBuf,
    /// `<data>/artwork`.
    pub artwork_dir: PathBuf,
    /// `<data>/friends`: the chat engine's encrypted database and its key (ADR-0033).
    /// Made (0700) only when Friends is turned on.
    pub friends_dir: PathBuf,
    /// `<data>/runtimes` (the runtime store).
    pub runtimes_dir: PathBuf,
    /// `<data>/runtimes/umu`.
    pub umu_store_dir: PathBuf,
    /// `<data>/runtimes/Steam/compatibilitytools.d`.
    pub compat_tools_dir: PathBuf,
    /// `<data>/runtimes/session-var`.
    pub session_var_root: PathBuf,
    /// `<data>/runtimes/quarantine`.
    pub quarantine_dir: PathBuf,
    /// `<data>/runtime-manifests`.
    pub runtime_manifests_dir: PathBuf,
    /// `<data>/saved-data`.
    pub saved_data_dir: PathBuf,
    /// `<data>/exports`.
    pub exports_dir: PathBuf,

    /// `<cache>/thumbnails`.
    pub thumbs_dir: PathBuf,
    /// `<cache>/umu`.
    pub umu_cache_dir: PathBuf,
    /// `<cache>/host-facts.json`.
    pub host_facts_file: PathBuf,
    /// `<cache>/CACHEDIR.TAG`.
    pub cachedir_tag: PathBuf,

    /// `<state>/logs`.
    pub logs_dir: PathBuf,
    /// `<state>/logs/easyplay.log`.
    pub app_log_file: PathBuf,
    /// `<state>/logs/sessions`.
    pub sessions_log_dir: PathBuf,
    /// `<state>/window.json`.
    pub window_state_file: PathBuf,
    /// `<state>/crash`.
    pub crash_dir: PathBuf,

    /// `<run>/instance.sock`.
    pub instance_socket: PathBuf,
    /// `<run>/gui.pid`.
    pub gui_pid_file: PathBuf,
    /// `<run>/sessions`.
    pub run_sessions_dir: PathBuf,
}

impl AppPaths {
    /// Resolve all locations from `env` (ARCH-005).
    pub fn resolve(env: &PathEnv) -> Result<AppPaths, PathsError> {
        let home = env.abs("HOME");
        let root = |override_key: &'static str,
                    xdg_key: &'static str,
                    default_rel: &str|
         -> Result<PathBuf, PathsError> {
            if let Some(dir) = env.override_dir(override_key)? {
                return Ok(dir);
            }
            if let Some(xdg) = env.abs(xdg_key) {
                return Ok(xdg.join(DIR_NAME));
            }
            match &home {
                Some(h) => Ok(h.join(default_rel).join(DIR_NAME)),
                None => Err(PathsError::NoHome(xdg_key)),
            }
        };
        let config_dir = root(ENV_CONFIG_DIR, "XDG_CONFIG_HOME", ".config")?;
        let data_dir = root(ENV_DATA_DIR, "XDG_DATA_HOME", ".local/share")?;
        let cache_dir = root(ENV_CACHE_DIR, "XDG_CACHE_HOME", ".cache")?;
        let state_dir = root(ENV_STATE_DIR, "XDG_STATE_HOME", ".local/state")?;
        let xdg_runtime_dir = env.abs("XDG_RUNTIME_DIR");
        let (runtime_dir, runtime_dir_is_fallback) = match &xdg_runtime_dir {
            Some(dir) => (dir.join(DIR_NAME), false),
            None => (
                PathBuf::from(format!("/tmp/{DIR_NAME}-{}", env.uid())),
                true,
            ),
        };

        let runtimes_dir = data_dir.join("runtimes");
        let logs_dir = state_dir.join("logs");
        Ok(AppPaths {
            config_file: config_dir.join("config.toml"),
            db_file: data_dir.join("library.db"),
            library_lock: data_dir.join("library.lock"),
            backups_dir: data_dir.join("backups"),
            games_dir: data_dir.join("games"),
            artwork_dir: data_dir.join("artwork"),
            friends_dir: data_dir.join("friends"),
            umu_store_dir: runtimes_dir.join("umu"),
            compat_tools_dir: runtimes_dir.join("Steam").join("compatibilitytools.d"),
            session_var_root: runtimes_dir.join("session-var"),
            quarantine_dir: runtimes_dir.join("quarantine"),
            runtimes_dir,
            runtime_manifests_dir: data_dir.join("runtime-manifests"),
            saved_data_dir: data_dir.join("saved-data"),
            exports_dir: data_dir.join("exports"),
            thumbs_dir: cache_dir.join("thumbnails"),
            umu_cache_dir: cache_dir.join("umu"),
            host_facts_file: cache_dir.join("host-facts.json"),
            cachedir_tag: cache_dir.join("CACHEDIR.TAG"),
            app_log_file: logs_dir.join("easyplay.log"),
            sessions_log_dir: logs_dir.join("sessions"),
            logs_dir,
            window_state_file: state_dir.join("window.json"),
            crash_dir: state_dir.join("crash"),
            instance_socket: runtime_dir.join("instance.sock"),
            gui_pid_file: runtime_dir.join("gui.pid"),
            run_sessions_dir: runtime_dir.join("sessions"),
            home,
            config_dir,
            data_dir,
            cache_dir,
            state_dir,
            runtime_dir,
            runtime_dir_is_fallback,
            xdg_runtime_dir,
        })
    }

    /// Whether every root is the default location (no `EASYPLAY_*_DIR` override in effect);
    /// computed by comparing with a resolution that ignores overrides.
    pub fn uses_default_roots(&self, env: &PathEnv) -> bool {
        let mut plain = env.clone();
        for key in [ENV_CONFIG_DIR, ENV_DATA_DIR, ENV_CACHE_DIR, ENV_STATE_DIR] {
            plain.vars.remove(OsStr::new(key));
        }
        AppPaths::resolve(&plain).is_ok_and(|p| {
            p.config_dir == self.config_dir
                && p.data_dir == self.data_dir
                && p.cache_dir == self.cache_dir
                && p.state_dir == self.state_dir
        })
    }

    /// Every directory EasyPlay creates at start (mode 0700), parents first.
    pub fn startup_dirs(&self) -> Vec<&Path> {
        vec![
            &self.config_dir,
            &self.data_dir,
            &self.backups_dir,
            &self.games_dir,
            &self.artwork_dir,
            &self.runtimes_dir,
            &self.runtime_manifests_dir,
            &self.cache_dir,
            &self.thumbs_dir,
            &self.state_dir,
            &self.logs_dir,
            &self.sessions_log_dir,
            &self.crash_dir,
            &self.runtime_dir,
            &self.run_sessions_dir,
        ]
    }

    // ---- per game ---------------------------------------------------------------------

    /// `<data>/games/<id>`. Everything EasyPlay deletes for a game lies inside it.
    pub fn game_dir(&self, id: GameId) -> PathBuf {
        self.games_dir.join(id.to_string())
    }

    /// `<data>/games/<id>/prefix` (`WINEPREFIX`).
    pub fn prefix_dir(&self, id: GameId) -> PathBuf {
        self.game_dir(id).join("prefix")
    }

    /// `<data>/games/<id>/home` (the game's private `$HOME`).
    pub fn home_dir(&self, id: GameId) -> PathBuf {
        self.game_dir(id).join("home")
    }

    /// `<data>/games/<id>/overlay` (Protected game folders).
    pub fn overlay_dir(&self, id: GameId) -> PathBuf {
        self.game_dir(id).join("overlay")
    }

    /// `<data>/games/<id>/overlay/upper`.
    pub fn overlay_upper_dir(&self, id: GameId) -> PathBuf {
        self.overlay_dir(id).join("upper")
    }

    /// `<data>/games/<id>/overlay/work`.
    pub fn overlay_work_dir(&self, id: GameId) -> PathBuf {
        self.overlay_dir(id).join("work")
    }

    /// `<data>/games/<id>/session.lock` (one sandbox per prefix).
    pub fn prefix_lock(&self, id: GameId) -> PathBuf {
        self.game_dir(id).join("session.lock")
    }

    /// `<data>/artwork/<id>`.
    pub fn artwork_game_dir(&self, id: GameId) -> PathBuf {
        self.artwork_dir.join(id.to_string())
    }

    // ---- per session ------------------------------------------------------------------

    /// `<state>/logs/sessions/<sid>` (session logs).
    pub fn session_dir(&self, sid: SessionId) -> PathBuf {
        self.sessions_log_dir.join(sid.to_string())
    }

    /// `<data>/runtimes/session-var/<sid>` (pressure-vessel variable directory, Isolated mode).
    pub fn session_var_dir(&self, sid: SessionId) -> PathBuf {
        self.session_var_root.join(sid.to_string())
    }

    /// `<run>/sessions/<sid>`.
    pub fn run_session_dir(&self, sid: SessionId) -> PathBuf {
        self.run_sessions_dir.join(sid.to_string())
    }

    /// `<run>/sessions/<sid>/plan.json`.
    pub fn plan_file(&self, sid: SessionId) -> PathBuf {
        self.run_session_dir(sid).join("plan.json")
    }

    /// `<run>/sessions/<sid>/stop-request`.
    pub fn stop_request_file(&self, sid: SessionId) -> PathBuf {
        self.run_session_dir(sid).join("stop-request")
    }

    // ---- runtime store ----------------------------------------------------------------

    /// `<data>/runtimes/umu/<variant>`.
    pub fn slr_dir(&self, variant: SlrVariant) -> PathBuf {
        self.umu_store_dir.join(variant.as_str())
    }

    /// `<data>/runtime-manifests/<variant>.manifest.json`.
    pub fn runtime_manifest_file(&self, variant: SlrVariant) -> PathBuf {
        self.runtime_manifests_dir
            .join(format!("{}.manifest.json", variant.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home_only() -> PathEnv {
        PathEnv::new(1000).with("HOME", "/home/sam")
    }

    #[test]
    fn defaults_from_home() {
        let p = AppPaths::resolve(&home_only().with("XDG_RUNTIME_DIR", "/run/user/1000")).unwrap();
        assert_eq!(p.config_dir, Path::new("/home/sam/.config/easyplay"));
        assert_eq!(p.data_dir, Path::new("/home/sam/.local/share/easyplay"));
        assert_eq!(p.cache_dir, Path::new("/home/sam/.cache/easyplay"));
        assert_eq!(p.state_dir, Path::new("/home/sam/.local/state/easyplay"));
        assert_eq!(p.runtime_dir, Path::new("/run/user/1000/easyplay"));
        assert!(!p.runtime_dir_is_fallback);
        assert_eq!(
            p.db_file,
            Path::new("/home/sam/.local/share/easyplay/library.db")
        );
        assert_eq!(
            p.app_log_file,
            Path::new("/home/sam/.local/state/easyplay/logs/easyplay.log")
        );
        assert_eq!(
            p.instance_socket,
            Path::new("/run/user/1000/easyplay/instance.sock")
        );
        assert_eq!(p.home.as_deref(), Some(Path::new("/home/sam")));
    }

    #[test]
    #[allow(clippy::type_complexity)]
    fn environment_table() {
        // (variables, expected config dir, expected data dir)
        let cases: &[(&[(&str, &str)], &str, &str)] = &[
            (
                &[],
                "/home/sam/.config/easyplay",
                "/home/sam/.local/share/easyplay",
            ),
            (
                &[("XDG_CONFIG_HOME", "/cfg"), ("XDG_DATA_HOME", "/data/")],
                "/cfg/easyplay",
                "/data/easyplay",
            ),
            // Empty values are unset.
            (
                &[("XDG_CONFIG_HOME", "")],
                "/home/sam/.config/easyplay",
                "/home/sam/.local/share/easyplay",
            ),
            // Relative values are ignored.
            (
                &[("XDG_CONFIG_HOME", "cfg"), ("XDG_DATA_HOME", "./data")],
                "/home/sam/.config/easyplay",
                "/home/sam/.local/share/easyplay",
            ),
            // Overrides win and are used as is (no `easyplay` suffix).
            (
                &[
                    ("XDG_CONFIG_HOME", "/cfg"),
                    ("EASYPLAY_CONFIG_DIR", "/portable/config"),
                ],
                "/portable/config",
                "/home/sam/.local/share/easyplay",
            ),
            // Lexical normalisation.
            (
                &[("XDG_DATA_HOME", "/a/./b/../c//")],
                "/home/sam/.config/easyplay",
                "/a/c/easyplay",
            ),
        ];
        for (vars, config, data) in cases {
            let mut env = home_only();
            for (k, v) in *vars {
                env.set(*k, *v);
            }
            let p = AppPaths::resolve(&env).unwrap();
            assert_eq!(p.config_dir, Path::new(config), "{vars:?}");
            assert_eq!(p.data_dir, Path::new(data), "{vars:?}");
        }
    }

    #[test]
    fn relative_override_is_an_error() {
        let env = home_only().with("EASYPLAY_DATA_DIR", "data");
        assert_eq!(
            AppPaths::resolve(&env),
            Err(PathsError::RelativeOverride("EASYPLAY_DATA_DIR"))
        );
    }

    #[test]
    fn missing_home() {
        assert!(matches!(
            AppPaths::resolve(&PathEnv::new(1000)),
            Err(PathsError::NoHome(_))
        ));
        assert!(AppPaths::resolve(&PathEnv::new(1000).with("HOME", "relative")).is_err());
        let all = PathEnv::new(1000)
            .with("XDG_CONFIG_HOME", "/c")
            .with("XDG_DATA_HOME", "/d")
            .with("XDG_CACHE_HOME", "/k")
            .with("XDG_STATE_HOME", "/s");
        let p = AppPaths::resolve(&all).unwrap();
        assert_eq!(p.home, None);
        assert_eq!(p.state_dir, Path::new("/s/easyplay"));
    }

    #[test]
    fn runtime_fallback() {
        let p = AppPaths::resolve(&home_only()).unwrap();
        assert!(p.runtime_dir_is_fallback);
        assert_eq!(p.runtime_dir, Path::new("/tmp/easyplay-1000"));
        let p = AppPaths::resolve(&home_only().with("XDG_RUNTIME_DIR", "run/rel")).unwrap();
        assert!(p.runtime_dir_is_fallback);
    }

    #[test]
    fn per_game_and_session_helpers() {
        let p = AppPaths::resolve(&home_only().with("XDG_RUNTIME_DIR", "/run/user/1000")).unwrap();
        let g: GameId = "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap();
        let s: SessionId = "01930a12-0000-7000-8000-000000000001".parse().unwrap();
        let data = "/home/sam/.local/share/easyplay";
        assert_eq!(p.game_dir(g), Path::new(&format!("{data}/games/{g}")));
        assert_eq!(p.prefix_dir(g), p.game_dir(g).join("prefix"));
        assert_eq!(p.home_dir(g), p.game_dir(g).join("home"));
        assert_eq!(p.overlay_upper_dir(g), p.game_dir(g).join("overlay/upper"));
        assert_eq!(p.prefix_lock(g), p.game_dir(g).join("session.lock"));
        assert_eq!(
            p.artwork_game_dir(g),
            Path::new(&format!("{data}/artwork/{g}"))
        );
        assert_eq!(
            p.session_dir(s),
            Path::new(&format!(
                "/home/sam/.local/state/easyplay/logs/sessions/{s}"
            ))
        );
        assert_eq!(
            p.session_var_dir(s),
            Path::new(&format!("{data}/runtimes/session-var/{s}"))
        );
        assert_eq!(
            p.plan_file(s),
            Path::new(&format!("/run/user/1000/easyplay/sessions/{s}/plan.json"))
        );
        assert_eq!(p.stop_request_file(s).file_name().unwrap(), "stop-request");
        assert_eq!(
            p.slr_dir(SlrVariant::Steamrt3),
            Path::new(&format!("{data}/runtimes/umu/steamrt3"))
        );
        assert_eq!(
            p.runtime_manifest_file(SlrVariant::Steamrt4),
            Path::new(&format!("{data}/runtime-manifests/steamrt4.manifest.json"))
        );
        for d in p.startup_dirs() {
            assert!(d.is_absolute());
        }
    }

    #[test]
    fn default_roots_detection() {
        let env = home_only();
        let p = AppPaths::resolve(&env).unwrap();
        assert!(p.uses_default_roots(&env));
        let env = home_only().with("EASYPLAY_DATA_DIR", "/tmp/portable");
        let p = AppPaths::resolve(&env).unwrap();
        assert!(!p.uses_default_roots(&env));
    }

    #[test]
    fn normalize_cases() {
        assert_eq!(normalize(Path::new("/a/b/../../..")), Path::new("/"));
        assert_eq!(normalize(Path::new("/a/./b/")), Path::new("/a/b"));
        assert_eq!(normalize(Path::new("//a//b")), Path::new("/a/b"));
    }
}

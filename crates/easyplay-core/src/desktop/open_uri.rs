//! Opening folders, files and links outside EasyPlay (DESKTOP_INTEGRATION §8).
//!
//! Only EasyPlay-known locations can be opened: a [`KnownPath`] is constructed inside
//! `easyplay-core` from the library and [`crate::paths::AppPaths`] (never from text a game
//! produced), and links are the compiled-in HTTPS URLs of [`Link`]. Everything runs
//! `xdg-open` through the [`CommandRunner`] with a fixed argument vector and a 5 s timeout.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::host::exec::{find_in, path_dirs};
use crate::host::runner::PROBE_TIMEOUT;
use crate::host::{CommandRunner, CommandSpec};
use crate::paths::PathEnv;

/// A location EasyPlay knows: a folder of the library or a file it wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnownPath {
    path: PathBuf,
    kind: KnownKind,
}

/// What a [`KnownPath`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KnownKind {
    /// A folder: opened in the file manager.
    Folder,
    /// A text file EasyPlay wrote (a session log): opened with the default viewer.
    File,
}

impl KnownPath {
    /// Only core builds these (from library rows and `AppPaths`).
    pub(crate) fn folder(path: PathBuf) -> Self {
        Self {
            path,
            kind: KnownKind::Folder,
        }
    }

    /// A file EasyPlay wrote.
    pub(crate) fn file(path: PathBuf) -> Self {
        Self {
            path,
            kind: KnownKind::File,
        }
    }

    /// The path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Folder or file.
    pub fn kind(&self) -> KnownKind {
        self.kind
    }
}

/// Links compiled into the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Link {
    /// The project page (About).
    Homepage,
    /// Help and troubleshooting.
    Help,
}

impl Link {
    /// The URL (always `https`).
    pub const fn url(self) -> &'static str {
        match self {
            Link::Homepage => "https://github.com/easyplay/easyplay",
            Link::Help => "https://github.com/easyplay/easyplay#readme",
        }
    }
}

/// Environment `xdg-open` needs to reach the desktop's file manager or portal.
const PASS_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_CONFIG_DIRS",
    "XDG_DATA_HOME",
    "XDG_DATA_DIRS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "DESKTOP_SESSION",
    "KDE_SESSION_VERSION",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "XAUTHORITY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
    "XDG_ACTIVATION_TOKEN",
];

/// Opens known paths and links.
pub struct Opener {
    runner: Arc<dyn CommandRunner>,
    xdg_open: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
}

impl Opener {
    /// An opener for `env` (`xdg-open` looked up on its `PATH`).
    pub fn new(runner: Arc<dyn CommandRunner>, fs: &dyn Fs, env: &PathEnv) -> Self {
        Self {
            runner,
            xdg_open: find_in(fs, &path_dirs(env.get("PATH")), "xdg-open"),
            env: PASS_ENV
                .iter()
                .filter_map(|k| env.get(k).map(|v| (OsString::from(*k), v.to_owned())))
                .collect(),
        }
    }

    fn run(&self, arg: &std::ffi::OsStr) -> Result<(), Error> {
        let program = self.xdg_open.as_ref().ok_or_else(|| {
            Error::new(ErrorCode::IntTaskFailed).with("detail", "xdg-open is not installed")
        })?;
        let mut spec = CommandSpec::new(program, &[arg]).timeout(PROBE_TIMEOUT);
        spec.env = self.env.clone();
        let out = self
            .runner
            .run(&spec)
            .map_err(|e| Error::new(ErrorCode::IntTaskFailed).with("detail", e.to_string()))?;
        // A file manager that keeps xdg-open waiting past the timeout still opened.
        if out.success() || out.timed_out {
            Ok(())
        } else {
            Err(Error::new(ErrorCode::IntTaskFailed).with(
                "detail",
                format!(
                    "xdg-open failed ({}): {}",
                    out.status,
                    out.stderr_text().trim()
                ),
            ))
        }
    }

    /// Open a known folder in the file manager, or a known file with its default viewer.
    pub fn open(&self, target: &KnownPath) -> Result<(), Error> {
        self.run(target.path.as_os_str())
    }

    /// Show a known file in its folder (the folder is opened).
    pub fn reveal(&self, target: &KnownPath) -> Result<(), Error> {
        match (target.kind, target.path.parent()) {
            (KnownKind::File, Some(dir)) => self.run(dir.as_os_str()),
            _ => self.open(target),
        }
    }

    /// Open a compiled-in link.
    pub fn open_link(&self, link: Link) -> Result<(), Error> {
        debug_assert!(link.url().starts_with("https://"));
        self.run(link.url().as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::{CommandOutput, RunError};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Rec(Mutex<Vec<Vec<OsString>>>);
    impl CommandRunner for Rec {
        fn run(&self, cmd: &CommandSpec) -> Result<CommandOutput, RunError> {
            self.0.lock().unwrap().push(cmd.argv());
            Ok(CommandOutput::default())
        }
    }

    #[test]
    fn fixed_argv_only() {
        let rec = Arc::new(Rec::default());
        let o = Opener {
            runner: rec.clone(),
            xdg_open: Some("/usr/bin/xdg-open".into()),
            env: vec![],
        };
        o.open(&KnownPath::folder("/games/x".into())).unwrap();
        o.reveal(&KnownPath::file("/state/logs/s.log".into()))
            .unwrap();
        o.open_link(Link::Help).unwrap();
        let calls = rec.0.lock().unwrap().clone();
        assert_eq!(
            calls,
            vec![
                vec![OsString::from("/usr/bin/xdg-open"), "/games/x".into()],
                vec!["/usr/bin/xdg-open".into(), "/state/logs".into()],
                vec!["/usr/bin/xdg-open".into(), Link::Help.url().into()],
            ]
        );
        let none = Opener {
            runner: rec,
            xdg_open: None,
            env: vec![],
        };
        assert!(none.open(&KnownPath::folder("/x".into())).is_err());
    }
}

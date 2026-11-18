//! The `.exe` handler (DESKTOP_INTEGRATION §3–4).
//!
//! Zaun only *offers* itself through `MimeType=` in its desktop entry. Becoming the default
//! happens only when the user presses the button in onboarding or Settings; the previous
//! defaults are recorded (by the service, in `kv`) so they can be restored. Everything goes
//! through `xdg-mime` run by the [`CommandRunner`] with a fixed argument vector and a 5 s
//! timeout; only the "there was no previous default" case edits `mimeapps.list` directly,
//! removing Zaun's own lines and preserving every other byte.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode};
use crate::fsx::Fs;
use crate::host::exec::{find_in, path_dirs};
use crate::host::runner::PROBE_TIMEOUT;
use crate::host::{CommandRunner, CommandSpec};
use crate::paths::PathEnv;

/// Zaun's desktop file ID.
pub const DESKTOP_ID: &str = "io.github.zaun.Zaun.desktop";
/// The types listed in the desktop entry's `MimeType=`.
pub const ENTRY_TYPES: &[&str] = &[
    "application/vnd.microsoft.portable-executable",
    "application/x-ms-dos-executable",
    "application/x-msdos-program",
    "application/x-msdownload",
    "application/x-dosexec",
    "application/x-msi",
];
/// The `kv` key holding [`PreviousHandlers`] as JSON.
pub const KV_PREVIOUS: &str = "integration.previous_handlers";
/// The manual instruction shown when `xdg-mime` is missing or fails.
pub const MANUAL_STEPS: &str =
    "In your file manager: right-click an .exe → Open with → Zaun → Always use";

/// A MIME type name (`type/subtype`, restricted characters).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MimeType(String);

impl MimeType {
    /// Validate `s`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        let (a, b) = s.split_once('/')?;
        let ok = |p: &str| {
            !p.is_empty()
                && p.len() <= 127
                && p.bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"+-.".contains(&c))
        };
        (ok(a) && ok(b)).then(|| Self(s.to_owned()))
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One type's current default.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandlerStatus {
    /// The type.
    pub mime: MimeType,
    /// Its default desktop ID (`None`: no default).
    pub default: Option<String>,
    /// The default's display name, read from its desktop file.
    pub name: Option<String>,
}

impl HandlerStatus {
    /// Whether Zaun is the default.
    pub fn is_ours(&self) -> bool {
        self.default.as_deref() == Some(DESKTOP_ID)
    }
}

/// The defaults before Zaun took over (stored in `kv`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousHandlers {
    /// Type → previous desktop ID (`None`: there was none).
    pub entries: Vec<(MimeType, Option<String>)>,
}

/// Status, set and restore (ARCHITECTURE §7 `MimeRegistrar`).
pub trait MimeRegistrar: Send + Sync {
    /// The types Zaun manages on this machine.
    fn managed_types(&self) -> Vec<MimeType>;
    /// Per managed type: the current default and its name.
    fn status(&self) -> Result<Vec<HandlerStatus>, Error>;
    /// Make Zaun the default for `types`; returns what was default before.
    fn set_default(&self, types: &[MimeType]) -> Result<PreviousHandlers, Error>;
    /// Put the recorded defaults back.
    fn restore(&self, previous: &PreviousHandlers) -> Result<(), Error>;
}

/// Summary of a status list for the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Overall {
    /// Zaun is the default for the main type.
    Zaun,
    /// Another application (its name, or its desktop ID).
    Other(String),
    /// Nothing handles `.exe`.
    Nothing,
}

/// The state of the first entry type that has a status (the PE type is what file managers
/// use for `.exe`).
pub fn overall(status: &[HandlerStatus]) -> Overall {
    let main = status
        .iter()
        .find(|s| s.default.is_some())
        .or_else(|| status.first());
    match main {
        Some(s) if s.is_ours() => Overall::Zaun,
        Some(HandlerStatus {
            default: Some(d),
            name,
            ..
        }) => Overall::Other(name.clone().unwrap_or_else(|| d.clone())),
        _ => Overall::Nothing,
    }
}

/// Environment variables `xdg-mime` (a shell script) needs.
const PASS_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "XDG_CONFIG_HOME",
    "XDG_CONFIG_DIRS",
    "XDG_DATA_HOME",
    "XDG_DATA_DIRS",
    "XDG_CURRENT_DESKTOP",
    "XDG_SESSION_DESKTOP",
    "DESKTOP_SESSION",
    "KDE_SESSION_VERSION",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
];

/// XDG base directories relevant to desktop files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct XdgDirs {
    /// `$XDG_CONFIG_HOME` (default `~/.config`).
    pub config_home: PathBuf,
    /// `$XDG_DATA_HOME` (default `~/.local/share`).
    pub data_home: PathBuf,
    /// `$XDG_DATA_DIRS` (default `/usr/local/share:/usr/share`).
    pub data_dirs: Vec<PathBuf>,
}

impl XdgDirs {
    /// Resolve from the environment; `None` without a usable home.
    pub fn from_env(env: &PathEnv) -> Option<Self> {
        let home = env
            .get("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())?;
        let abs = |k: &str| env.get(k).map(PathBuf::from).filter(|p| p.is_absolute());
        let data_dirs = env
            .get("XDG_DATA_DIRS")
            .map(|v| path_dirs(Some(v)))
            .filter(|d| !d.is_empty())
            .unwrap_or_else(|| vec!["/usr/local/share".into(), "/usr/share".into()]);
        Some(Self {
            config_home: abs("XDG_CONFIG_HOME").unwrap_or_else(|| home.join(".config")),
            data_home: abs("XDG_DATA_HOME").unwrap_or_else(|| home.join(".local/share")),
            data_dirs,
        })
    }

    /// `$XDG_CONFIG_HOME/mimeapps.list`.
    pub fn mimeapps_list(&self) -> PathBuf {
        self.config_home.join("mimeapps.list")
    }

    /// Find a desktop file by ID in the data directories (user first).
    pub fn find_desktop_file(&self, fs: &dyn Fs, id: &str) -> Option<PathBuf> {
        if id.contains('/') || id.starts_with('.') {
            return None;
        }
        std::iter::once(&self.data_home)
            .chain(self.data_dirs.iter())
            .map(|d| d.join("applications").join(id))
            .find(|p| fs.metadata_follow(p).is_ok_and(|m| m.is_file()))
    }
}

/// The real registrar: `xdg-mime` through the runner.
pub struct XdgMimeRegistrar {
    runner: Arc<dyn CommandRunner>,
    fs: Arc<dyn Fs>,
    xdg_mime: Option<PathBuf>,
    env: Vec<(OsString, OsString)>,
    dirs: XdgDirs,
    scratch: Option<PathBuf>,
    detected: std::sync::OnceLock<Vec<MimeType>>,
}

fn failed(detail: impl Into<String>) -> Error {
    Error::new(ErrorCode::DskDefaultHandlerFailed)
        .with("detail", detail.into())
        .with("manual", MANUAL_STEPS)
}

impl XdgMimeRegistrar {
    /// A registrar for the environment `env`; `xdg-mime` is looked up on its `PATH`. With
    /// `scratch`, the machine's type names are detected once, on first use, from sample
    /// files written there (§3 step 1).
    pub fn new(
        runner: Arc<dyn CommandRunner>,
        fs: Arc<dyn Fs>,
        env: &PathEnv,
        dirs: XdgDirs,
        scratch: Option<PathBuf>,
    ) -> Self {
        let xdg_mime = find_in(&*fs, &path_dirs(env.get("PATH")), "xdg-mime");
        let env = PASS_ENV
            .iter()
            .filter_map(|k| env.get(k).map(|v| (OsString::from(*k), v.to_owned())))
            .chain(std::iter::once(("LANG".into(), "C.UTF-8".into())))
            .collect();
        Self {
            runner,
            fs,
            xdg_mime,
            env,
            dirs,
            scratch,
            detected: std::sync::OnceLock::new(),
        }
    }

    /// Whether `xdg-mime` was found (otherwise the feature is disabled with [`MANUAL_STEPS`]).
    pub fn available(&self) -> bool {
        self.xdg_mime.is_some()
    }

    /// The directories used.
    pub fn dirs(&self) -> &XdgDirs {
        &self.dirs
    }

    fn run(&self, args: &[&str]) -> Result<String, Error> {
        let program = self
            .xdg_mime
            .as_ref()
            .ok_or_else(|| failed("xdg-mime is not installed"))?;
        let mut spec = CommandSpec::new(program, args).timeout(PROBE_TIMEOUT);
        spec.env = self.env.clone();
        let out = self.runner.run(&spec).map_err(|e| failed(e.to_string()))?;
        if !out.success() {
            return Err(failed(format!(
                "xdg-mime {} failed ({}): {}",
                args.first().copied().unwrap_or_default(),
                out.status,
                out.stderr_text().trim()
            )));
        }
        Ok(out.stdout_text())
    }

    /// Determine the type names this machine's MIME database gives a PE and an MSI file
    /// (§3 step 1): sample files are written to `scratch` and queried.
    fn detect(&self, scratch: &Path) -> Vec<MimeType> {
        let mut found = Vec::new();
        if self.fs.create_dir_all(scratch, 0o700).is_ok() {
            for (name, bytes) in [("sample.exe", pe_sample()), ("sample.msi", msi_sample())] {
                let p = scratch.join(name);
                if self.fs.atomic_write(&p, &bytes, 0o600).is_err() {
                    continue;
                }
                let s = p.to_string_lossy().into_owned();
                if let Ok(t) = self.run(&["query", "filetype", &s])
                    && let Some(m) = MimeType::parse(t.lines().next().unwrap_or_default())
                    && !found.contains(&m)
                    && m.as_str() != "application/octet-stream"
                {
                    found.push(m);
                }
                let _ = self.fs.remove_file(&p);
            }
            let _ = self.fs.remove_dir(scratch);
        }
        found
    }

    fn display_name(&self, id: &str) -> Option<String> {
        let p = self.dirs.find_desktop_file(&*self.fs, id)?;
        let bytes = self.fs.read_follow(&p, 256 * 1024).ok()?;
        super::desktop_entry::read_key(&String::from_utf8_lossy(&bytes), "Name")
    }
}

impl MimeRegistrar for XdgMimeRegistrar {
    fn managed_types(&self) -> Vec<MimeType> {
        let mut out = match (&self.scratch, self.available()) {
            (Some(dir), true) => self.detected.get_or_init(|| self.detect(dir)).clone(),
            _ => Vec::new(),
        };
        for t in ENTRY_TYPES {
            if let Some(m) = MimeType::parse(t)
                && !out.contains(&m)
            {
                out.push(m);
            }
        }
        out
    }

    fn status(&self) -> Result<Vec<HandlerStatus>, Error> {
        self.managed_types()
            .into_iter()
            .map(|mime| {
                let out = self.run(&["query", "default", mime.as_str()])?;
                let default = out
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|d| !d.is_empty() && d.ends_with(".desktop"))
                    .map(str::to_owned);
                let name = default.as_deref().and_then(|d| self.display_name(d));
                Ok(HandlerStatus {
                    mime,
                    default,
                    name,
                })
            })
            .collect()
    }

    fn set_default(&self, types: &[MimeType]) -> Result<PreviousHandlers, Error> {
        let before = self.status()?;
        let previous = PreviousHandlers {
            entries: types
                .iter()
                .map(|t| {
                    let d = before
                        .iter()
                        .find(|s| s.mime == *t)
                        .and_then(|s| s.default.clone())
                        .filter(|d| d != DESKTOP_ID);
                    (t.clone(), d)
                })
                .collect(),
        };
        let mut args = vec!["default", DESKTOP_ID];
        args.extend(types.iter().map(MimeType::as_str));
        self.run(&args)?;
        let after = self.status()?;
        let missing: Vec<&str> = after
            .iter()
            .filter(|s| types.contains(&s.mime) && !s.is_ours())
            .map(|s| s.mime.as_str())
            .collect();
        if !missing.is_empty() {
            return Err(failed(format!(
                "still not the default for {}",
                missing.join(", ")
            )));
        }
        Ok(previous)
    }

    fn restore(&self, previous: &PreviousHandlers) -> Result<(), Error> {
        let mut none = Vec::new();
        for (t, id) in &previous.entries {
            match id {
                Some(id) if id != DESKTOP_ID && is_desktop_id(id) => {
                    self.run(&["default", id, t.as_str()])?;
                }
                _ => none.push(t.clone()),
            }
        }
        if none.is_empty() {
            return Ok(());
        }
        let path = self.dirs.mimeapps_list();
        let text = match self.fs.read(&path, 4 * 1024 * 1024) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(_) => return Ok(()),
        };
        let new = remove_default_lines(&text, DESKTOP_ID, &none);
        if new != text {
            if new.trim().is_empty() {
                self.fs
                    .remove_file(&path)
                    .map_err(|e| failed(e.to_string()))?;
            } else {
                self.fs
                    .atomic_write(&path, new.as_bytes(), 0o644)
                    .map_err(|e| failed(e.to_string()))?;
            }
        }
        Ok(())
    }
}

fn is_desktop_id(id: &str) -> bool {
    id.ends_with(".desktop")
        && !id.contains('/')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

/// Remove `type=…` entries naming `desktop_id` from `[Default Applications]` for `types`.
/// Lines listing several applications keep the others. Every other byte is preserved; a
/// `[Default Applications]` group left empty is removed together with its header.
pub fn remove_default_lines(text: &str, desktop_id: &str, types: &[MimeType]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut in_defaults = false;
    let mut group_start: Option<usize> = None;
    let mut group_kept_entries = 0usize;
    let finish_group = |out: &mut Vec<String>, start: Option<usize>, kept: usize| {
        if let Some(s) = start
            && kept == 0
        {
            out.truncate(s);
        }
    };
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            if in_defaults {
                finish_group(&mut out, group_start, group_kept_entries);
            }
            in_defaults = trimmed == "[Default Applications]";
            group_start = in_defaults.then_some(out.len());
            group_kept_entries = 0;
            out.push(line.to_owned());
            continue;
        }
        if in_defaults && let Some((k, v)) = trimmed.split_once('=') {
            if types.iter().any(|t| t.as_str() == k.trim()) {
                let rest: Vec<&str> = v
                    .split(';')
                    .map(str::trim)
                    .filter(|a| !a.is_empty() && *a != desktop_id)
                    .collect();
                if rest.is_empty() {
                    continue;
                }
                if rest.len() != v.split(';').filter(|a| !a.trim().is_empty()).count() {
                    let nl = if line.ends_with('\n') { "\n" } else { "" };
                    out.push(format!("{}={};{nl}", k.trim(), rest.join(";")));
                    group_kept_entries += 1;
                    continue;
                }
            }
            group_kept_entries += 1;
        } else if in_defaults && !trimmed.is_empty() {
            group_kept_entries += 1;
        }
        out.push(line.to_owned());
    }
    if in_defaults {
        finish_group(&mut out, group_start, group_kept_entries);
    }
    out.concat()
}

/// A minimal valid PE image header ("MZ", `e_lfanew`, "PE\0\0", i386 COFF header).
pub fn pe_sample() -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[0] = b'M';
    b[1] = b'Z';
    b[0x3c] = 0x80;
    b[0x80..0x84].copy_from_slice(b"PE\0\0");
    b[0x84] = 0x4c; // IMAGE_FILE_MACHINE_I386
    b[0x85] = 0x01;
    b
}

/// The compound-file signature an MSI starts with.
pub fn msi_sample() -> Vec<u8> {
    let mut b = vec![0u8; 512];
    b[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> MimeType {
        MimeType::parse(s).unwrap()
    }

    #[test]
    fn mime_types_are_validated() {
        assert!(MimeType::parse("application/x-msdownload").is_some());
        assert!(MimeType::parse("application/x;rm").is_none());
        assert!(MimeType::parse("noslash").is_none());
        assert!(MimeType::parse("a/").is_none());
    }

    #[test]
    fn removing_our_lines_preserves_everything_else() {
        let text = "# mine\n[Added Associations]\napplication/x-msdownload=wine.desktop;\n\n\
                    [Default Applications]\ntext/plain=gedit.desktop\n\
                    application/x-msdownload=io.github.zaun.Zaun.desktop\n\
                    application/x-msi=io.github.zaun.Zaun.desktop;wine.desktop;\n";
        let out = remove_default_lines(
            text,
            DESKTOP_ID,
            &[t("application/x-msdownload"), t("application/x-msi")],
        );
        assert_eq!(
            out,
            "# mine\n[Added Associations]\napplication/x-msdownload=wine.desktop;\n\n\
             [Default Applications]\ntext/plain=gedit.desktop\n\
             application/x-msi=wine.desktop;\n"
        );
        // A group that only held our lines disappears with its header.
        let only = "[Default Applications]\napplication/x-msdownload=io.github.zaun.Zaun.desktop\n";
        assert_eq!(
            remove_default_lines(only, DESKTOP_ID, &[t("application/x-msdownload")]),
            ""
        );
        let before = "[A]\nx=y\n";
        assert_eq!(
            remove_default_lines(
                &format!("{before}{only}"),
                DESKTOP_ID,
                &[t("application/x-msdownload")]
            ),
            before
        );
        // Unrelated types are untouched.
        assert_eq!(remove_default_lines(only, DESKTOP_ID, &[t("a/b")]), only);
    }

    #[test]
    fn overall_prefers_a_type_with_a_default() {
        let s = |m: &str, d: Option<&str>| HandlerStatus {
            mime: t(m),
            default: d.map(str::to_owned),
            name: None,
        };
        assert_eq!(overall(&[]), Overall::Nothing);
        assert_eq!(
            overall(&[s("a/b", None), s("a/c", Some(DESKTOP_ID))]),
            Overall::Zaun
        );
        assert_eq!(
            overall(&[s("a/b", Some("wine.desktop"))]),
            Overall::Other("wine.desktop".into())
        );
    }

    #[test]
    fn samples_have_magic() {
        assert_eq!(&pe_sample()[..2], b"MZ");
        assert_eq!(&pe_sample()[0x80..0x84], b"PE\0\0");
        assert_eq!(msi_sample()[0], 0xD0);
    }
}

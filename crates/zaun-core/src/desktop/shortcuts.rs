//! Per-game shortcuts (DESKTOP_INTEGRATION §6).
//!
//! Created only on request. A menu entry is
//! `$XDG_DATA_HOME/applications/zaun-game-<id>.desktop`; an optional copy goes to the XDG
//! Desktop directory as `<sanitised name>.desktop` (mode 0755, `metadata::trusted` set with
//! `gio` where present); the icon is
//! `$XDG_DATA_HOME/icons/hicolor/256x256/apps/zaun-game-<id>.png`. Shortcuts are found
//! again by the file-name pattern and the `X-Zaun-GameId` key, so renaming rewrites and
//! removal deletes exactly Zaun's files.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::desktop_entry::{self, GAME_ID_KEY, GameEntry, SHORTCUT_PREFIX};
use super::mime::XdgDirs;
use crate::error::{Error, ErrorCode};
use crate::fsx::{FileType, Fs};
use crate::host::exec::{find_in, path_dirs};
use crate::host::{CommandRunner, CommandSpec};
use crate::ids::GameId;
use crate::paths::PathEnv;

/// Largest desktop file read while listing.
const ENTRY_MAX_BYTES: u64 = 64 * 1024;

/// A shortcut Zaun created.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutInfo {
    /// The game.
    pub id: GameId,
    /// The name written into the entry.
    pub name: String,
    /// The menu entry.
    pub menu: PathBuf,
    /// Copies on the Desktop.
    pub desktop_copies: Vec<PathBuf>,
}

/// What [`Shortcuts::create`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Created {
    /// The Desktop copy, when one was asked for.
    pub desktop_copy: Option<PathBuf>,
    /// Whether the copy was marked trusted (`false`: the user has to allow launching once).
    pub trusted: bool,
}

/// Creates, rewrites, removes and lists per-game shortcuts.
pub struct Shortcuts {
    fs: Arc<dyn Fs>,
    dirs: XdgDirs,
    desktop_dir: Option<PathBuf>,
    runner: Option<Arc<dyn CommandRunner>>,
    gio: Option<PathBuf>,
}

fn failed(e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::DskShortcutFailed).with("detail", e.to_string())
}

impl Shortcuts {
    /// Shortcuts in `dirs`; `desktop_dir` is the XDG Desktop directory (if any); `runner`
    /// runs `gio` (found on `env`'s `PATH`) for the trusted attribute.
    pub fn new(
        fs: Arc<dyn Fs>,
        dirs: XdgDirs,
        desktop_dir: Option<PathBuf>,
        runner: Option<Arc<dyn CommandRunner>>,
        env: &PathEnv,
    ) -> Self {
        let gio = runner
            .as_ref()
            .and_then(|_| find_in(&*fs, &path_dirs(env.get("PATH")), "gio"));
        Self {
            fs,
            dirs,
            desktop_dir,
            runner,
            gio,
        }
    }

    /// The XDG Desktop directory of `env` (`user-dirs.dirs`, default `~/Desktop`).
    pub fn desktop_dir_of(fs: &dyn Fs, env: &PathEnv, dirs: &XdgDirs) -> Option<PathBuf> {
        let home = env.get("HOME").map(PathBuf::from)?;
        let from_file = fs
            .read(&dirs.config_home.join("user-dirs.dirs"), 64 * 1024)
            .ok()
            .and_then(|b| {
                crate::identity::sensitive::parse_user_dirs(&String::from_utf8_lossy(&b), &home)
                    .into_iter()
                    .find(|(k, _)| k == "Desktop")
                    .map(|(_, p)| p)
            });
        Some(from_file.unwrap_or_else(|| home.join("Desktop")))
    }

    fn applications(&self) -> PathBuf {
        self.dirs.data_home.join("applications")
    }

    /// The menu entry's path.
    pub fn menu_path(&self, id: GameId) -> PathBuf {
        self.applications().join(desktop_entry::file_name(id))
    }

    /// The exported icon's path.
    pub fn icon_path(&self, id: GameId) -> PathBuf {
        self.dirs
            .data_home
            .join("icons/hicolor/256x256/apps")
            .join(format!("{}.png", desktop_entry::icon_name(id)))
    }

    /// Whether the game has a menu entry.
    pub fn exists(&self, id: GameId) -> bool {
        self.fs.exists(&self.menu_path(id))
    }

    /// Create (or rewrite) the menu entry, export `icon_png` (a 256 × 256 PNG) and, with
    /// `on_desktop`, put a copy on the Desktop.
    pub fn create(
        &self,
        id: GameId,
        name: &str,
        icon_png: Option<&[u8]>,
        on_desktop: bool,
    ) -> Result<Created, Error> {
        let icon = match icon_png {
            Some(png) => {
                let p = self.icon_path(id);
                if let Some(d) = p.parent() {
                    self.fs.create_dir_all(d, 0o755).map_err(failed)?;
                }
                self.fs.atomic_write(&p, png, 0o644).map_err(failed)?;
                Some(desktop_entry::icon_name(id))
            }
            None => None,
        };
        let text = desktop_entry::render(&GameEntry { id, name, icon });
        self.fs
            .create_dir_all(&self.applications(), 0o755)
            .map_err(failed)?;
        self.fs
            .atomic_write(&self.menu_path(id), text.as_bytes(), 0o644)
            .map_err(failed)?;
        let mut created = Created::default();
        if on_desktop {
            let dir = self
                .desktop_dir
                .as_ref()
                .ok_or_else(|| failed("no Desktop folder"))?;
            for old in self.desktop_copies(id) {
                let _ = self.fs.remove_file(&old);
            }
            let p = dir.join(desktop_entry::desktop_file_name(name, id));
            self.fs.create_dir_all(dir, 0o755).map_err(failed)?;
            self.fs
                .atomic_write(&p, text.as_bytes(), 0o755)
                .map_err(failed)?;
            created.trusted = self.mark_trusted(&p);
            created.desktop_copy = Some(p);
        }
        Ok(created)
    }

    fn mark_trusted(&self, p: &Path) -> bool {
        let (Some(runner), Some(gio)) = (&self.runner, &self.gio) else {
            return false;
        };
        let spec = CommandSpec::new(
            gio,
            &[
                "set".as_ref(),
                p.as_os_str(),
                "metadata::trusted".as_ref(),
                "true".as_ref(),
            ],
        );
        runner.run(&spec).is_ok_and(|o| o.success())
    }

    /// Rewrite the shortcuts of a renamed game (no-op without one).
    pub fn rename(&self, id: GameId, name: &str) -> Result<(), Error> {
        if !self.exists(id) {
            return Ok(());
        }
        let had_desktop = !self.desktop_copies(id).is_empty();
        let icon = self
            .fs
            .exists(&self.icon_path(id))
            .then(|| desktop_entry::icon_name(id));
        let text = desktop_entry::render(&GameEntry { id, name, icon });
        self.fs
            .atomic_write(&self.menu_path(id), text.as_bytes(), 0o644)
            .map_err(failed)?;
        if had_desktop && let Some(dir) = &self.desktop_dir {
            for old in self.desktop_copies(id) {
                let _ = self.fs.remove_file(&old);
            }
            let p = dir.join(desktop_entry::desktop_file_name(name, id));
            self.fs
                .atomic_write(&p, text.as_bytes(), 0o755)
                .map_err(failed)?;
            self.mark_trusted(&p);
        }
        Ok(())
    }

    /// Remove the menu entry, the Desktop copies and the exported icon.
    pub fn remove(&self, id: GameId) -> Result<(), Error> {
        let mut paths = self.desktop_copies(id);
        paths.push(self.menu_path(id));
        paths.push(self.icon_path(id));
        for p in paths {
            match self.fs.remove_file(&p) {
                Ok(()) => {}
                Err(e) if e.kind == crate::fsx::FsErrorKind::NotFound => {}
                Err(e) => return Err(failed(e)),
            }
        }
        Ok(())
    }

    fn game_of(&self, path: &Path) -> Option<(GameId, String)> {
        let bytes = self.fs.read(path, ENTRY_MAX_BYTES).ok()?;
        let text = String::from_utf8_lossy(&bytes);
        let id: GameId = desktop_entry::read_key(&text, GAME_ID_KEY)?.parse().ok()?;
        // Only Zaun's own entries: the Exec line must be exactly ours.
        if desktop_entry::read_key(&text, "Exec")? != desktop_entry::exec_line(id) {
            return None;
        }
        Some((
            id,
            desktop_entry::read_key(&text, "Name").unwrap_or_default(),
        ))
    }

    fn desktop_copies(&self, id: GameId) -> Vec<PathBuf> {
        let Some(dir) = &self.desktop_dir else {
            return Vec::new();
        };
        self.fs
            .read_dir(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.file_type == FileType::File)
            .filter(|e| e.name.to_string_lossy().ends_with(".desktop"))
            .filter(|e| self.game_of(&e.path).is_some_and(|(g, _)| g == id))
            .map(|e| e.path)
            .collect()
    }

    /// Every shortcut Zaun created (Settings → Integration).
    pub fn list(&self) -> Vec<ShortcutInfo> {
        self.fs
            .read_dir(&self.applications())
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.file_type == FileType::File)
            .filter(|e| {
                let n = e.name.to_string_lossy();
                n.starts_with(SHORTCUT_PREFIX) && n.ends_with(".desktop")
            })
            .filter_map(|e| {
                let (id, name) = self.game_of(&e.path)?;
                (e.path == self.menu_path(id)).then(|| ShortcutInfo {
                    id,
                    name,
                    desktop_copies: self.desktop_copies(id),
                    menu: e.path,
                })
            })
            .collect()
    }
}

#[cfg(test)]
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;
    use crate::fsx::RealFs;

    fn id() -> GameId {
        "0192f0c2-7b1e-7c3a-9d2f-5e1a2b3c4d5e".parse().unwrap()
    }

    fn files(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_path_buf()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                } else {
                    out.push(p);
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn create_rename_remove_leave_no_stray_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = XdgDirs {
            config_home: tmp.path().join("config"),
            data_home: tmp.path().join("data"),
            data_dirs: vec![],
        };
        let desk = tmp.path().join("Desktop");
        let s = Shortcuts::new(
            Arc::new(RealFs::new()),
            dirs,
            Some(desk.clone()),
            None,
            &PathEnv::new(1000),
        );
        let c = s
            .create(id(), "Hollow\nKnight", Some(b"\x89PNG fake"), true)
            .unwrap();
        assert!(!c.trusted);
        assert_eq!(c.desktop_copy, Some(desk.join("Hollow Knight.desktop")));
        let menu = std::fs::read_to_string(s.menu_path(id())).unwrap();
        assert!(menu.contains("Name=Hollow Knight\n"));
        assert!(menu.contains(&format!("Icon=zaun-game-{}\n", id())));
        let l = s.list();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "Hollow Knight");
        assert_eq!(l[0].desktop_copies.len(), 1);
        s.rename(id(), "Silksong").unwrap();
        assert!(desk.join("Silksong.desktop").exists());
        assert!(!desk.join("Hollow Knight.desktop").exists());
        assert_eq!(s.list()[0].name, "Silksong");
        // A foreign file with our key but another Exec is never touched.
        std::fs::write(
            desk.join("other.desktop"),
            format!("[Desktop Entry]\nExec=evil\n{GAME_ID_KEY}={}\n", id()),
        )
        .unwrap();
        s.remove(id()).unwrap();
        assert_eq!(files(tmp.path()), vec![desk.join("other.desktop")]);
        assert!(s.list().is_empty());
        s.remove(id()).unwrap();
    }
}

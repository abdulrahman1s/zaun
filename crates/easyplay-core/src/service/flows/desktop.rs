//! Desktop commands (DESKTOP_INTEGRATION §4, §6, §8): the `.exe` default handler, per-game
//! menu shortcuts, opening folders and session logs. `xdg-mime`, `xdg-open` and file writes
//! run on the job pool; results come back as [`Internal::Desktop`]. The integration state
//! (handler, shortcuts) is published in the snapshot hub's `integration` cell.

use std::path::PathBuf;
use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::command::FolderRequest;
use super::super::event::Event;
use crate::artwork::ImageLimits;
use crate::artwork::codec::{pad_square, resize};
use crate::config::PreferencePatch;
use crate::desktop::mime::{self, KV_PREVIOUS, PreviousHandlers};
use crate::desktop::{DesktopServices, IntegrationState, KnownPath};
use crate::error::{Error, ErrorCode};
use crate::ids::{GameId, SessionId};
use crate::jobs::{JobLabel, JobPriority};
use crate::model::{SaveDataLocation, SessionKind};
use crate::storage::{KvRepo, SessionRepo};

/// Results of desktop jobs.
pub(crate) enum DesktopDone {
    /// A fresh integration state.
    State(IntegrationState),
    /// EasyPlay became the default; the previous handlers.
    HandlerSet(Result<PreviousHandlers, Error>),
    /// The previous handlers were restored.
    HandlerRestored(Result<(), Error>),
    /// A job failed (shown as a failure).
    Failed(Error),
    /// Done; nothing to publish.
    Done,
}

impl AppService {
    fn desktop(&self) -> Option<Arc<DesktopServices>> {
        self.deps.desktop.clone()
    }

    fn desktop_job(
        &self,
        label: &'static str,
        f: impl FnOnce(&DesktopServices) -> DesktopDone + Send + 'static,
    ) {
        let Some(d) = self.desktop() else {
            return self.fail(Error::not_available("DESKTOP-005"));
        };
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel(label), move |_| {
                post(Internal::Desktop(Box::new(f(&d))));
            });
    }

    /// Query the handler and list the shortcuts (start-up and after every change).
    pub(crate) fn refresh_integration(&self) {
        if self.desktop().is_none() {
            return;
        }
        self.desktop_job("desktop-status", |d| {
            DesktopDone::State(integration_state(d))
        });
    }

    /// `SetDefaultHandler`: only ever on the user's request (§4).
    pub(crate) fn set_default_handler(&mut self, enable: bool) {
        let choice = if enable { "yes" } else { "no" };
        self.handle(super::super::command::Command::SetPreference(
            PreferencePatch::new().set("integration.handle_exe", choice),
        ));
        if enable {
            self.desktop_job("desktop-handler", |d| {
                let types = d.mime.managed_types();
                DesktopDone::HandlerSet(d.mime.set_default(&types))
            });
            return;
        }
        let previous = match self.lib.db_mut().kv_get(KV_PREVIOUS) {
            Ok(Some(json)) => serde_json::from_str::<PreviousHandlers>(&json).ok(),
            _ => None,
        };
        let Some(previous) = previous else {
            // Nothing was taken over; "no" only records the choice.
            return self.refresh_integration();
        };
        self.desktop_job("desktop-handler", move |d| {
            DesktopDone::HandlerRestored(d.mime.restore(&previous))
        });
    }

    /// `SetShortcut`: create or remove a game's applications-menu entry (§6).
    pub(crate) fn set_shortcut(&mut self, id: GameId, enable: bool) {
        let snap = self.lib.snapshot();
        let Some(g) = snap.get(id) else {
            return self.fail(Error::new(ErrorCode::LibInvalidField).with("game", id));
        };
        let name = g.name.to_string();
        let icon_file = g
            .icon
            .and_then(|k| self.deps.art_index.get(&k.0))
            .map(|(p, _)| p);
        let fs = Arc::clone(&self.deps.fs);
        let codec = self.deps.codec.clone();
        self.desktop_job("desktop-shortcut", move |d| {
            let r = if enable {
                let png = icon_file.and_then(|p| {
                    let codec = codec?;
                    let bytes = fs.read(&p, ImageLimits::DEFAULT.max_bytes).ok()?;
                    let img = codec.decode(&bytes, &ImageLimits::DEFAULT).ok()?;
                    codec.encode_png(&resize(&pad_square(&img), 256, 256)).ok()
                });
                d.shortcuts
                    .create(id, &name, png.as_deref(), false)
                    .map(|_| ())
            } else {
                d.shortcuts.remove(id)
            };
            match r {
                Ok(()) => DesktopDone::State(integration_state_shortcuts(d)),
                Err(e) => DesktopDone::Failed(e),
            }
        });
    }

    /// Keep shortcuts in step with the library: renamed games are rewritten, removed games
    /// lose theirs (§6). Cheap: one directory listing on the job pool.
    pub(crate) fn sync_shortcuts(&self) {
        let current = self.hub.integration.load();
        let current = &current.shortcuts;
        if current.is_empty() {
            return;
        }
        let snap = self.lib.snapshot();
        let mut work: Vec<(GameId, Option<String>)> = Vec::new();
        for s in current {
            match snap.get(s.id) {
                None => work.push((s.id, None)),
                Some(g)
                    if crate::desktop::desktop_entry::escape_value(&g.name)
                        != crate::desktop::desktop_entry::escape_value(&s.name) =>
                {
                    work.push((s.id, Some(g.name.to_string())));
                }
                Some(_) => {}
            }
        }
        if work.is_empty() {
            return;
        }
        self.desktop_job("desktop-shortcut-sync", move |d| {
            for (id, name) in work {
                let r = match name {
                    Some(n) => d.shortcuts.rename(id, &n),
                    None => d.shortcuts.remove(id),
                };
                if let Err(e) = r {
                    tracing::warn!(game = %id, error = %e, "shortcut not updated");
                }
            }
            DesktopDone::State(integration_state_shortcuts(d))
        });
    }

    /// `OpenFolder`: only folders EasyPlay knows (§8).
    pub(crate) fn open_folder(&mut self, target: FolderRequest) {
        let path = match self.folder_path(&target) {
            Ok(p) => p,
            Err(e) => return self.fail(e),
        };
        let opener = self.desktop().map(|d| Arc::clone(&d.opener));
        let fs = Arc::clone(&self.deps.fs);
        self.desktop_job("desktop-open", move |_| {
            if !fs.metadata_follow(&path).is_ok_and(|m| m.is_dir()) {
                return DesktopDone::Failed(
                    Error::new(ErrorCode::FsReadDenied).with("path", path.display()),
                );
            }
            match opener.map(|o| o.open(&KnownPath::folder(path))) {
                Some(Err(e)) => DesktopDone::Failed(e),
                _ => DesktopDone::Done,
            }
        });
    }

    fn folder_path(&mut self, target: &FolderRequest) -> Result<PathBuf, Error> {
        let paths = &self.deps.paths;
        let game = |svc: &mut Self, id: GameId| svc.lib.game(id);
        Ok(match target {
            FolderRequest::DataFolder => paths.data_dir.clone(),
            FolderRequest::Session(s) => paths.session_dir(*s),
            FolderRequest::Prefix(id) => paths.prefix_dir(*id),
            FolderRequest::GameFolder(id) => {
                let g = game(self, *id)?;
                match g.root {
                    Some(r) => r,
                    None => g
                        .primary()
                        .and_then(|e| e.path.parent().map(PathBuf::from))
                        .ok_or_else(|| Error::new(ErrorCode::FsReadDenied))?,
                }
            }
            FolderRequest::SaveData(id) => {
                let g = game(self, *id)?;
                match &g.policy.save_data {
                    SaveDataLocation::External { dir } => dir.clone(),
                    SaveDataLocation::Internal => {
                        let users = self.deps.paths.prefix_dir(*id).join("drive_c/users");
                        let user = users.join("steamuser");
                        if self.deps.fs.exists(&user) {
                            user
                        } else {
                            users
                        }
                    }
                }
            }
            FolderRequest::Granted(p) => {
                // Only a folder some game was granted (the user chose it).
                let snap = self.lib.snapshot();
                let ids: Vec<GameId> = snap.games.iter().map(|g| g.id).collect();
                let granted = ids.into_iter().any(|id| {
                    self.lib.game(id).is_ok_and(|g| {
                        g.policy
                            .folders
                            .iter()
                            .any(|f| f.path == *p || f.canonical == *p)
                    })
                });
                if !granted {
                    return Err(Error::new(ErrorCode::FsReadDenied)
                        .with("detail", "not a folder EasyPlay manages"));
                }
                p.clone()
            }
        })
    }

    /// `ViewLog`: open a session's `game.log` (the game's latest session by default).
    pub(crate) fn view_log(&mut self, session: Option<SessionId>, game: Option<GameId>) {
        let sid = match (session, game) {
            (Some(s), _) => Some(s),
            (None, Some(g)) => self
                .lib
                .db_mut()
                .session_history(Some(g), &[SessionKind::Game], 0, 1)
                .ok()
                .and_then(|v| v.first().map(|s| s.id)),
            (None, None) => None,
        };
        let Some(sid) = sid else {
            return self.fail(Error::new(ErrorCode::FsReadDenied).with("detail", "no session log"));
        };
        let dir = self.deps.paths.session_dir(sid);
        let opener = self.desktop().map(|d| Arc::clone(&d.opener));
        let fs = Arc::clone(&self.deps.fs);
        self.desktop_job("desktop-log", move |_| {
            let log = dir.join("game.log");
            let target = if fs.exists(&log) {
                KnownPath::file(log)
            } else if fs.exists(&dir) {
                KnownPath::folder(dir)
            } else {
                return DesktopDone::Failed(
                    Error::new(ErrorCode::FsReadDenied).with("detail", "the log was removed"),
                );
            };
            match opener.map(|o| o.open(&target)) {
                Some(Err(e)) => DesktopDone::Failed(e),
                _ => DesktopDone::Done,
            }
        });
    }

    pub(crate) fn on_desktop(&mut self, done: DesktopDone) {
        match done {
            DesktopDone::Done => {}
            DesktopDone::State(mut s) => {
                let cur = self.hub.integration.load();
                if s.handler.is_none() {
                    s.handler = cur.handler.clone();
                    s.handler_unavailable = cur.handler_unavailable;
                }
                self.hub.integration.store(Arc::new(s));
                self.emit(Event::HostFactsChanged);
                // Shortcuts of games renamed or removed while EasyPlay was closed.
                self.sync_shortcuts();
            }
            DesktopDone::HandlerSet(Ok(previous)) => {
                let now = self.deps.clock.now();
                let db = self.lib.db_mut();
                // Only the first takeover is recorded: later ones would record EasyPlay.
                if db.kv_get(KV_PREVIOUS).ok().flatten().is_none()
                    && let Ok(json) = serde_json::to_string(&previous)
                    && let Err(e) = db.kv_set(KV_PREVIOUS, &json, now)
                {
                    tracing::warn!(error = %e, "previous handlers not recorded");
                }
                self.refresh_integration();
            }
            DesktopDone::HandlerRestored(Ok(())) => {
                let _ = self.lib.db_mut().kv_delete(KV_PREVIOUS);
                self.refresh_integration();
            }
            DesktopDone::HandlerSet(Err(e)) | DesktopDone::HandlerRestored(Err(e)) => {
                self.fail(e);
                self.refresh_integration();
            }
            DesktopDone::Failed(e) => self.fail(e),
        }
    }
}

fn integration_state_shortcuts(d: &DesktopServices) -> IntegrationState {
    IntegrationState {
        handler: None,
        handler_unavailable: false,
        shortcuts: d.shortcuts.list(),
    }
}

fn integration_state(d: &DesktopServices) -> IntegrationState {
    let (handler, unavailable) = if d.mime_available {
        match d.mime.status() {
            Ok(s) => (Some(mime::overall(&s)), false),
            Err(e) => {
                tracing::info!(error = %e, "handler status unknown");
                (None, false)
            }
        }
    } else {
        (None, true)
    };
    IntegrationState {
        handler,
        handler_unavailable: unavailable,
        shortcuts: d.shortcuts.list(),
    }
}

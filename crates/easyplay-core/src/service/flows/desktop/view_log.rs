//! `ViewLog`: open a session's `game.log`, the game's latest session by default (DESKTOP_INTEGRATION §8).
use super::DesktopDone;
use crate::desktop::KnownPath;
use crate::model::SessionKind;
use crate::service::prelude::*;
use crate::storage::SessionRepo;

impl AppService {
    pub(crate) fn view_log(
        &mut self,
        session: Option<SessionId>,
        game: Option<GameId>,
    ) -> Result<()> {
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
            bail!(FsReadDenied, detail = "no session log");
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
                return DesktopDone::Failed(err!(FsReadDenied, detail = "the log was removed"));
            };
            match opener.map(|o| o.open(&target)) {
                Some(Err(e)) => DesktopDone::Failed(e),
                _ => DesktopDone::Done,
            }
        })
    }
}

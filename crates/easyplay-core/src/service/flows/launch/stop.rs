//! `StopSession`: ask the supervisor to stop; a supervisor that is gone means recovery (SVC-005).
use crate::session::StopOutcome;

use crate::service::prelude::*;

impl AppService {
    pub(crate) fn stop_session(&mut self, session: SessionId, mode: StopMode) {
        if let Err(e) = self.try_stop_session(session, mode) {
            self.fail(e);
        }
    }

    fn try_stop_session(&mut self, session: SessionId, mode: StopMode) -> Result<()> {
        let row = match self.sessions.get(&session).cloned() {
            Some(s) => s,
            None => match self.lib.db_mut().session(session)? {
                Some(s) if s.is_open() => s,
                _ => return Ok(()), // already ended: idempotent
            },
        };
        match self.deps.launcher.request_stop(&row, mode)? {
            StopOutcome::Signalled => {
                self.notice(Notice::StopRequested { session });
                self.poke_watcher();
            }
            StopOutcome::SupervisorGone => self.recover_sessions(),
        }
        Ok(())
    }
}

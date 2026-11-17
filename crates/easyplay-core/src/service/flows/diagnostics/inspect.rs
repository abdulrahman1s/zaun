//! `Inspect`: the launch inspector's data for a session or the next launch (DEV-001).
use crate::launch;
use crate::service::command::InspectTarget;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn inspect(&mut self, target: InspectTarget, reply_to: QueryId) -> Result<()> {
        match target {
            InspectTarget::Session(sid) => self.inspect_session(sid, reply_to),
            InspectTarget::NextLaunch { game, exe } => self.inspect_next(game, exe, reply_to),
        }
        Ok(())
    }

    /// Answer `id` with a failure.
    pub(super) fn query_failed(&self, id: QueryId, u: UserError) {
        self.emit(Event::QueryResult {
            id,
            result: QueryResult::Failed(Box::new(u)),
        });
    }

    fn inspect_session(&mut self, sid: SessionId, reply_to: QueryId) {
        let row = match self.sessions.get(&sid).cloned() {
            Some(s) => Some(s),
            None => self.lib.db_mut().session(sid).ok().flatten(),
        };
        let describe = self.describer(row.as_ref().and_then(|s| s.game_id));
        let Some(row) = row else {
            return self.query_failed(reply_to, describe(&err!(SesSessionNotFound, session = sid)));
        };
        let fs = Arc::clone(&self.deps.fs);
        let paths = self.deps.paths.clone();
        self.answer_on_pool("inspect-session", reply_to, move |_| {
            match launch::inspect::load_session(&*fs, &paths, &row) {
                Ok(d) => QueryResult::Inspector(Box::new(d)),
                Err(e) => QueryResult::Failed(Box::new(describe(&err!(
                    FsReadDenied,
                    detail = e.to_string(),
                    session = sid
                )))),
            }
        });
    }
}

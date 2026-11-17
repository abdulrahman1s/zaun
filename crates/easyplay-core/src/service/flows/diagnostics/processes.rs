//! `ListProcesses`: the open session's processes now, with CPU time and memory (DEV-003).
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn list_processes(&mut self, session: SessionId, reply_to: QueryId) -> Result<()> {
        let Some(s) = self.sessions.get(&session).cloned() else {
            self.emit(Event::QueryResult {
                id: reply_to,
                result: QueryResult::Processes(Vec::new()),
            });
            return Ok(());
        };
        let target = s
            .game_id
            .and_then(|g| self.lib.game(g).ok())
            .and_then(|g| {
                s.executable_id
                    .and_then(|id| g.executables.iter().find(|e| e.id == id).cloned())
                    .or_else(|| g.primary().cloned())
            })
            .and_then(|e| e.path.file_name().map(|n| n.to_string_lossy().into_owned()));
        let proc = Arc::clone(&self.deps.proc);
        let own_ns = self
            .deps
            .proc
            .entry(std::process::id())
            .ok()
            .flatten()
            .and_then(|e| e.pid_ns);
        self.answer_on_pool("list-processes", reply_to, move |_| {
            QueryResult::Processes(crate::session::process_list(
                &*proc,
                &s,
                own_ns,
                target.as_deref(),
                true,
            ))
        });
        Ok(())
    }
}

//! The engine failed a request or exited: say why, fail waiting pages, restart with a back-off
//! (ADR-0033). After `MAX_FAILURES` exits without a successful start Chat stops with
//! `ZN-CHT-002`.
use super::state::{ChatDone, MAX_FAILURES, backoff};
use crate::chat::RequestKind;
use crate::model::ChatStatus;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_chat_failed(
        &mut self,
        request: RequestKind,
        code: ErrorCode,
        detail: String,
    ) -> Result<()> {
        tracing::warn!(
            ?request,
            code = code.as_str(),
            detail,
            "chat: request failed"
        );
        if request == RequestKind::Start {
            self.on_start_failed(code);
            // The Chat page says why (its status); a dialog on top would say it twice.
            return Ok(());
        }
        if request == RequestKind::History {
            self.fail_oldest_page(code);
            return Ok(());
        }
        Err(Error::new(code))
    }

    /// Not something a restart fixes (no engine, a database that does not open, a name or
    /// relays the engine refuses): stop and say why. Turning Chat off and on again retries.
    fn on_start_failed(&mut self, code: ErrorCode) {
        self.call_reset();
        self.stream_reset();
        let st = &mut self.flow.chat;
        st.engine = None;
        st.started_with = None;
        st.generation += 1;
        let status = if code == ErrorCode::ChtUnavailable {
            ChatStatus::Unavailable
        } else {
            ChatStatus::Failed(code)
        };
        self.chat_set_status(status);
        self.chat_fail_pages(code);
    }

    /// The engine answers pages in order; the oldest waiting one failed.
    fn fail_oldest_page(&mut self, code: ErrorCode) {
        if let Some((_, id)) = self.flow.chat.pages.pop_first() {
            self.answer_page_failed(id, code);
        }
    }

    /// Answer every page still waiting with a failure (the engine stopped).
    pub(super) fn chat_fail_pages(&mut self, code: ErrorCode) {
        for id in std::mem::take(&mut self.flow.chat.pages).into_values() {
            self.answer_page_failed(id, code);
        }
    }

    /// The engine's own words stay in the log: they name relays and internals, never
    /// anything the user can act on.
    fn answer_page_failed(&self, id: QueryId, code: ErrorCode) {
        let user = self.describe(&Error::new(code), None);
        self.emit(Event::QueryResult {
            id,
            result: QueryResult::Failed(Box::new(user)),
        });
    }

    pub(super) fn on_chat_exited(&mut self, status: Option<i32>, started: bool) -> Result<()> {
        tracing::warn!(?status, started, "chat: the chat engine stopped");
        self.chat_fail_pages(ErrorCode::ChtEngineFailing);
        self.call_reset();
        self.stream_reset();
        let st = &mut self.flow.chat;
        st.engine = None;
        st.started_with = None;
        if !self.prefs.chat.enabled
            || matches!(
                st.snapshot.status,
                ChatStatus::Unavailable | ChatStatus::Failed(_)
            )
        {
            return Ok(());
        }
        st.failures += 1;
        if st.failures >= MAX_FAILURES {
            self.chat_set_status(ChatStatus::Failed(ErrorCode::ChtEngineFailing));
            bail!(ChtEngineFailing);
        }
        self.chat_set_status(ChatStatus::Starting);
        let wait = backoff(self.flow.chat.failures);
        let wait = self
            .deps
            .chat
            .as_ref()
            .map_or(wait, |f| f.restart_wait(wait));
        self.schedule_restart(wait);
        Ok(())
    }

    fn schedule_restart(&self, wait: Duration) {
        let generation = self.flow.chat.generation;
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("chat-restart".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Chat(
                    ChatDone::Restart { generation },
                ))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "chat: restart timer not started");
        }
    }
}

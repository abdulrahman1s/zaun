//! Start, stop and restart the engine to match `chat.*`; publish the snapshot (ADR-0033).
//!
//! - Every engine start has a generation: events of an engine already replaced or stopped
//!   are dropped, so a late `Exited` never restarts anything.
//! - Turning Chat on makes the data folder (0700) and the database key if needed.
use super::state::{ChatDone, PAPER_FILE};
use crate::chat::{EngineConfig, EventSink, Request};
use crate::model::{ChatSnapshot, ChatStatus};
use crate::service::prelude::*;

impl AppService {
    /// Start-up: run the engine when Chat is on.
    pub(crate) fn chat_start(&mut self) {
        self.chat_preference_changed();
        if self.flow.chat.engine.is_none() {
            // Off: publish the build's features once, for Settings.
            self.chat_publish();
        }
    }

    /// The preferences changed: start, stop or restart the engine to match.
    pub(crate) fn chat_preference_changed(&mut self) {
        let p = &self.prefs.chat;
        let want = p.enabled.then(|| (p.name.clone(), p.relays.clone()));
        let st = &self.flow.chat;
        match (&want, &st.started_with) {
            (None, None) => {
                if st.snapshot.status == ChatStatus::Off {
                    // The relays may have changed what files can do.
                    self.chat_publish();
                } else {
                    self.chat_stop();
                }
            }
            (None, Some(_)) => self.chat_stop(),
            // Same relays; a new name for a first start does not matter once started.
            (Some(w), Some(s)) if w.1 == s.1 => {}
            (Some(_), Some(_)) => {
                self.chat_stop();
                self.flow.chat.failures = 0;
                self.launch_or_fail();
            }
            (Some(_), None) => {
                if !matches!(st.snapshot.status, ChatStatus::Failed(_)) {
                    self.launch_or_fail();
                }
            }
        }
    }

    /// The service is ending: let the engine close its database before the process goes
    /// (the helper would otherwise be killed with the service thread).
    pub(crate) fn chat_shutdown(&mut self) {
        self.call_reset();
        self.stream_reset();
        if let Some(mut e) = self.flow.chat.engine.take() {
            self.flow.chat.generation += 1;
            e.shutdown();
        }
    }

    pub(super) fn launch_or_fail(&mut self) {
        if let Err(e) = self.chat_launch() {
            self.fail(e);
        }
    }

    fn chat_stop(&mut self) {
        self.chat_fail_pages(ErrorCode::ChtUnavailable);
        self.call_reset();
        self.stream_reset();
        let st = &mut self.flow.chat;
        st.engine = None;
        st.started_with = None;
        st.generation += 1;
        st.failures = 0;
        st.snapshot = ChatSnapshot::default();
        self.chat_publish();
    }

    pub(super) fn chat_launch(&mut self) -> Result<()> {
        let Some(factory) = self.deps.chat.clone().filter(|f| f.available()) else {
            self.chat_set_status(ChatStatus::Unavailable);
            return Ok(());
        };
        let config = self.engine_config()?;
        let started_with = (self.prefs.chat.name.clone(), self.prefs.chat.relays.clone());
        self.flow.chat.generation += 1;
        let generation = self.flow.chat.generation;
        let post = self.poster();
        let sink: EventSink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Event {
                generation,
                event,
            }))));
        });
        let engine = factory.start(config, sink).inspect_err(|e| {
            let status = if e.code == ErrorCode::ChtUnavailable {
                ChatStatus::Unavailable
            } else {
                ChatStatus::Failed(e.code)
            };
            self.chat_set_status(status);
        })?;
        self.flow.chat.engine = Some(engine);
        self.flow.chat.started_with = Some(started_with);
        self.chat_set_status(ChatStatus::Starting);
        Ok(())
    }

    /// The data folder, the database key, the avatar pair read back, and the name and relays.
    fn engine_config(&mut self) -> Result<EngineConfig> {
        let dir = self.deps.paths.chat_dir.clone();
        let fs = &*self.deps.fs;
        let key = fs
            .create_dir_all(&dir, 0o700)
            .map_err(Error::from)
            .and_then(|()| crate::chat::key::load_or_create(fs, &dir, &crate::chat::key::os_random))
            .inspect_err(|_| self.chat_set_status(ChatStatus::Failed(ErrorCode::ChtDatabase)))?;
        self.flow.chat.paper = self
            .deps
            .fs
            .read(&dir.join(PAPER_FILE), 8)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.trim().parse::<u8>().ok())
            .filter(|p| *p < 10);
        let p = &self.prefs.chat;
        let name = match p.name.trim() {
            "" => "Player".to_owned(),
            n => n.to_owned(),
        };
        Ok(EngineConfig {
            dir,
            key,
            name,
            relays: p.relays.clone(),
        })
    }

    pub(super) fn chat_set_status(&mut self, status: ChatStatus) {
        if self.flow.chat.snapshot.status != status {
            self.flow.chat.snapshot.status = status;
            self.chat_publish();
        }
    }

    pub(super) fn chat_publish(&mut self) {
        // Until a running engine reports its own, the features are the build's: Settings and
        // the page show the same controls whether Chat is on or off.
        if self.flow.chat.engine.is_none() {
            let features = self
                .deps
                .chat
                .as_ref()
                .filter(|f| f.available())
                .map(|f| f.features(&self.prefs.chat.relays))
                .unwrap_or_default();
            self.flow.chat.snapshot.features = features;
        }
        // Calls need call media (ADR-0035).
        self.flow.chat.snapshot.features.calls &= self.deps.media.is_some();
        // Streams need media that can stream (ADR-0036).
        self.flow.chat.snapshot.features.share &= self
            .deps
            .media
            .as_ref()
            .is_some_and(|m| m.streams().is_some_and(|s| s.can_share()));
        self.hub
            .chat
            .store(Arc::new(self.flow.chat.snapshot.clone()));
        self.emit(Event::ChatChanged);
        self.overlay_sync();
    }

    pub(super) fn chat_request(&mut self, request: Request) {
        match &self.flow.chat.engine {
            Some(e) => e.request(request),
            None => tracing::info!(?request, "chat: a request while the engine is not running"),
        }
    }
}

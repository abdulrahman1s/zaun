//! Chat (ADR-0033): runs the chat engine while `chat.enabled` is on, keeps the
//! Chat snapshot, and turns the page's commands into engine requests.
//!
//! - **Start and stop** follow the preference: on → the data folder (0700) and the
//!   database key are made if needed and the engine starts; off → it is stopped (the
//!   helper closes its database) and the snapshot is emptied. Changing the relays restarts it.
//! - **Generations.** Every engine start has a number; events of an engine already
//!   replaced or stopped are dropped, so a late `Exited` never restarts anything.
//! - **Restarts.** An engine that exits while Chat is on is started again after 1, 2, 4,
//!   8 … 60 s; after [`MAX_FAILURES`] exits without a successful start in between, Chat
//!   stops with `EP-CHT-002` (the status says so) until it is turned off and on.
//! - **Snapshot.** The list comes from the engine (`ChatEvent::Chats`); new messages
//!   update the previews and unread counts at once and are announced as notices, so an
//!   open conversation appends them without asking again.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use super::super::app_service::{AppService, Internal};
use super::super::command::ChatCommand;
use super::super::event::{ChatPage, Event, Notice, QueryResult};
use super::FlowDone;
use crate::chat::{ChatEvent, Engine, EngineConfig, Request, RequestKind};
use crate::error::{Error, ErrorCode};
use crate::ids::QueryId;
use crate::model::{ChatMessage, ChatPreview, ChatSnapshot, ChatStatus, MessageAuthor};

/// Exits in a row (without a successful start between) before Chat gives up.
pub const MAX_FAILURES: u32 = 5;
/// The longest wait before a restart.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Messages per page of a conversation.
pub const PAGE: u32 = 50;

/// Completions that re-enter the loop.
pub(crate) enum ChatDone {
    /// The engine of `generation` reported.
    Event {
        /// Which start.
        generation: u64,
        /// What.
        event: ChatEvent,
    },
    /// The wait before restarting the engine of `generation` is over.
    Restart {
        /// Which start failed.
        generation: u64,
    },
}

/// The Chat part of the service's state.
#[derive(Default)]
pub(crate) struct ChatState {
    engine: Option<Box<dyn Engine>>,
    generation: u64,
    /// What the engine was started with: (name, relays).
    started_with: Option<(String, Vec<String>)>,
    failures: u32,
    snapshot: ChatSnapshot,
    next_tag: u64,
    /// Pages asked for, by tag; the engine answers in order.
    pages: BTreeMap<u64, QueryId>,
}

/// The wait before restart number `failures` (1 → 1 s, 2 → 2 s, … at most [`MAX_BACKOFF`]).
pub fn backoff(failures: u32) -> Duration {
    let secs = 1u64 << failures.saturating_sub(1).min(6);
    Duration::from_secs(secs).min(MAX_BACKOFF)
}

impl AppService {
    /// Start-up: run the engine when Chat is on.
    pub(crate) fn chat_start(&mut self) {
        self.chat_preference_changed();
    }

    /// The preferences changed: start, stop or restart the engine to match.
    pub(crate) fn chat_preference_changed(&mut self) {
        let p = &self.prefs.chat;
        let want = p.enabled.then(|| (p.name.clone(), p.relays.clone()));
        let st = &self.flow.chat;
        match (&want, &st.started_with) {
            (None, None) => {
                if st.snapshot.status != ChatStatus::Off {
                    self.chat_stop();
                }
            }
            (None, Some(_)) => self.chat_stop(),
            (Some(w), Some(s)) if w.1 == s.1 => {
                // Same relays; a new name for a first start does not matter once started.
            }
            (Some(_), Some(_)) => {
                self.chat_stop();
                self.flow.chat.failures = 0;
                self.chat_launch();
            }
            (Some(_), None) => {
                if !matches!(st.snapshot.status, ChatStatus::Failed(_)) {
                    self.chat_launch();
                }
            }
        }
    }

    /// The service is ending: let the engine close its database before the process goes
    /// (the helper would otherwise be killed with the service thread).
    pub(crate) fn chat_shutdown(&mut self) {
        if let Some(mut e) = self.flow.chat.engine.take() {
            self.flow.chat.generation += 1;
            e.shutdown();
        }
    }

    fn chat_stop(&mut self) {
        self.chat_fail_pages(ErrorCode::ChtUnavailable);
        let st = &mut self.flow.chat;
        st.engine = None;
        st.started_with = None;
        st.generation += 1;
        st.failures = 0;
        st.snapshot = ChatSnapshot::default();
        self.chat_publish();
    }

    fn chat_launch(&mut self) {
        let Some(factory) = self.deps.chat.clone() else {
            self.chat_set_status(ChatStatus::Unavailable);
            return;
        };
        if !factory.available() {
            self.chat_set_status(ChatStatus::Unavailable);
            return;
        }
        let dir = self.deps.paths.chat_dir.clone();
        let key = match self
            .deps
            .fs
            .create_dir_all(&dir, 0o700)
            .map_err(Error::from)
            .and_then(|()| {
                crate::chat::key::load_or_create(&*self.deps.fs, &dir, &crate::chat::key::os_random)
            }) {
            Ok(k) => k,
            Err(e) => {
                self.chat_set_status(ChatStatus::Failed(ErrorCode::ChtDatabase));
                return self.fail(e);
            }
        };
        let p = &self.prefs.chat;
        let name = if p.name.trim().is_empty() {
            "Player".to_owned()
        } else {
            p.name.trim().to_owned()
        };
        let config = EngineConfig {
            dir,
            key,
            name,
            relays: p.relays.clone(),
        };
        let started_with = (p.name.clone(), p.relays.clone());
        self.flow.chat.generation += 1;
        let generation = self.flow.chat.generation;
        let post = self.poster();
        let sink: crate::chat::EventSink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Event {
                generation,
                event,
            }))));
        });
        match factory.start(config, sink) {
            Ok(engine) => {
                self.flow.chat.engine = Some(engine);
                self.flow.chat.started_with = Some(started_with);
                self.chat_set_status(ChatStatus::Starting);
            }
            Err(e) => {
                let status = if e.code == ErrorCode::ChtUnavailable {
                    ChatStatus::Unavailable
                } else {
                    ChatStatus::Failed(e.code)
                };
                self.chat_set_status(status);
                self.fail(e);
            }
        }
    }

    fn chat_set_status(&mut self, status: ChatStatus) {
        if self.flow.chat.snapshot.status != status {
            self.flow.chat.snapshot.status = status;
            self.chat_publish();
        }
    }

    fn chat_publish(&mut self) {
        self.hub
            .chat
            .store(Arc::new(self.flow.chat.snapshot.clone()));
        self.emit(Event::ChatChanged);
    }

    fn chat_request(&mut self, request: Request) {
        match &self.flow.chat.engine {
            Some(e) => e.request(request),
            None => tracing::info!(?request, "chat: a request while the engine is not running"),
        }
    }

    /// A Chat job finished or the engine reported.
    pub(crate) fn on_chat_done(&mut self, done: ChatDone) {
        match done {
            ChatDone::Event { generation, event } => {
                if generation == self.flow.chat.generation {
                    self.on_chat_event(event);
                }
            }
            ChatDone::Restart { generation } => {
                let st = &self.flow.chat;
                if generation == st.generation && st.engine.is_none() && self.prefs.chat.enabled {
                    self.chat_launch();
                }
            }
        }
    }

    fn on_chat_event(&mut self, event: ChatEvent) {
        let snap = &mut self.flow.chat.snapshot;
        match event {
            ChatEvent::Ready(profile) => {
                self.flow.chat.failures = 0;
                snap.profile = Some(profile);
                snap.status = ChatStatus::Ready;
            }
            ChatEvent::Profile(profile) => snap.profile = Some(profile),
            ChatEvent::Chats {
                friends,
                groups,
                requests,
                chats,
            } => {
                snap.friends = friends;
                snap.groups = groups;
                snap.requests = requests;
                snap.chats = chats;
            }
            ChatEvent::Invite(link) => snap.invite = Some(link),
            ChatEvent::Address(a) => snap.address = a,
            ChatEvent::Messages(messages) => {
                let unknown = messages.iter().any(|m| !preview_new(snap, m));
                if unknown {
                    self.chat_request(Request::Refresh);
                }
                self.emit(Event::Notice(Notice::ChatMessages(messages)));
            }
            ChatEvent::Updated(messages) => {
                for m in &messages {
                    if let Some(p) = snap.chats.iter_mut().find(|p| p.chat == m.chat)
                        && p.last.as_ref().is_some_and(|l| l.id == m.id)
                    {
                        p.last = Some(m.clone());
                    }
                }
                self.emit(Event::Notice(Notice::ChatMessagesUpdated(messages)));
            }
            ChatEvent::History {
                tag,
                chat,
                messages,
            } => {
                if let Some(id) = self.flow.chat.pages.remove(&tag) {
                    self.emit(Event::QueryResult {
                        id,
                        result: QueryResult::Chat(ChatPage { chat, messages }),
                    });
                }
                return;
            }
            ChatEvent::Failed {
                request,
                code,
                detail,
            } => return self.on_chat_failed(request, code, detail),
            ChatEvent::Exited { status, started } => {
                return self.on_chat_exited(status, started);
            }
        }
        self.chat_publish();
    }

    fn on_chat_failed(&mut self, request: RequestKind, code: ErrorCode, detail: String) {
        tracing::warn!(
            ?request,
            code = code.as_str(),
            detail,
            "chat: request failed"
        );
        if request == RequestKind::Start {
            // Not something a restart fixes (no engine, a database that does not open, a
            // name or relays the engine refuses): stop and say why. Turning Chat off and
            // on again retries.
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
        if request == RequestKind::History {
            // The engine answers pages in order; the oldest waiting one failed.
            if let Some((_, id)) = self.flow.chat.pages.pop_first() {
                let user = self.describe(&Error::new(code).with("detail", &detail), None);
                self.emit(Event::QueryResult {
                    id,
                    result: QueryResult::Failed(Box::new(user)),
                });
            }
            return;
        }
        self.fail(Error::new(code).with("detail", detail));
    }

    /// Answer every page still waiting with a failure (the engine stopped).
    fn chat_fail_pages(&mut self, code: ErrorCode) {
        let pages = std::mem::take(&mut self.flow.chat.pages);
        for id in pages.into_values() {
            let user = self.describe(&Error::new(code), None);
            self.emit(Event::QueryResult {
                id,
                result: QueryResult::Failed(Box::new(user)),
            });
        }
    }

    fn on_chat_exited(&mut self, status: Option<i32>, started: bool) {
        tracing::warn!(?status, started, "chat: the chat engine stopped");
        self.chat_fail_pages(ErrorCode::ChtEngineFailing);
        let st = &mut self.flow.chat;
        st.engine = None;
        st.started_with = None;
        if !self.prefs.chat.enabled
            || matches!(
                st.snapshot.status,
                ChatStatus::Unavailable | ChatStatus::Failed(_)
            )
        {
            return;
        }
        st.failures += 1;
        if st.failures >= MAX_FAILURES {
            self.chat_set_status(ChatStatus::Failed(ErrorCode::ChtEngineFailing));
            return self.fail(Error::new(ErrorCode::ChtEngineFailing));
        }
        self.chat_set_status(ChatStatus::Starting);
        let wait = backoff(self.flow.chat.failures);
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

    // ---- commands ------------------------------------------------------------------------

    pub(crate) fn chat_command(&mut self, c: ChatCommand) {
        match c {
            ChatCommand::SetEnabled { on, name } => {
                let mut patch = crate::config::PreferencePatch::new().set("chat.enabled", on);
                if let Some(n) = name {
                    patch = patch.set("chat.name", n.trim().to_owned());
                }
                if on {
                    // Turning on again after a failure is the way to retry.
                    if matches!(self.flow.chat.snapshot.status, ChatStatus::Failed(_)) {
                        self.flow.chat.snapshot.status = ChatStatus::Off;
                        self.flow.chat.failures = 0;
                    }
                }
                self.handle(super::super::command::Command::SetPreference(patch));
                if on && self.flow.chat.started_with.is_none() {
                    self.chat_preference_changed();
                }
            }
            ChatCommand::SetName(name) => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let patch = crate::config::PreferencePatch::new().set("chat.name", name.clone());
                self.handle(super::super::command::Command::SetPreference(patch));
                self.chat_request(Request::SetProfile(crate::model::ChatProfile { name }));
            }
            ChatCommand::CreateInvite => self.chat_request(Request::CreateInvite),
            ChatCommand::Connect(link) => self.chat_request(Request::Connect(link)),
            ChatCommand::Accept(id) => self.chat_request(Request::Accept(id)),
            ChatCommand::Reject(id) => self.chat_request(Request::Reject(id)),
            ChatCommand::CreateAddress => self.chat_request(Request::CreateAddress),
            ChatCommand::DeleteAddress => self.chat_request(Request::DeleteAddress),
            ChatCommand::Send { chat, text } => {
                if !text.trim().is_empty() {
                    self.chat_request(Request::Send { chat, text });
                }
            }
            ChatCommand::MarkRead(chat) => {
                if let Some(p) = self
                    .flow
                    .chat
                    .snapshot
                    .chats
                    .iter_mut()
                    .find(|p| p.chat == chat && p.unread > 0)
                {
                    p.unread = 0;
                    self.chat_publish();
                    self.chat_request(Request::MarkRead(chat));
                }
            }
            ChatCommand::LoadChat {
                chat,
                before,
                reply_to,
            } => {
                if self.flow.chat.engine.is_none() {
                    let user = self.describe(&Error::new(ErrorCode::ChtUnavailable), None);
                    return self.emit(Event::QueryResult {
                        id: reply_to,
                        result: QueryResult::Failed(Box::new(user)),
                    });
                }
                let st = &mut self.flow.chat;
                st.next_tag += 1;
                let tag = st.next_tag;
                st.pages.insert(tag, reply_to);
                self.chat_request(Request::History {
                    chat,
                    before,
                    count: PAGE,
                    tag,
                });
            }
            ChatCommand::NewGroup(name) => {
                let name = name.trim().to_owned();
                if !name.is_empty() {
                    self.chat_request(Request::NewGroup(name));
                }
            }
            ChatCommand::AddMember { group, friend } => {
                self.chat_request(Request::AddMember { group, friend });
            }
            ChatCommand::JoinGroup(g) => self.chat_request(Request::JoinGroup(g)),
            ChatCommand::LeaveGroup(g) => self.chat_request(Request::LeaveGroup(g)),
            ChatCommand::RemoveFriend(c) => self.chat_request(Request::RemoveFriend(c)),
        }
    }
}

/// Put a new message into its conversation's preview (moving it to the top). `false` when
/// the conversation is not in the list yet.
fn preview_new(snap: &mut ChatSnapshot, m: &ChatMessage) -> bool {
    let Some(i) = snap.chats.iter().position(|p| p.chat == m.chat) else {
        return false;
    };
    let mut p: ChatPreview = snap.chats.remove(i);
    if p.last.as_ref().is_none_or(|l| l.id <= m.id) {
        p.last = Some(m.clone());
    }
    if m.unread && m.author != MessageAuthor::Me {
        p.unread += 1;
    }
    snap.chats.insert(0, p);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restarts_wait_longer_each_time_up_to_a_minute() {
        let waits: Vec<u64> = (1..=9).map(|n| backoff(n).as_secs()).collect();
        assert_eq!(waits, vec![1, 2, 4, 8, 16, 32, 60, 60, 60]);
    }
}

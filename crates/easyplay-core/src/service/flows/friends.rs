//! Friends (ADR-0033): runs the chat engine while `friends.enabled` is on, keeps the
//! Friends snapshot, and turns the page's commands into engine requests.
//!
//! - **Start and stop** follow the preference: on → the data folder (0700) and the
//!   database key are made if needed and the engine starts; off → it is stopped (the
//!   helper closes its database) and the snapshot is emptied. Changing the relays restarts it.
//! - **Generations.** Every engine start has a number; events of an engine already
//!   replaced or stopped are dropped, so a late `Exited` never restarts anything.
//! - **Restarts.** An engine that exits while Friends is on is started again after 1, 2, 4,
//!   8 … 60 s; after [`MAX_FAILURES`] exits without a successful start in between, Friends
//!   stops with `EP-FRN-002` (the status says so) until it is turned off and on.
//! - **Snapshot.** The list comes from the engine (`FriendsEvent::Chats`); new messages
//!   update the previews and unread counts at once and are announced as notices, so an
//!   open conversation appends them without asking again.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use super::super::app_service::{AppService, Internal};
use super::super::command::FriendsCommand;
use super::super::event::{ChatPage, Event, Notice, QueryResult};
use super::FlowDone;
use crate::error::{Error, ErrorCode};
use crate::friends::{Engine, EngineConfig, FriendsEvent, Request, RequestKind};
use crate::ids::QueryId;
use crate::model::{ChatPreview, FriendMessage, FriendsSnapshot, FriendsStatus, MessageAuthor};

/// Exits in a row (without a successful start between) before Friends gives up.
pub const MAX_FAILURES: u32 = 5;
/// The longest wait before a restart.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Messages per page of a conversation.
pub const PAGE: u32 = 50;

/// Completions that re-enter the loop.
pub(crate) enum FriendsDone {
    /// The engine of `generation` reported.
    Event {
        /// Which start.
        generation: u64,
        /// What.
        event: FriendsEvent,
    },
    /// The wait before restarting the engine of `generation` is over.
    Restart {
        /// Which start failed.
        generation: u64,
    },
}

/// The Friends part of the service's state.
#[derive(Default)]
pub(crate) struct FriendsState {
    engine: Option<Box<dyn Engine>>,
    generation: u64,
    /// What the engine was started with: (name, relays).
    started_with: Option<(String, Vec<String>)>,
    failures: u32,
    snapshot: FriendsSnapshot,
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
    /// Start-up: run the engine when Friends is on.
    pub(crate) fn friends_start(&mut self) {
        self.friends_preference_changed();
    }

    /// The preferences changed: start, stop or restart the engine to match.
    pub(crate) fn friends_preference_changed(&mut self) {
        let p = &self.prefs.friends;
        let want = p.enabled.then(|| (p.name.clone(), p.relays.clone()));
        let st = &self.flow.friends;
        match (&want, &st.started_with) {
            (None, None) => {
                if st.snapshot.status != FriendsStatus::Off {
                    self.friends_stop();
                }
            }
            (None, Some(_)) => self.friends_stop(),
            (Some(w), Some(s)) if w.1 == s.1 => {
                // Same relays; a new name for a first start does not matter once started.
            }
            (Some(_), Some(_)) => {
                self.friends_stop();
                self.flow.friends.failures = 0;
                self.friends_launch();
            }
            (Some(_), None) => {
                if !matches!(st.snapshot.status, FriendsStatus::Failed(_)) {
                    self.friends_launch();
                }
            }
        }
    }

    /// The service is ending: let the engine close its database before the process goes
    /// (the helper would otherwise be killed with the service thread).
    pub(crate) fn friends_shutdown(&mut self) {
        if let Some(mut e) = self.flow.friends.engine.take() {
            self.flow.friends.generation += 1;
            e.shutdown();
        }
    }

    fn friends_stop(&mut self) {
        self.friends_fail_pages(ErrorCode::FrnUnavailable);
        let st = &mut self.flow.friends;
        st.engine = None;
        st.started_with = None;
        st.generation += 1;
        st.failures = 0;
        st.snapshot = FriendsSnapshot::default();
        self.friends_publish();
    }

    fn friends_launch(&mut self) {
        let Some(factory) = self.deps.friends.clone() else {
            self.friends_set_status(FriendsStatus::Unavailable);
            return;
        };
        if !factory.available() {
            self.friends_set_status(FriendsStatus::Unavailable);
            return;
        }
        let dir = self.deps.paths.friends_dir.clone();
        let key = match self
            .deps
            .fs
            .create_dir_all(&dir, 0o700)
            .map_err(Error::from)
            .and_then(|()| {
                crate::friends::key::load_or_create(
                    &*self.deps.fs,
                    &dir,
                    &crate::friends::key::os_random,
                )
            }) {
            Ok(k) => k,
            Err(e) => {
                self.friends_set_status(FriendsStatus::Failed(ErrorCode::FrnDatabase));
                return self.fail(e);
            }
        };
        let p = &self.prefs.friends;
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
        self.flow.friends.generation += 1;
        let generation = self.flow.friends.generation;
        let post = self.poster();
        let sink: crate::friends::EventSink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Friends(
                FriendsDone::Event { generation, event },
            ))));
        });
        match factory.start(config, sink) {
            Ok(engine) => {
                self.flow.friends.engine = Some(engine);
                self.flow.friends.started_with = Some(started_with);
                self.friends_set_status(FriendsStatus::Starting);
            }
            Err(e) => {
                let status = if e.code == ErrorCode::FrnUnavailable {
                    FriendsStatus::Unavailable
                } else {
                    FriendsStatus::Failed(e.code)
                };
                self.friends_set_status(status);
                self.fail(e);
            }
        }
    }

    fn friends_set_status(&mut self, status: FriendsStatus) {
        if self.flow.friends.snapshot.status != status {
            self.flow.friends.snapshot.status = status;
            self.friends_publish();
        }
    }

    fn friends_publish(&mut self) {
        self.hub
            .friends
            .store(Arc::new(self.flow.friends.snapshot.clone()));
        self.emit(Event::FriendsChanged);
    }

    fn friends_request(&mut self, request: Request) {
        match &self.flow.friends.engine {
            Some(e) => e.request(request),
            None => tracing::info!(
                ?request,
                "friends: a request while the engine is not running"
            ),
        }
    }

    /// A Friends job finished or the engine reported.
    pub(crate) fn on_friends_done(&mut self, done: FriendsDone) {
        match done {
            FriendsDone::Event { generation, event } => {
                if generation == self.flow.friends.generation {
                    self.on_friends_event(event);
                }
            }
            FriendsDone::Restart { generation } => {
                let st = &self.flow.friends;
                if generation == st.generation && st.engine.is_none() && self.prefs.friends.enabled
                {
                    self.friends_launch();
                }
            }
        }
    }

    fn on_friends_event(&mut self, event: FriendsEvent) {
        let snap = &mut self.flow.friends.snapshot;
        match event {
            FriendsEvent::Ready(profile) => {
                self.flow.friends.failures = 0;
                snap.profile = Some(profile);
                snap.status = FriendsStatus::Ready;
            }
            FriendsEvent::Profile(profile) => snap.profile = Some(profile),
            FriendsEvent::Chats {
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
            FriendsEvent::Invite(link) => snap.invite = Some(link),
            FriendsEvent::Address(a) => snap.address = a,
            FriendsEvent::Messages(messages) => {
                let unknown = messages.iter().any(|m| !preview_new(snap, m));
                if unknown {
                    self.friends_request(Request::Refresh);
                }
                self.emit(Event::Notice(Notice::FriendMessages(messages)));
            }
            FriendsEvent::Updated(messages) => {
                for m in &messages {
                    if let Some(p) = snap.chats.iter_mut().find(|p| p.chat == m.chat)
                        && p.last.as_ref().is_some_and(|l| l.id == m.id)
                    {
                        p.last = Some(m.clone());
                    }
                }
                self.emit(Event::Notice(Notice::FriendMessagesUpdated(messages)));
            }
            FriendsEvent::History {
                tag,
                chat,
                messages,
            } => {
                if let Some(id) = self.flow.friends.pages.remove(&tag) {
                    self.emit(Event::QueryResult {
                        id,
                        result: QueryResult::Chat(ChatPage { chat, messages }),
                    });
                }
                return;
            }
            FriendsEvent::Failed {
                request,
                code,
                detail,
            } => return self.on_friends_failed(request, code, detail),
            FriendsEvent::Exited { status, started } => {
                return self.on_friends_exited(status, started);
            }
        }
        self.friends_publish();
    }

    fn on_friends_failed(&mut self, request: RequestKind, code: ErrorCode, detail: String) {
        tracing::warn!(
            ?request,
            code = code.as_str(),
            detail,
            "friends: request failed"
        );
        if request == RequestKind::Start {
            // Not something a restart fixes (no engine, a database that does not open, a
            // name or relays the engine refuses): stop and say why. Turning Friends off and
            // on again retries.
            let st = &mut self.flow.friends;
            st.engine = None;
            st.started_with = None;
            st.generation += 1;
            let status = if code == ErrorCode::FrnUnavailable {
                FriendsStatus::Unavailable
            } else {
                FriendsStatus::Failed(code)
            };
            self.friends_set_status(status);
            self.friends_fail_pages(code);
        }
        if request == RequestKind::History {
            // The engine answers pages in order; the oldest waiting one failed.
            if let Some((_, id)) = self.flow.friends.pages.pop_first() {
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
    fn friends_fail_pages(&mut self, code: ErrorCode) {
        let pages = std::mem::take(&mut self.flow.friends.pages);
        for id in pages.into_values() {
            let user = self.describe(&Error::new(code), None);
            self.emit(Event::QueryResult {
                id,
                result: QueryResult::Failed(Box::new(user)),
            });
        }
    }

    fn on_friends_exited(&mut self, status: Option<i32>, started: bool) {
        tracing::warn!(?status, started, "friends: the chat engine stopped");
        self.friends_fail_pages(ErrorCode::FrnEngineFailing);
        let st = &mut self.flow.friends;
        st.engine = None;
        st.started_with = None;
        if !self.prefs.friends.enabled
            || matches!(
                st.snapshot.status,
                FriendsStatus::Unavailable | FriendsStatus::Failed(_)
            )
        {
            return;
        }
        st.failures += 1;
        if st.failures >= MAX_FAILURES {
            self.friends_set_status(FriendsStatus::Failed(ErrorCode::FrnEngineFailing));
            return self.fail(Error::new(ErrorCode::FrnEngineFailing));
        }
        self.friends_set_status(FriendsStatus::Starting);
        let wait = backoff(self.flow.friends.failures);
        let generation = self.flow.friends.generation;
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("friends-restart".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Friends(
                    FriendsDone::Restart { generation },
                ))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "friends: restart timer not started");
        }
    }

    // ---- commands ------------------------------------------------------------------------

    pub(crate) fn friends_command(&mut self, c: FriendsCommand) {
        match c {
            FriendsCommand::SetEnabled { on, name } => {
                let mut patch = crate::config::PreferencePatch::new().set("friends.enabled", on);
                if let Some(n) = name {
                    patch = patch.set("friends.name", n.trim().to_owned());
                }
                if on {
                    // Turning on again after a failure is the way to retry.
                    if matches!(self.flow.friends.snapshot.status, FriendsStatus::Failed(_)) {
                        self.flow.friends.snapshot.status = FriendsStatus::Off;
                        self.flow.friends.failures = 0;
                    }
                }
                self.handle(super::super::command::Command::SetPreference(patch));
                if on && self.flow.friends.started_with.is_none() {
                    self.friends_preference_changed();
                }
            }
            FriendsCommand::SetName(name) => {
                let name = name.trim().to_owned();
                if name.is_empty() {
                    return;
                }
                let patch = crate::config::PreferencePatch::new().set("friends.name", name.clone());
                self.handle(super::super::command::Command::SetPreference(patch));
                self.friends_request(Request::SetProfile(crate::model::FriendProfile { name }));
            }
            FriendsCommand::CreateInvite => self.friends_request(Request::CreateInvite),
            FriendsCommand::Connect(link) => self.friends_request(Request::Connect(link)),
            FriendsCommand::Accept(id) => self.friends_request(Request::Accept(id)),
            FriendsCommand::Reject(id) => self.friends_request(Request::Reject(id)),
            FriendsCommand::CreateAddress => self.friends_request(Request::CreateAddress),
            FriendsCommand::DeleteAddress => self.friends_request(Request::DeleteAddress),
            FriendsCommand::Send { chat, text } => {
                if !text.trim().is_empty() {
                    self.friends_request(Request::Send { chat, text });
                }
            }
            FriendsCommand::MarkRead(chat) => {
                if let Some(p) = self
                    .flow
                    .friends
                    .snapshot
                    .chats
                    .iter_mut()
                    .find(|p| p.chat == chat && p.unread > 0)
                {
                    p.unread = 0;
                    self.friends_publish();
                    self.friends_request(Request::MarkRead(chat));
                }
            }
            FriendsCommand::LoadChat {
                chat,
                before,
                reply_to,
            } => {
                if self.flow.friends.engine.is_none() {
                    let user = self.describe(&Error::new(ErrorCode::FrnUnavailable), None);
                    return self.emit(Event::QueryResult {
                        id: reply_to,
                        result: QueryResult::Failed(Box::new(user)),
                    });
                }
                let st = &mut self.flow.friends;
                st.next_tag += 1;
                let tag = st.next_tag;
                st.pages.insert(tag, reply_to);
                self.friends_request(Request::History {
                    chat,
                    before,
                    count: PAGE,
                    tag,
                });
            }
            FriendsCommand::NewGroup(name) => {
                let name = name.trim().to_owned();
                if !name.is_empty() {
                    self.friends_request(Request::NewGroup(name));
                }
            }
            FriendsCommand::AddMember { group, friend } => {
                self.friends_request(Request::AddMember { group, friend });
            }
            FriendsCommand::JoinGroup(g) => self.friends_request(Request::JoinGroup(g)),
            FriendsCommand::LeaveGroup(g) => self.friends_request(Request::LeaveGroup(g)),
            FriendsCommand::RemoveFriend(c) => self.friends_request(Request::RemoveFriend(c)),
        }
    }
}

/// Put a new message into its conversation's preview (moving it to the top). `false` when
/// the conversation is not in the list yet.
fn preview_new(snap: &mut FriendsSnapshot, m: &FriendMessage) -> bool {
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

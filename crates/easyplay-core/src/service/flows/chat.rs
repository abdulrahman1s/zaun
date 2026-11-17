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
use crate::model::{
    ChatMessage, ChatPreview, ChatProfile, ChatRef, ChatSnapshot, ChatStatus, MessageAuthor,
};

/// Exits in a row (without a successful start between) before Chat gives up.
pub const MAX_FAILURES: u32 = 5;
/// The longest wait before a restart.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Messages per page of a conversation.
pub const PAGE: u32 = 50;
/// The user's avatar pair, kept on this computer (`<data>/chat/paper`): the engine's profile
/// has no place for it.
pub const PAPER_FILE: &str = "paper";
/// Sent pictures are copied here inside the Chat folder (the engine's sandbox sees no other).
pub const OUTBOX_DIR: &str = "files/sent";

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
    /// A file to send was copied into the Chat folder (and its preview made).
    Prepared {
        /// For which start.
        generation: u64,
        /// The request to send, or why not.
        result: Result<Request, Error>,
    },
}

/// A file to send: a file on this computer, or a picture from the clipboard.
enum Outgoing {
    Path(std::path::PathBuf),
    /// Already copied into place.
    Ready(std::path::PathBuf),
    Bytes {
        bytes: Vec<u8>,
        name: String,
    },
}

/// The longest side of a preview picture, in pixels (a JPEG of about 1 to 2 KB).
pub const PREVIEW_SIDE: u32 = 64;
/// Pictures larger than this are sent without a preview (not decoded).
pub const PREVIEW_MAX_SOURCE: u64 = 64 * 1024 * 1024;

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
    /// Numbers for copies of sent files.
    next_file: u64,
    /// The user's avatar pair (`PAPER_FILE`).
    paper: Option<u8>,
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
        self.flow.chat.paper = self
            .deps
            .fs
            .read(&dir.join(PAPER_FILE), 8)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .and_then(|s| s.trim().parse::<u8>().ok())
            .filter(|p| *p < 10);
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
            ChatDone::Prepared { generation, result } => {
                if generation != self.flow.chat.generation {
                    return;
                }
                match result {
                    Ok(r) => self.chat_request(r),
                    Err(e) => self.fail(e),
                }
            }
        }
    }

    fn on_chat_event(&mut self, event: ChatEvent) {
        let snap = &mut self.flow.chat.snapshot;
        match event {
            ChatEvent::Features {
                features,
                reactions,
            } => {
                snap.features = features;
                snap.reaction_emoji = reactions;
            }
            ChatEvent::Ready(profile) => {
                self.flow.chat.failures = 0;
                snap.profile = Some(local_profile(snap, profile, self.flow.chat.paper));
                snap.status = ChatStatus::Ready;
                // A new engine reports going offline afresh.
                snap.offline = false;
            }
            ChatEvent::Profile(profile) => {
                snap.profile = Some(local_profile(snap, profile, self.flow.chat.paper));
            }
            ChatEvent::Chats {
                friends,
                mut groups,
                requests,
                chats,
            } => {
                // Members and invite links are read separately: keep them.
                for g in &mut groups {
                    if let Some(old) = snap.groups.iter().find(|o| o.id == g.id) {
                        g.member_list.clone_from(&old.member_list);
                        g.link.clone_from(&old.link);
                    }
                }
                snap.friends = friends;
                snap.groups = groups;
                snap.requests = requests;
                snap.chats = chats;
            }
            ChatEvent::Code { friend, code } => snap.code = Some((friend, code)),
            ChatEvent::Verified { friend, matched } => {
                if let Some(f) = snap.friends.iter_mut().find(|f| f.id == friend) {
                    f.verified = matched;
                }
                if !matched {
                    self.chat_publish();
                    return self.fail(
                        Error::new(ErrorCode::ChtRequestFailed)
                            .with("detail", "the security codes differ"),
                    );
                }
            }
            ChatEvent::Members { group, members } => {
                if let Some(g) = snap.groups.iter_mut().find(|g| g.id == group) {
                    g.members = u32::try_from(members.len()).ok();
                    g.member_list = members;
                }
            }
            ChatEvent::Offline(offline) => snap.offline = offline,
            ChatEvent::Preview { chat, last } => {
                if let Some(p) = snap.chats.iter_mut().find(|p| p.chat == chat)
                    && p.last.as_ref().is_none_or(|l| l.id < last.id)
                {
                    p.last = Some(last);
                }
            }
            ChatEvent::OpenFile { path, kind } => return self.chat_open_file(&path, kind),
            ChatEvent::GroupLink { group, link } => {
                if let Some(g) = snap.groups.iter_mut().find(|g| g.id == group) {
                    g.link = link;
                }
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
                // The engine's own words stay in the log (above): they name relays and
                // internals, never anything the user can act on.
                let user = self.describe(&Error::new(code), None);
                self.emit(Event::QueryResult {
                    id,
                    result: QueryResult::Failed(Box::new(user)),
                });
            }
            return;
        }
        if request == RequestKind::Start {
            // The Chat page says why (its status); a dialog on top would say it twice.
            return;
        }
        self.fail(Error::new(code));
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
        let wait = self
            .deps
            .chat
            .as_ref()
            .map_or(wait, |f| f.restart_wait(wait));
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

    /// The name (also `chat.name`) and the line about the user go to the engine; the avatar
    /// pair is kept here.
    fn chat_set_profile(&mut self, p: ChatProfile) {
        let name = p.name.trim().to_owned();
        if self.prefs.chat.name != name {
            let patch = crate::config::PreferencePatch::new().set("chat.name", name.clone());
            self.handle(super::super::command::Command::SetPreference(patch));
        }
        let paper = p.paper.filter(|x| *x < 10);
        if paper != self.flow.chat.paper {
            self.flow.chat.paper = paper;
            let file = self.deps.paths.chat_dir.join(PAPER_FILE);
            let written = match paper {
                Some(x) => self
                    .deps
                    .fs
                    .create_dir_all(&self.deps.paths.chat_dir, 0o700)
                    .and_then(|()| {
                        self.deps
                            .fs
                            .atomic_write(&file, x.to_string().as_bytes(), 0o600)
                    }),
                None => self.deps.fs.remove_file(&file).or(Ok(())),
            };
            if let Err(e) = written {
                tracing::warn!(error = %e, "chat: the avatar pair was not saved");
            }
        }
        let snap = &mut self.flow.chat.snapshot;
        if let Some(cur) = &mut snap.profile {
            cur.paper = paper;
            cur.status = p.status;
        }
        self.chat_publish();
        let current = self.flow.chat.snapshot.profile.clone().unwrap_or_default();
        if current.name != name || current.about != p.about.trim() {
            self.chat_request(Request::SetProfile(ChatProfile {
                name,
                about: p.about.trim().to_owned(),
                ..p
            }));
        }
    }

    /// Copy a file into the Chat folder (the engine's sandbox sees nothing else), make a
    /// picture's preview, and send it; on a thread of its own (a large file takes a while).
    /// Refused when files are off or it is larger than `chat::MAX_SEND`.
    fn chat_send_file(&mut self, chat: ChatRef, src: Outgoing) {
        if !self.flow.chat.snapshot.features.files || self.flow.chat.engine.is_none() {
            return self
                .fail(Error::new(ErrorCode::ChtRequestFailed).with("detail", "files are off"));
        }
        let (name, size) = match &src {
            Outgoing::Path(p) => match self.deps.fs.metadata_follow(p) {
                Ok(m) if m.is_dir() => {
                    return self
                        .fail(Error::new(ErrorCode::ChtRequestFailed).with("detail", "a folder"));
                }
                Ok(m) => (
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    m.size,
                ),
                Err(e) => return self.fail(Error::from(e)),
            },
            Outgoing::Bytes { bytes, name } => (name.clone(), bytes.len() as u64),
            Outgoing::Ready(_) => return,
        };
        if size > crate::chat::MAX_SEND {
            return self.fail(
                Error::new(ErrorCode::ChtRequestFailed)
                    .with("detail", "the file is larger than 1 GiB"),
            );
        }
        let name = safe_name(&name);
        let kind = file_kind(std::path::Path::new(&name));
        self.flow.chat.next_file += 1;
        let stamp = self.deps.clock.now().as_millis();
        // A folder per file keeps its name, which is what the friend sees.
        let dir = self
            .deps
            .paths
            .chat_dir
            .join(OUTBOX_DIR)
            .join(format!("{stamp}-{}", self.flow.chat.next_file));
        // A small file is copied before this returns (a caller may remove its own copy
        // then); a large one on the thread.
        let src = match src {
            Outgoing::Path(p) if size <= crate::chat::MAX_FILE => {
                let dest = dir.join(&name);
                let copied = self
                    .deps
                    .fs
                    .create_dir_all(&dir, 0o700)
                    .and_then(|()| self.deps.fs.copy_file(&p, &dest, 0o600));
                if let Err(e) = copied {
                    return self.fail(Error::from(e));
                }
                Outgoing::Ready(dest)
            }
            other => other,
        };
        let fs = Arc::clone(&self.deps.fs);
        let generation = self.flow.chat.generation;
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("chat-file".into())
            .spawn(move || {
                let result =
                    prepare_file(&*fs, src, &dir, &name, kind, size).map(|(path, preview)| {
                        Request::SendFile {
                            chat,
                            path,
                            kind,
                            preview,
                            duration: None,
                        }
                    });
                post(Internal::Flow(Box::new(FlowDone::Chat(
                    ChatDone::Prepared { generation, result },
                ))));
            });
        if let Err(e) = spawned {
            self.fail(Error::new(ErrorCode::IntTaskFailed).with_source(e));
        }
    }

    /// Open a message's file: pictures and videos with the desktop's default application,
    /// other files shown in their folder (a received file is never run). Only files inside
    /// the Chat folder.
    fn chat_open_file(&mut self, path: &std::path::Path, kind: crate::chat::FileKind) {
        let inside = self
            .deps
            .fs
            .canonicalize(path)
            .ok()
            .zip(self.deps.fs.canonicalize(&self.deps.paths.chat_dir).ok())
            .is_some_and(|(p, d)| p.starts_with(&d) && p != d);
        if !inside {
            tracing::warn!("chat: a file outside the Chat folder was not opened");
            return;
        }
        let Some(desktop) = self.deps.desktop.clone() else {
            return;
        };
        let target = crate::desktop::open_uri::KnownPath::file(path.to_owned());
        let reveal = kind == crate::chat::FileKind::File;
        let spawned = std::thread::Builder::new()
            .name("chat-open".into())
            .spawn(move || {
                let r = if reveal {
                    desktop.opener.reveal(&target)
                } else {
                    desktop.opener.open(&target)
                };
                if let Err(e) = r {
                    tracing::warn!(error = %e, "chat: the file was not opened");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "chat: the opener did not start");
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
                let mut p = self.flow.chat.snapshot.profile.clone().unwrap_or_default();
                p.name = name;
                self.chat_set_profile(p);
            }
            ChatCommand::SetProfile(p) => {
                if p.name.trim().is_empty() {
                    return;
                }
                self.chat_set_profile(p);
            }
            ChatCommand::CreateInvite => self.chat_request(Request::CreateInvite),
            ChatCommand::Connect(link) => self.chat_request(Request::Connect(link)),
            ChatCommand::Accept(id) => self.chat_request(Request::Accept(id)),
            ChatCommand::Reject(id) => self.chat_request(Request::Reject(id)),
            ChatCommand::CreateAddress => self.chat_request(Request::CreateAddress),
            ChatCommand::DeleteAddress => self.chat_request(Request::DeleteAddress),
            ChatCommand::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => {
                if !text.trim().is_empty() {
                    self.chat_request(Request::Send {
                        chat,
                        text,
                        reply_to,
                        mentions,
                    });
                }
            }
            ChatCommand::SendFile { chat, path } => self.chat_send_file(chat, Outgoing::Path(path)),
            ChatCommand::SendClipboard { chat, bytes, kind } => {
                let ext = match kind {
                    crate::service::ClipKind::Png => "png",
                    crate::service::ClipKind::Jpeg => "jpg",
                    crate::service::ClipKind::Gif => "gif",
                };
                let name = format!("pasted-{}.{ext}", self.deps.clock.now().as_millis());
                self.chat_send_file(chat, Outgoing::Bytes { bytes, name });
            }
            ChatCommand::ReceiveFile { chat, item } => {
                self.chat_request(Request::ReceiveFile { chat, item });
            }
            ChatCommand::CancelFile { chat, item } => {
                self.chat_request(Request::CancelFile { chat, item });
            }
            ChatCommand::OpenFile { chat, item } => {
                self.chat_request(Request::OpenFile { chat, item });
            }
            ChatCommand::CreateGroupLink(g) => self.chat_request(Request::CreateGroupLink(g)),
            ChatCommand::DeleteGroupLink(g) => self.chat_request(Request::DeleteGroupLink(g)),
            ChatCommand::Edit { chat, item, text } => {
                if !text.trim().is_empty() {
                    self.chat_request(Request::Edit { chat, item, text });
                }
            }
            ChatCommand::Delete { chat, item } => self.chat_request(Request::Delete { chat, item }),
            ChatCommand::React {
                chat,
                item,
                emoji,
                on,
            } => self.chat_request(Request::React {
                chat,
                item,
                emoji,
                on,
            }),
            ChatCommand::SetMuted { chat, on } => {
                let snap = &mut self.flow.chat.snapshot;
                if let Some(p) = snap.chats.iter_mut().find(|p| p.chat == chat) {
                    p.muted = on;
                }
                match chat {
                    ChatRef::Direct(id) => {
                        if let Some(f) = snap.friends.iter_mut().find(|f| f.id == id) {
                            f.muted = on;
                        }
                    }
                    ChatRef::Group(id) => {
                        if let Some(g) = snap.groups.iter_mut().find(|g| g.id == id) {
                            g.muted = on;
                        }
                    }
                }
                self.chat_publish();
                self.chat_request(Request::SetMuted { chat, on });
            }
            ChatCommand::SetBlocked { friend, on } => {
                let snap = &mut self.flow.chat.snapshot;
                if let Some(f) = snap.friends.iter_mut().find(|f| f.id == friend) {
                    f.blocked = on;
                }
                if on {
                    snap.chats.retain(|p| p.chat != ChatRef::Direct(friend));
                }
                self.chat_publish();
                self.chat_request(Request::SetBlocked { friend, on });
            }
            ChatCommand::SetMemberBlocked { group, member, on } => {
                if let Some(m) = self
                    .flow
                    .chat
                    .snapshot
                    .groups
                    .iter_mut()
                    .find(|g| g.id == group)
                    .and_then(|g| g.member_list.iter_mut().find(|m| m.id == member))
                {
                    m.blocked = on;
                    self.chat_publish();
                }
                self.chat_request(Request::SetMemberBlocked { group, member, on });
            }
            ChatCommand::ShowCode(friend) => {
                if self.flow.chat.snapshot.code.take().is_some() {
                    self.chat_publish();
                }
                self.chat_request(Request::ShowCode(friend));
            }
            ChatCommand::Verify { friend, code } => {
                self.chat_request(Request::Verify { friend, code });
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
                // Opening a group reads its members (mentions, the details panel).
                if let (ChatRef::Group(g), None) = (chat, before) {
                    self.chat_request(Request::Members(g));
                }
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
            // Status, calls, voice messages, sharing and watching have no engine yet:
            // `ChatSnapshot::features` keeps their controls off the page. Opening a link is
            // the GUI's (and the link check's) part.
            _ => tracing::debug!("chat command not carried by this engine yet"),
        }
    }
}

/// The profile the engine reports, with what EasyPlay keeps itself: the avatar pair and the
/// status the user chose.
fn local_profile(snap: &ChatSnapshot, mut p: ChatProfile, paper: Option<u8>) -> ChatProfile {
    p.paper = paper;
    if let Some(old) = &snap.profile {
        p.status = old.status;
    }
    p
}

/// What a file sent from this computer is, by its name.
fn file_kind(path: &std::path::Path) -> crate::chat::FileKind {
    use crate::chat::FileKind;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "gif" => FileKind::Gif,
        "jpg" | "jpeg" | "png" | "webp" | "bmp" => FileKind::Image,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => FileKind::Video,
        _ => FileKind::File,
    }
}

/// A file name for the Chat folder and for the friend: the last component, no control
/// characters, not empty, at most 120 characters.
fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let clean = clean.trim().trim_start_matches('.').to_owned();
    if clean.is_empty() {
        "file".to_owned()
    } else {
        clean
    }
}

/// Put the file to send at `dir/name` (0600, the folder 0700) and make a picture's preview.
fn prepare_file(
    fs: &dyn crate::fsx::Fs,
    src: Outgoing,
    dir: &std::path::Path,
    name: &str,
    kind: crate::chat::FileKind,
    size: u64,
) -> Result<(std::path::PathBuf, Option<Vec<u8>>), Error> {
    let dest = match src {
        Outgoing::Ready(p) => p,
        Outgoing::Path(p) => {
            fs.create_dir_all(dir, 0o700)?;
            let dest = dir.join(name);
            fs.copy_file(&p, &dest, 0o600)?;
            dest
        }
        Outgoing::Bytes { bytes, .. } => {
            fs.create_dir_all(dir, 0o700)?;
            let dest = dir.join(name);
            fs.write_new(&dest, &bytes, 0o600)?;
            dest
        }
    };
    let preview = (kind == crate::chat::FileKind::Image && size <= PREVIEW_MAX_SOURCE)
        .then(|| fs.read(&dest, PREVIEW_MAX_SOURCE).ok())
        .flatten()
        .and_then(|b| preview_jpeg(&b));
    Ok((dest, preview))
}

/// A small JPEG of a picture (its longest side [`PREVIEW_SIDE`]), or `None` when it cannot
/// be read.
pub fn preview_jpeg(bytes: &[u8]) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    let small = img.thumbnail(PREVIEW_SIDE, PREVIEW_SIDE).to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 60)
        .encode_image(&small)
        .ok()?;
    Some(out)
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

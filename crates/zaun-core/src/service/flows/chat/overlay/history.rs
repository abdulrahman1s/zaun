//! The open conversation's messages and the toasts. The Chat snapshot holds no messages, so
//! the overlay asks for the latest page of its conversation (through `pages`, so a failed
//! page is matched as the page's own are) and then follows new and changed messages.
use super::state::PAGES_IN_FLIGHT;
use crate::chat::Request;
use crate::chat::overlay::build;
use crate::chat::overlay::snapshot::{MAX_MESSAGES, MAX_TOASTS};
use crate::model::{ChatMessage, MessageAuthor};
use crate::service::prelude::*;

impl AppService {
    /// The most recent conversation, when the overlay shows none yet.
    pub(super) fn overlay_default_chat(&mut self) {
        if let Some(chat) = self.flow.chat.snapshot.chats.first().map(|p| p.chat) {
            self.overlay_open_chat(chat);
        }
    }

    /// Show `chat`: forget the other conversation's messages and ask for this one's.
    pub(super) fn overlay_open_chat(&mut self, chat: ChatRef) {
        let st = &mut self.flow.overlay;
        st.chat = Some(chat);
        st.messages.clear();
        if self.flow.chat.engine.is_none() {
            return;
        }
        let id = QueryId::generate(&*self.deps.clock);
        let chat_state = &mut self.flow.chat;
        chat_state.next_tag += 1;
        let tag = chat_state.next_tag;
        chat_state.pages.insert(tag, id);
        let st = &mut self.flow.overlay;
        st.history = Some(id);
        st.pages.push_back(id);
        while st.pages.len() > PAGES_IN_FLIGHT {
            st.pages.pop_front();
        }
        #[allow(clippy::cast_possible_truncation)]
        self.chat_request(Request::History {
            chat,
            before: None,
            count: MAX_MESSAGES as u32,
            tag,
        });
        if self.flow.overlay.open {
            self.chat_mark_read(chat);
        }
    }

    /// A page arrived for query `id`: `true` when it was the overlay's (and is taken).
    pub(crate) fn overlay_history(
        &mut self,
        id: QueryId,
        chat: ChatRef,
        messages: &[ChatMessage],
    ) -> bool {
        let st = &mut self.flow.overlay;
        let Some(at) = st.pages.iter().position(|p| *p == id) else {
            return false;
        };
        st.pages.remove(at);
        if st.history != Some(id) {
            // Asked for a conversation the overlay no longer shows.
            return true;
        }
        st.history = None;
        if st.chat == Some(chat) {
            // Messages that arrived while the page was on its way stay after it.
            let newer: Vec<ChatMessage> = st
                .messages
                .drain(..)
                .filter(|m| messages.last().is_none_or(|l| m.id > l.id))
                .collect();
            st.messages.extend(messages.iter().cloned());
            st.messages.extend(newer);
            trim(&mut st.messages);
            self.overlay_push();
        }
        true
    }

    /// New messages: the open conversation's join its list; others' become toasts.
    pub(crate) fn overlay_messages(&mut self, messages: &[ChatMessage]) {
        if self.flow.overlay.overlay.is_none() {
            return;
        }
        let toasts_on = self.prefs.chat.overlay_toasts;
        let st = &mut self.flow.overlay;
        let mut read = false;
        for m in messages {
            let shown = st.chat == Some(m.chat);
            if shown && !st.messages.iter().any(|o| o.id == m.id) {
                st.messages.push_back(m.clone());
                read |= st.open && m.unread;
            }
            let muted = self
                .flow
                .chat
                .snapshot
                .chats
                .iter()
                .any(|p| p.chat == m.chat && p.muted);
            if toasts_on && m.author != MessageAuthor::Me && !muted && !(st.open && shown) {
                st.next_toast += 1;
                st.toasts
                    .push_back(build::toast(&self.flow.chat.snapshot, m, st.next_toast));
                while st.toasts.len() > MAX_TOASTS {
                    st.toasts.pop_front();
                }
            }
        }
        trim(&mut st.messages);
        if read && let Some(chat) = st.chat {
            self.chat_mark_read(chat);
        }
    }

    /// Changed messages (delivery, edits, deletion) replace their copies.
    pub(crate) fn overlay_updated(&mut self, messages: &[ChatMessage]) {
        for m in messages {
            if let Some(old) = self
                .flow
                .overlay
                .messages
                .iter_mut()
                .find(|o| o.id == m.id && o.chat == m.chat)
            {
                old.clone_from(m);
            }
        }
    }
}

/// Keep the latest `MAX_MESSAGES`.
fn trim(messages: &mut std::collections::VecDeque<ChatMessage>) {
    while messages.len() > MAX_MESSAGES {
        messages.pop_front();
    }
}

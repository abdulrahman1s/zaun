//! Messages: send, edit, mark read, and load a page of a conversation (ADR-0033).
use super::state::PAGE;
use crate::chat::Request;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn chat_send(
        &mut self,
        chat: ChatRef,
        text: String,
        reply_to: Option<i64>,
        mentions: Vec<(String, i64)>,
    ) {
        if !text.trim().is_empty() {
            self.chat_request(Request::Send {
                chat,
                text,
                reply_to,
                mentions,
            });
        }
    }

    pub(super) fn chat_edit(&mut self, chat: ChatRef, item: i64, text: String) {
        if !text.trim().is_empty() {
            self.chat_request(Request::Edit { chat, item, text });
        }
    }

    pub(super) fn chat_mark_read(&mut self, chat: ChatRef) {
        let unread = self
            .flow
            .chat
            .snapshot
            .preview_mut(chat)
            .filter(|p| p.unread > 0);
        if let Some(p) = unread {
            p.unread = 0;
            self.chat_publish();
            self.chat_request(Request::MarkRead(chat));
        }
    }

    /// A page of a conversation, answered through the query (also when the engine is off).
    pub(super) fn chat_load(&mut self, chat: ChatRef, before: Option<i64>, reply_to: QueryId) {
        if self.flow.chat.engine.is_none() {
            let user = self.describe(&err!(ChtUnavailable), None);
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
}

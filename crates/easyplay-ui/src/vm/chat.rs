//! The Chat page's view-model (ADR-0033): the open conversation, its loaded messages, the
//! composer drafts. No egui types.

use easyplay_core::model::ChatMessage;

/// Chat page state kept across frames.
#[derive(Debug, Default)]
pub struct ChatVm {}

impl ChatVm {
    /// Messages arrived or changed (`updated`) — from `UiEvent::ChatMessages`.
    pub fn on_messages(&mut self, messages: &[ChatMessage], updated: bool) {
        let _ = (messages, updated);
    }

    /// A query answer: returns `true` when it was one of this page's.
    pub fn on_query(
        &mut self,
        id: easyplay_core::ids::QueryId,
        payload: &crate::bridge::QueryPayload,
    ) -> bool {
        let _ = (id, payload);
        false
    }
}

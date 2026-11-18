//! The hotkey: `easyplay overlay toggle|open|close` reaches here through the instance
//! socket (ADR-0037 §7). Opening shows the panel on the conversation it showed last, or
//! the most recent one, and marks it read.
use crate::chat::overlay::ToOverlay;
use crate::service::command::OverlayAction;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn overlay_action(&mut self, action: OverlayAction) {
        let st = &self.flow.overlay;
        let Some(o) = &st.overlay else {
            tracing::info!(?action, "overlay: not running");
            return;
        };
        let open = match action {
            OverlayAction::Toggle => !st.open,
            OverlayAction::Open => true,
            OverlayAction::Close => false,
        };
        if open == st.open {
            return;
        }
        o.send(if open {
            &ToOverlay::Open
        } else {
            &ToOverlay::Close
        });
        self.flow.overlay.open = open;
        if open {
            if self.flow.overlay.chat.is_none() {
                self.overlay_default_chat();
            }
            if let Some(chat) = self.flow.overlay.chat {
                self.chat_mark_read(chat);
            }
        }
        self.overlay_push();
    }
}

//! The panel's keys (`chat-calls.html` §G KEYS): Esc closes the corner menu, then the
//! popover, then the overlay; Alt+↑ / Alt+↓ change the chat; Ctrl+Shift+M mutes and
//! Ctrl+Shift+D stops hearing the call, as in the window. Typing goes to the composer,
//! which has the keyboard when the panel opens.
use easyplay_core::chat::overlay::FromOverlay;
use egui::{Context, Key, Modifiers};

use crate::model::{Focus, Model};

pub fn handle(ctx: &Context, m: &mut Model) {
    if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if m.corner_menu {
            m.corner_menu = false;
        } else if m.pins {
            m.pins = false;
        } else {
            m.said.push(FromOverlay::Closed);
        }
    }
    let step = ctx.input_mut(|i| {
        if i.consume_key(Modifiers::ALT, Key::ArrowUp) {
            -1
        } else if i.consume_key(Modifiers::ALT, Key::ArrowDown) {
            1
        } else {
            0
        }
    });
    if step != 0 {
        let chats: Vec<_> = m.snap.chats.iter().map(|c| c.chat).collect();
        let at = m
            .snap
            .open
            .as_ref()
            .and_then(|o| chats.iter().position(|c| *c == o.chat))
            .unwrap_or(0);
        let next = at
            .saturating_add_signed(step)
            .min(chats.len().saturating_sub(1));
        if let Some(chat) = chats.get(next).copied().filter(|_| next != at) {
            m.said.push(FromOverlay::OpenConversation { chat });
            m.focus = Some(Focus::Composer);
        }
    }
    let both = Modifiers::CTRL | Modifiers::SHIFT;
    if let Some(call) = m.snap.call.as_ref() {
        if ctx.input_mut(|i| i.consume_key(both, Key::M)) {
            m.said.push(FromOverlay::CallMute { on: !call.muted });
        }
        if ctx.input_mut(|i| i.consume_key(both, Key::D)) {
            m.said.push(FromOverlay::CallDeafen { on: !call.deafened });
        }
    }
}

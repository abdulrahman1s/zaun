//! UI-006: a modal keeps keyboard focus — `Tab` and `Shift+Tab` cycle through its controls and
//! never reach the page behind it or a toast above it, and focus is never lost.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::*;
use egui::{Key, Modifiers};
use zaun_core::ids::PromptId;
use zaun_core::model::{PromptKind, PromptRequest};
use zaun_ui::bridge::{ToastLevel, ToastMsg, UiEvent};
use zaun_ui::route::Route;

fn focus_in_modal(r: &Rig) -> bool {
    let ctx = &r.h.ctx;
    let Some(f) = ctx.memory(|m| m.focused()) else {
        return false;
    };
    let top = ctx.memory(|m| m.top_modal_layer());
    let layer = ctx.read_response(f).map(|x| x.layer_id);
    top.is_some() && layer == top
}

#[test]
fn tab_cycles_inside_a_dialog() {
    let lib = settled_library();
    let g = game_id(&lib, "Lantern Keep");
    let mut r = rig(Route::Library);
    r.h.run();
    r.fake.push_event(UiEvent::Toast(ToastMsg {
        text: "Something happened".into(),
        level: ToastLevel::Info,
        undo: None,
    }));
    r.fake.push_event(UiEvent::Prompt(PromptRequest {
        id: PromptId::from_uuid(uuid::Uuid::from_u128(5)),
        kind: PromptKind::RemoveRunningGame { game: g },
    }));
    r.h.run();
    r.h.run();
    for shift in [false, true] {
        for _ in 0..8 {
            let m = if shift {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            };
            r.h.key_press_modifiers(m, Key::Tab);
            r.h.run();
            let ctx = &r.h.ctx;
            let f = ctx.memory(|m| m.focused());
            let dbg = (
                f,
                f.and_then(|f| ctx.read_response(f))
                    .map(|x| (x.layer_id, x.rect)),
                ctx.memory(|m| m.top_modal_layer()),
            );
            assert!(
                focus_in_modal(&r),
                "focus left the dialog (shift = {shift}): {dbg:?}"
            );
        }
    }
}

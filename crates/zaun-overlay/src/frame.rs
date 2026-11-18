//! One frame of the overlay: the passive widgets (the call widget and the toasts) that fade
//! away while the panel is open, then the panel.
use egui::{Area, Context, Id, Order};
use zaun_core::chat::overlay::OverlaySnapshot;
use zaun_core::clock::{Clock, SystemClock};

use crate::closed::{toasts, widget};
use crate::model::Model;
use crate::motion::{FAST, REDUCED};

pub fn draw(ctx: &Context, m: &mut Model, now: f64) {
    let snap = std::sync::Arc::clone(&m.snap);
    m.labels
        .refresh(&snap, m.rev, SystemClock::new().now(), m.offset_s);
    m.step_toasts(now);
    let len = if m.reduced() { REDUCED } else { FAST };
    let closed = 1.0 - m.open.value(now, len);
    let screen = ctx.content_rect();
    if closed > 0.0 && has_passive(&m.snap, m) {
        Area::new(Id::unique("ovl-closed"))
            .order(Order::Background)
            .fixed_pos(screen.min)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(screen);
                if snap.look.voice_widget
                    && let Some(call) = &snap.call
                {
                    widget::draw(ui, m, call, closed);
                }
                toasts::draw(ui, m, now, closed);
            });
    }
    crate::open::panel::draw(ctx, m, now);
}

fn has_passive(s: &OverlaySnapshot, m: &Model) -> bool {
    (s.look.voice_widget && s.call.is_some()) || !m.toasts.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, Rect, pos2, vec2};
    use zaun_core::chat::overlay::snapshot::{Conversation, OverlayChat, OverlayRing, Person};
    use zaun_core::chat::overlay::{ChatKey, Corner, FromOverlay};

    const SCREEN: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1920.0, 1080.0));

    fn person(name: &str) -> Person {
        Person {
            name: name.into(),
            paper: Some(1),
            picture: None,
        }
    }

    fn chat(key: ChatKey, name: &str) -> OverlayChat {
        OverlayChat {
            chat: key,
            who: person(name),
            status: None,
            unread: 0,
            muted: false,
            last: None,
        }
    }

    /// An open panel on Friday Crew, Dev ringing.
    fn rig() -> (Context, Model) {
        let ctx = Context::default();
        crate::style::fonts(&ctx);
        let mut m = Model::new(0);
        m.snap_mut().chats = vec![
            chat(ChatKey::Group(1), "Friday Crew"),
            chat(ChatKey::Direct(2), "Ana"),
        ];
        m.snap_mut().open = Some(Conversation {
            chat: ChatKey::Group(1),
            who: person("Friday Crew"),
            status: None,
            members: Some((6, 5)),
            messages: Vec::new(),
        });
        m.snap_mut().ringing = Some(OverlayRing {
            chat: ChatKey::Direct(3),
            who: person("Dev Patel"),
            since: 0,
        });
        m.snap_mut().look.toasts = true;
        m.snap_mut().look.voice_widget = true;
        m.snap_mut().look.key = "Shift+F1".into();
        m.has_state = true;
        m.open.set(true, 0.0);
        (ctx, m)
    }

    fn pass(ctx: &Context, m: &mut Model, events: Vec<Event>) {
        let raw = RawInput {
            screen_rect: Some(SCREEN),
            time: Some(5.0),
            events,
            ..RawInput::default()
        };
        ctx.run_pass(raw, |ctx| draw(ctx, m, 5.0))
            .drop_without_applying_deltas();
    }

    fn click(ctx: &Context, m: &mut Model, at: Pos2) {
        let button = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        pass(ctx, m, vec![Event::PointerMoved(at)]);
        pass(ctx, m, vec![button(true)]);
        pass(ctx, m, vec![button(false)]);
        pass(ctx, m, Vec::new());
    }

    fn panel() -> Rect {
        Rect::from_center_size(pos2(960.0, 526.0), crate::open::panel::size(SCREEN))
    }

    #[test]
    fn the_pin_opens_the_popover_and_its_switch_writes_the_setting() {
        let (ctx, mut m) = rig();
        pass(&ctx, &mut m, Vec::new());
        let p = panel();
        click(&ctx, &mut m, pos2(p.left() + 280.0 - 25.0, p.top() + 26.0));
        assert!(m.pins, "the pin opens the popover");
        let pop = pos2(p.left() + 240.0, p.top() + 56.0);
        click(&ctx, &mut m, pos2(pop.x + 150.0, pop.y + 32.0 + 19.0));
        assert!(m.said.contains(&FromOverlay::SetLook {
            toasts: false,
            voice_widget: true,
            corner: Corner::TopLeft,
        }));
    }

    #[test]
    fn a_click_on_a_row_opens_that_chat_and_answer_answers() {
        let (ctx, mut m) = rig();
        pass(&ctx, &mut m, Vec::new());
        let p = panel();
        click(
            &ctx,
            &mut m,
            pos2(p.left() + 140.0, p.top() + 52.0 + 52.0 + 26.0),
        );
        assert!(m.said.contains(&FromOverlay::OpenConversation {
            chat: ChatKey::Direct(2)
        }));
        let bar = Rect::from_min_size(
            pos2(p.left() + 280.0 + 12.0, p.top() + 12.0),
            vec2(p.width() - 280.0 - 24.0, 56.0),
        );
        click(
            &ctx,
            &mut m,
            pos2(bar.right() - 12.0 - 52.0, bar.center().y),
        );
        assert!(m.said.contains(&FromOverlay::Answer));
    }

    #[test]
    fn drawing_again_without_a_new_snapshot_makes_no_labels_and_copies_nothing() {
        let (ctx, mut m) = rig();
        let minute = || SystemClock::new().now().as_millis().div_euclid(60_000);
        let before = minute();
        pass(&ctx, &mut m, Vec::new());
        let builds = m.labels.builds;
        let shared = std::sync::Arc::as_ptr(&m.snap);
        for _ in 0..5 {
            pass(&ctx, &mut m, Vec::new());
        }
        // A new minute may make the times again, once.
        let slack = u64::try_from(minute() - before).unwrap_or(0).min(1);
        assert!(
            m.labels.builds <= builds + slack,
            "the labels were made again"
        );
        assert_eq!(
            std::sync::Arc::as_ptr(&m.snap),
            shared,
            "the snapshot was copied"
        );
        m.snap_mut().game = Some("Lantern Keep".into());
        pass(&ctx, &mut m, Vec::new());
        assert!(
            m.labels.builds > builds,
            "a changed snapshot makes them again"
        );
    }

    #[test]
    fn escape_closes_the_popover_before_the_overlay() {
        let (ctx, mut m) = rig();
        m.pins = true;
        let esc = || Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        pass(&ctx, &mut m, vec![esc()]);
        assert!(!m.pins && m.said.is_empty());
        pass(&ctx, &mut m, vec![esc()]);
        assert_eq!(m.said, vec![FromOverlay::Closed]);
    }
}

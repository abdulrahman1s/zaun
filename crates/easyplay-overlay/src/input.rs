//! Pointer and keyboard events become egui input. They only arrive while the panel is open:
//! closed, the input region is empty and the keyboard is not ours.
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers,
};
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use wayland_client::protocol::{wl_keyboard::WlKeyboard, wl_pointer::WlPointer, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::app::App;

/// Linux input codes of the mouse buttons (`linux/input-event-codes.h`).
const BTN_LEFT: u32 = 0x110;
const BTN_RIGHT: u32 = 0x111;
const BTN_MIDDLE: u32 = 0x112;

/// Pixels per scroll step for wheels that report steps.
const STEP: f32 = 40.0;

impl PointerHandler for App {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlPointer,
        events: &[PointerEvent],
    ) {
        for e in events {
            #[allow(clippy::cast_possible_truncation)]
            let pos = egui::pos2(e.position.0 as f32, e.position.1 as f32);
            let ev = match e.kind {
                PointerEventKind::Enter { .. } | PointerEventKind::Motion { .. } => {
                    egui::Event::PointerMoved(pos)
                }
                PointerEventKind::Leave { .. } => egui::Event::PointerGone,
                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let Some(button) = mouse_button(button) else {
                        continue;
                    };
                    egui::Event::PointerButton {
                        pos,
                        button,
                        pressed: matches!(e.kind, PointerEventKind::Press { .. }),
                        modifiers: self.modifiers,
                    }
                }
                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    #[allow(clippy::cast_possible_truncation)]
                    let px = |a: smithay_client_toolkit::seat::pointer::AxisScroll| {
                        if a.discrete != 0 {
                            a.discrete as f32 * STEP
                        } else {
                            a.absolute as f32
                        }
                    };
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(-px(horizontal), -px(vertical)),
                        phase: egui::TouchPhase::Move,
                        source: egui::MouseWheelSource::Unknown,
                        modifiers: self.modifiers,
                    }
                }
            };
            self.events.push(ev);
            self.dirty = true;
        }
    }
}

fn mouse_button(code: u32) -> Option<egui::PointerButton> {
    match code {
        BTN_LEFT => Some(egui::PointerButton::Primary),
        BTN_RIGHT => Some(egui::PointerButton::Secondary),
        BTN_MIDDLE => Some(egui::PointerButton::Middle),
        _ => None,
    }
}

/// A key repeated by the client-side repeat timer.
pub fn on_repeat(app: &mut App, _: &WlKeyboard, e: KeyEvent) {
    app.key(&e, true, true);
}

impl App {
    fn key(&mut self, e: &KeyEvent, pressed: bool, repeat: bool) {
        if let Some(key) = crate::keys::key(e.keysym) {
            self.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat,
                modifiers: self.modifiers,
            });
        }
        let command = self.modifiers.ctrl || self.modifiers.alt;
        if pressed
            && !command
            && let Some(text) = e
                .utf8
                .as_deref()
                .filter(|t| t.chars().all(|c| !c.is_control()))
        {
            self.events.push(egui::Event::Text(text.to_owned()));
        }
        self.dirty = true;
    }
}

impl KeyboardHandler for App {
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
        self.events.push(egui::Event::WindowFocused(true));
        self.dirty = true;
    }

    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.events.push(egui::Event::WindowFocused(false));
        self.dirty = true;
    }

    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        e: KeyEvent,
    ) {
        self.key(&e, true, false);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        e: KeyEvent,
    ) {
        self.key(&e, true, true);
    }

    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        e: KeyEvent,
    ) {
        self.key(&e, false, false);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &WlKeyboard,
        _: u32,
        m: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.modifiers = egui::Modifiers {
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
            mac_cmd: false,
            command: m.ctrl,
        };
        self.events
            .push(egui::Event::ModifiersChanged(self.modifiers));
    }
}

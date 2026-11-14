//! Controller navigation (USER_EXPERIENCE §5.3, ACCESSIBILITY §7, task UX-001).
//!
//! A `gilrs` thread ([`spawn`], feature `gamepad`) forwards raw [`PadInput`] to the UI and
//! wakes it. [`PadNav`] turns them into [`NavIntent`]s with the repeat timing of
//! ACCESSIBILITY §7 (350 ms delay, 12 per second) and a stick dead zone with hysteresis. The
//! app turns intents into ordinary egui input in `raw_input_hook` ([`key_events`]) or into
//! shortcut actions, so a controller drives exactly the keyboard paths of ACCESSIBILITY §2:
//! focus moves spatially, the grid's roving focus, `Esc` closes overlays.
//!
//! Gating (USER_EXPERIENCE §5.3): input is dropped while the window is unfocused, while
//! `integration.gamepad_navigation` is off, and from the moment a game starts until the
//! window regains focus ([`PadGate`]).

use egui::{Event, Key, Modifiers, Vec2};

/// A controller button, independent of the backend (Xbox names; PlayStation ✕ ○ □ △ map to
/// South, East, West, North).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    /// A / ✕.
    South,
    /// B / ○.
    East,
    /// X / □.
    West,
    /// Y / △.
    North,
    /// Start / Options.
    Start,
    /// Back / Select / Share.
    Select,
    /// LB / L1.
    LeftBumper,
    /// RB / R1.
    RightBumper,
    /// LT / L2.
    LeftTrigger,
    /// RT / R2.
    RightTrigger,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
}

/// A stick axis; Y is positive upwards (gilrs convention).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadAxis {
    /// Left stick, horizontal.
    LeftX,
    /// Left stick, vertical.
    LeftY,
    /// Right stick, horizontal.
    RightX,
    /// Right stick, vertical.
    RightY,
}

/// One raw controller event.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PadInput {
    /// A button went down.
    Pressed(PadButton),
    /// A button went up.
    Released(PadButton),
    /// A stick moved to `value` (−1 ..= 1).
    Axis(PadAxis, f32),
    /// The controller went away: release everything.
    Disconnected,
}

/// A direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
}

impl Dir {
    /// The arrow key for this direction.
    pub const fn key(self) -> Key {
        match self {
            Dir::Up => Key::ArrowUp,
            Dir::Down => Key::ArrowDown,
            Dir::Left => Key::ArrowLeft,
            Dir::Right => Key::ArrowRight,
        }
    }
}

/// What the user asked for (USER_EXPERIENCE §5.3).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NavIntent {
    /// D-pad / left stick: move focus (grid-aware).
    Move(Dir),
    /// A: activate; on a card, Play.
    Activate,
    /// B: back, cancel, close the overlay.
    Back,
    /// X: open the game page.
    Open,
    /// Y: toggle favourite.
    Favorite,
    /// Start: context menu.
    ContextMenu,
    /// Select: focus search.
    FocusSearch,
    /// LB: previous collection (library) or tab (pages).
    PrevGroup,
    /// RB: next collection or tab.
    NextGroup,
    /// LT: page up.
    PageUp,
    /// RT: page down.
    PageDown,
    /// Right stick: scroll by this many points.
    Scroll(Vec2),
}

/// The intent of a button press (directions are handled by [`PadNav`] for repeats).
pub const fn intent_for(b: PadButton) -> Option<NavIntent> {
    Some(match b {
        PadButton::South => NavIntent::Activate,
        PadButton::East => NavIntent::Back,
        PadButton::West => NavIntent::Open,
        PadButton::North => NavIntent::Favorite,
        PadButton::Start => NavIntent::ContextMenu,
        PadButton::Select => NavIntent::FocusSearch,
        PadButton::LeftBumper => NavIntent::PrevGroup,
        PadButton::RightBumper => NavIntent::NextGroup,
        PadButton::LeftTrigger => NavIntent::PageUp,
        PadButton::RightTrigger => NavIntent::PageDown,
        PadButton::DPadUp | PadButton::DPadDown | PadButton::DPadLeft | PadButton::DPadRight => {
            return None;
        }
    })
}

const fn dpad_dir(b: PadButton) -> Option<Dir> {
    match b {
        PadButton::DPadUp => Some(Dir::Up),
        PadButton::DPadDown => Some(Dir::Down),
        PadButton::DPadLeft => Some(Dir::Left),
        PadButton::DPadRight => Some(Dir::Right),
        _ => None,
    }
}

/// Delay before a held direction repeats (ACCESSIBILITY §7).
pub const REPEAT_DELAY_S: f64 = 0.350;
/// Repeats per second while held.
pub const REPEAT_RATE: f64 = 12.0;
/// A stick counts as pushed beyond this deflection…
pub const STICK_ON: f32 = 0.55;
/// …and as released below this one (hysteresis against jitter).
pub const STICK_OFF: f32 = 0.35;
/// Right-stick dead zone.
pub const SCROLL_DEAD_ZONE: f32 = 0.2;
/// Scroll speed at full deflection, points per second.
pub const SCROLL_SPEED: f32 = 900.0;

/// Turns raw input into intents, with repeat timing. Times are seconds (egui input time).
#[derive(Clone, Debug, Default)]
pub struct PadNav {
    dpad: Option<Dir>,
    stick: Option<Dir>,
    left: Vec2,
    right: Vec2,
    /// The held direction, when it started repeating and when it last fired.
    held: Option<(Dir, f64, f64)>,
    last_tick: Option<f64>,
}

impl PadNav {
    /// Feed one event at `now`; returns the intents it produces immediately.
    pub fn input(&mut self, ev: PadInput, now: f64) -> Vec<NavIntent> {
        let mut out = Vec::new();
        match ev {
            PadInput::Pressed(b) => {
                if let Some(d) = dpad_dir(b) {
                    self.dpad = Some(d);
                } else if let Some(i) = intent_for(b) {
                    out.push(i);
                }
            }
            PadInput::Released(b) => {
                if dpad_dir(b).is_some() && self.dpad == dpad_dir(b) {
                    self.dpad = None;
                }
            }
            PadInput::Axis(a, v) => {
                let v = if v.is_finite() {
                    v.clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                match a {
                    PadAxis::LeftX => self.left.x = v,
                    PadAxis::LeftY => self.left.y = v,
                    PadAxis::RightX => self.right.x = v,
                    PadAxis::RightY => self.right.y = v,
                }
                self.stick = stick_dir(self.left, self.stick);
                if self.right.length() < SCROLL_DEAD_ZONE {
                    self.last_tick = None;
                } else if self.last_tick.is_none() {
                    self.last_tick = Some(now);
                }
            }
            PadInput::Disconnected => {
                *self = Self::default();
            }
        }
        // The D-pad wins over the stick when both are held.
        let want = self.dpad.or(self.stick);
        match (want, self.held) {
            (Some(d), Some((h, _, _))) if d == h => {}
            (Some(d), _) => {
                out.push(NavIntent::Move(d));
                self.held = Some((d, now, now));
            }
            (None, _) => self.held = None,
        }
        out
    }

    /// Repeats and scrolling due at `now`, and when to call again (seconds from now).
    pub fn tick(&mut self, now: f64) -> (Vec<NavIntent>, Option<f64>) {
        let mut out = Vec::new();
        let mut next: Option<f64> = None;
        let mut soonest = |t: f64| next = Some(next.map_or(t, |n: f64| n.min(t)));
        if let Some((d, start, last)) = self.held {
            let interval = 1.0 / REPEAT_RATE;
            let first = start + REPEAT_DELAY_S;
            let due = if last <= start {
                first
            } else {
                last + interval
            };
            if now >= due {
                out.push(NavIntent::Move(d));
                self.held = Some((d, start, now));
                soonest(interval);
            } else {
                soonest(due - now);
            }
        }
        if let Some(t) = self.last_tick {
            let dt = (now - t).clamp(0.0, 0.1) as f32;
            self.last_tick = Some(now);
            if dt > 0.0 {
                // Stick up scrolls content up: positive wheel delta.
                out.push(NavIntent::Scroll(
                    Vec2::new(-self.right.x, self.right.y) * SCROLL_SPEED * dt,
                ));
            }
            soonest(1.0 / 60.0);
        }
        (out, next)
    }

    /// Whether anything is held (repaints are needed for repeats or scrolling).
    pub fn is_active(&self) -> bool {
        self.held.is_some() || self.last_tick.is_some()
    }
}

fn stick_dir(v: Vec2, current: Option<Dir>) -> Option<Dir> {
    let (ax, ay) = (v.x.abs(), v.y.abs());
    let threshold = if current.is_some() {
        STICK_OFF
    } else {
        STICK_ON
    };
    if ax.max(ay) < threshold {
        return None;
    }
    Some(if ax > ay {
        if v.x > 0.0 { Dir::Right } else { Dir::Left }
    } else if v.y > 0.0 {
        Dir::Up
    } else {
        Dir::Down
    })
}

/// Whether controller input may act now (USER_EXPERIENCE §5.3).
#[derive(Clone, Copy, Debug, Default)]
pub struct PadGate {
    /// A game started since the window last gained focus.
    paused_for_game: bool,
    was_focused: bool,
}

impl PadGate {
    /// Update with this frame's facts; returns whether pad input is accepted.
    pub fn update(&mut self, enabled: bool, focused: bool, game_started: bool) -> bool {
        if game_started {
            self.paused_for_game = true;
        }
        if focused && !self.was_focused {
            // Regaining focus after the game resumes reading immediately (UF-15 step 4).
            self.paused_for_game = false;
        }
        self.was_focused = focused;
        enabled && focused && !self.paused_for_game
    }
}

fn key(key: Key, modifiers: Modifiers) -> [Event; 2] {
    let ev = |pressed| Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers,
    };
    [ev(true), ev(false)]
}

/// The keyboard equivalent of an intent, as egui events, or `None` when the intent is an
/// application action (favourite, context menu, search, collection/tab cycling).
/// `focused` says whether a widget has keyboard focus: `A` then presses it (`Space`), else it
/// confirms the default dialog button (`Enter`).
pub fn key_events(intent: NavIntent, focused: bool) -> Option<Vec<Event>> {
    let none = Modifiers::NONE;
    Some(match intent {
        NavIntent::Move(d) => key(d.key(), none).to_vec(),
        NavIntent::Activate => key(if focused { Key::Space } else { Key::Enter }, none).to_vec(),
        NavIntent::Back => key(Key::Escape, none).to_vec(),
        NavIntent::Open => key(Key::Enter, none).to_vec(),
        NavIntent::PageUp => key(Key::PageUp, none).to_vec(),
        NavIntent::PageDown => key(Key::PageDown, none).to_vec(),
        NavIntent::Scroll(delta) => vec![Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta,
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: none,
        }],
        NavIntent::Favorite
        | NavIntent::ContextMenu
        | NavIntent::FocusSearch
        | NavIntent::PrevGroup
        | NavIntent::NextGroup => return None,
    })
}

/// Whether `raw` holds real keyboard or pointer activity (which leaves controller mode).
pub fn has_desktop_input(raw: &egui::RawInput) -> bool {
    raw.events.iter().any(|e| {
        matches!(
            e,
            Event::Key { pressed: true, .. }
                | Event::Text(_)
                | Event::PointerButton { pressed: true, .. }
                | Event::MouseWheel { .. }
        ) || matches!(e, Event::PointerMoved(_))
    })
}

/// Where the reader thread sends: the window that asked last, and its waker.
#[cfg(feature = "gamepad")]
type PadSink = (
    std::sync::mpsc::Sender<PadInput>,
    std::sync::Arc<dyn Fn() + Send + Sync>,
);

/// Read controller input for a window. Returns `None` when the reader thread cannot be
/// started. `wake` is called after each forwarded event.
///
/// The process has one `gilrs` reader thread, started by the first call; a later call (the
/// resident service opens windows in turn) only points it at the new window. A reader per
/// window could not be stopped while it waits in `gilrs`, so each closed window would leave
/// a thread and its device descriptors behind.
#[cfg(feature = "gamepad")]
pub fn spawn(
    wake: std::sync::Arc<dyn Fn() + Send + Sync>,
) -> Option<std::sync::mpsc::Receiver<PadInput>> {
    use std::sync::{Arc, Mutex, PoisonError};

    static READER: Mutex<Option<Arc<Mutex<Option<PadSink>>>>> = Mutex::new(None);

    let (tx, rx) = std::sync::mpsc::channel();
    let mut reader = READER.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(sink) = reader.as_ref() {
        *sink.lock().unwrap_or_else(PoisonError::into_inner) = Some((tx, wake));
        return Some(rx);
    }
    let sink = Arc::new(Mutex::new(Some((tx, wake))));
    let for_thread = Arc::clone(&sink);
    std::thread::Builder::new()
        .name("gamepad".into())
        .spawn(move || {
            // Without a backend the thread ends and the receivers simply stay empty.
            let mut gilrs = match gilrs::Gilrs::new() {
                Ok(g) => g,
                Err(e) => {
                    tracing::info!("controller navigation unavailable: {e}");
                    return;
                }
            };
            loop {
                let Some(ev) = gilrs.next_event_blocking(None) else {
                    continue;
                };
                let Some(input) = from_gilrs(ev.event) else {
                    continue;
                };
                let mut sink = for_thread.lock().unwrap_or_else(PoisonError::into_inner);
                match sink.as_ref() {
                    Some((tx, wake)) if tx.send(input).is_ok() => wake(),
                    // The window closed: let go of its waker until the next one opens.
                    Some(_) => *sink = None,
                    None => {}
                }
            }
        })
        .ok()?;
    *reader = Some(sink);
    Some(rx)
}

#[cfg(feature = "gamepad")]
fn from_gilrs(ev: gilrs::EventType) -> Option<PadInput> {
    use gilrs::{Axis, Button, EventType};
    let button = |b: Button| -> Option<PadButton> {
        Some(match b {
            Button::South => PadButton::South,
            Button::East => PadButton::East,
            Button::West => PadButton::West,
            Button::North => PadButton::North,
            Button::Start => PadButton::Start,
            Button::Select => PadButton::Select,
            Button::LeftTrigger => PadButton::LeftBumper,
            Button::RightTrigger => PadButton::RightBumper,
            Button::LeftTrigger2 => PadButton::LeftTrigger,
            Button::RightTrigger2 => PadButton::RightTrigger,
            Button::DPadUp => PadButton::DPadUp,
            Button::DPadDown => PadButton::DPadDown,
            Button::DPadLeft => PadButton::DPadLeft,
            Button::DPadRight => PadButton::DPadRight,
            _ => return None,
        })
    };
    match ev {
        EventType::ButtonPressed(b, _) => button(b).map(PadInput::Pressed),
        EventType::ButtonReleased(b, _) => button(b).map(PadInput::Released),
        EventType::AxisChanged(a, v, _) => {
            let axis = match a {
                Axis::LeftStickX => PadAxis::LeftX,
                Axis::LeftStickY => PadAxis::LeftY,
                Axis::RightStickX => PadAxis::RightX,
                Axis::RightStickY => PadAxis::RightY,
                _ => return None,
            };
            Some(PadInput::Axis(axis, v))
        }
        EventType::Disconnected => Some(PadInput::Disconnected),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_map_to_the_controller_table() {
        assert_eq!(intent_for(PadButton::South), Some(NavIntent::Activate));
        assert_eq!(intent_for(PadButton::East), Some(NavIntent::Back));
        assert_eq!(intent_for(PadButton::West), Some(NavIntent::Open));
        assert_eq!(intent_for(PadButton::North), Some(NavIntent::Favorite));
        assert_eq!(intent_for(PadButton::Start), Some(NavIntent::ContextMenu));
        assert_eq!(intent_for(PadButton::Select), Some(NavIntent::FocusSearch));
        assert_eq!(
            intent_for(PadButton::LeftBumper),
            Some(NavIntent::PrevGroup)
        );
        assert_eq!(
            intent_for(PadButton::RightBumper),
            Some(NavIntent::NextGroup)
        );
        assert_eq!(intent_for(PadButton::LeftTrigger), Some(NavIntent::PageUp));
        assert_eq!(
            intent_for(PadButton::RightTrigger),
            Some(NavIntent::PageDown)
        );
        assert_eq!(intent_for(PadButton::DPadUp), None);
    }

    #[test]
    fn dpad_moves_once_then_repeats_after_the_delay_at_twelve_per_second() {
        let mut n = PadNav::default();
        assert_eq!(
            n.input(PadInput::Pressed(PadButton::DPadDown), 0.0),
            vec![NavIntent::Move(Dir::Down)]
        );
        let (none, next) = n.tick(0.2);
        assert!(none.is_empty());
        assert!((next.unwrap() - 0.15).abs() < 1e-9);
        assert_eq!(n.tick(0.35).0, vec![NavIntent::Move(Dir::Down)]);
        assert!(n.tick(0.35 + 0.05).0.is_empty());
        assert_eq!(
            n.tick(0.35 + 1.0 / 12.0).0,
            vec![NavIntent::Move(Dir::Down)]
        );
        // One second of holding after the delay: about twelve repeats.
        let mut count = 0;
        let mut t = 0.35 + 1.0 / 12.0;
        while t < 1.35 + 1.0 / 12.0 {
            t += 1.0 / 120.0;
            count += n.tick(t).0.len();
        }
        assert!((11..=13).contains(&count), "{count}");
        assert!(
            n.input(PadInput::Released(PadButton::DPadDown), t)
                .is_empty()
        );
        assert!(n.tick(t + 1.0).0.is_empty());
        assert!(!n.is_active());
    }

    #[test]
    fn stick_has_a_dead_zone_and_hysteresis() {
        let mut n = PadNav::default();
        assert!(n.input(PadInput::Axis(PadAxis::LeftX, 0.4), 0.0).is_empty());
        assert_eq!(
            n.input(PadInput::Axis(PadAxis::LeftX, 0.6), 0.0),
            vec![NavIntent::Move(Dir::Right)]
        );
        // Falling back to 0.4 keeps it held (no second move, no release).
        assert!(n.input(PadInput::Axis(PadAxis::LeftX, 0.4), 0.1).is_empty());
        assert!(n.is_active());
        assert!(n.input(PadInput::Axis(PadAxis::LeftX, 0.1), 0.2).is_empty());
        assert!(!n.is_active());
        // Up is positive Y.
        assert_eq!(
            n.input(PadInput::Axis(PadAxis::LeftY, 0.9), 0.3),
            vec![NavIntent::Move(Dir::Up)]
        );
    }

    #[test]
    fn right_stick_scrolls_while_deflected() {
        let mut n = PadNav::default();
        n.input(PadInput::Axis(PadAxis::RightY, -1.0), 0.0);
        let (out, next) = n.tick(0.05);
        assert!(matches!(out.as_slice(), [NavIntent::Scroll(v)] if v.y < 0.0));
        assert!(next.is_some());
        n.input(PadInput::Axis(PadAxis::RightY, 0.0), 0.06);
        assert!(n.tick(0.1).0.is_empty());
    }

    #[test]
    fn disconnect_releases_everything() {
        let mut n = PadNav::default();
        n.input(PadInput::Pressed(PadButton::DPadLeft), 0.0);
        n.input(PadInput::Disconnected, 0.1);
        assert!(!n.is_active());
        assert!(n.tick(1.0).0.is_empty());
    }

    #[test]
    fn the_gate_closes_while_a_game_runs_until_focus_returns() {
        let mut g = PadGate::default();
        assert!(g.update(true, true, false));
        assert!(!g.update(false, true, false), "preference off");
        assert!(!g.update(true, true, true), "game started");
        assert!(!g.update(true, true, false), "still the same focus");
        assert!(!g.update(true, false, false), "unfocused");
        assert!(g.update(true, true, false), "focus regained");
    }

    #[test]
    fn intents_become_keys() {
        let keys = |i, f| {
            key_events(i, f)
                .unwrap()
                .into_iter()
                .filter_map(|e| match e {
                    Event::Key {
                        key, pressed: true, ..
                    } => Some(key),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(keys(NavIntent::Move(Dir::Left), true), vec![Key::ArrowLeft]);
        assert_eq!(keys(NavIntent::Activate, true), vec![Key::Space]);
        assert_eq!(keys(NavIntent::Activate, false), vec![Key::Enter]);
        assert_eq!(keys(NavIntent::Back, true), vec![Key::Escape]);
        assert_eq!(keys(NavIntent::Open, true), vec![Key::Enter]);
        assert!(key_events(NavIntent::Favorite, true).is_none());
    }
}

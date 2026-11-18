//! Toasts (notes §5 "Toast"): one at a time, a new one replaces the one showing; 3.2 s, 6 s
//! with an action; paused while hovered or focused. Time comes from the injected clock's
//! monotonic `MonoInstant`, so tests drive it with `FakeClock`.

use std::collections::VecDeque;
use std::time::Duration;

use zaun_core::clock::MonoInstant;

use crate::theme::tokens::{layout, timing};

/// Identifies one toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToastId(pub u64);

/// The toast's tone. The design draws every toast alike; the tone is what assistive
/// technology hears.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToastKind {
    /// Neutral information.
    #[default]
    Info,
    /// A completed action.
    Success,
    /// Something did not work but needs no decision.
    Warning,
}

/// An action button on a toast; `key` tells the owner what to do when it is activated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToastAction {
    /// Verb label ("Undo").
    pub label: String,
    /// Opaque key returned to the owner.
    pub key: u64,
}

/// One toast.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    /// Identifier.
    pub id: ToastId,
    /// One line of text.
    pub text: String,
    /// Tone.
    pub kind: ToastKind,
    /// Optional action.
    pub action: Option<ToastAction>,
    remaining: Duration,
    running_since: Option<MonoInstant>,
}

impl Toast {
    /// Time left before it expires, as of `now`.
    pub fn remaining(&self, now: MonoInstant) -> Duration {
        match self.running_since {
            Some(t) => self
                .remaining
                .saturating_sub(now.saturating_duration_since(t)),
            None => self.remaining,
        }
    }

    /// Whether the timer is paused.
    pub fn is_paused(&self) -> bool {
        self.running_since.is_none()
    }
}

/// The queue.
#[derive(Clone, Debug, Default)]
pub struct ToastQueue {
    visible: Vec<Toast>,
    waiting: VecDeque<Toast>,
    next: u64,
}

fn lifetime(has_action: bool) -> Duration {
    let secs = if has_action {
        timing::TOAST_WITH_ACTION
    } else {
        timing::TOAST
    };
    Duration::from_millis((secs * 1000.0).round() as u64)
}

impl ToastQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Show a toast, replacing the one showing. Returns its id.
    pub fn push(
        &mut self,
        text: impl Into<String>,
        kind: ToastKind,
        action: Option<ToastAction>,
        now: MonoInstant,
    ) -> ToastId {
        let id = ToastId(self.next);
        self.next += 1;
        let toast = Toast {
            id,
            text: text.into(),
            kind,
            remaining: lifetime(action.is_some()),
            action,
            running_since: None,
        };
        // One at a time: the new one replaces whatever shows or waits.
        self.visible.clear();
        self.waiting.clear();
        self.waiting.push_back(toast);
        self.promote(now);
        id
    }

    fn promote(&mut self, now: MonoInstant) {
        while self.visible.len() < layout::TOAST_MAX {
            match self.waiting.pop_front() {
                Some(mut t) => {
                    t.running_since = Some(now);
                    self.visible.push(t);
                }
                None => break,
            }
        }
    }

    /// Drop expired toasts and promote waiting ones. Returns how long until the next
    /// expiry, for `request_repaint_after` (`None` when nothing is running).
    pub fn tick(&mut self, now: MonoInstant) -> Option<Duration> {
        self.visible
            .retain(|t| t.is_paused() || !t.remaining(now).is_zero());
        self.promote(now);
        self.visible
            .iter()
            .filter(|t| !t.is_paused())
            .map(|t| t.remaining(now))
            .min()
    }

    /// Toasts on screen, oldest first.
    pub fn visible(&self) -> &[Toast] {
        &self.visible
    }

    /// Toasts waiting for a slot.
    pub fn waiting(&self) -> usize {
        self.waiting.len()
    }

    /// Remove a toast (dismiss or action taken).
    pub fn dismiss(&mut self, id: ToastId, now: MonoInstant) {
        self.visible.retain(|t| t.id != id);
        self.waiting.retain(|t| t.id != id);
        self.promote(now);
    }

    /// Pause (`hold = true`, on hover or focus) or resume a toast's timer.
    pub fn hold(&mut self, id: ToastId, hold: bool, now: MonoInstant) {
        if let Some(t) = self.visible.iter_mut().find(|t| t.id == id) {
            match (hold, t.running_since) {
                (true, Some(_)) => {
                    t.remaining = t.remaining(now);
                    t.running_since = None;
                }
                (false, None) => t.running_since = Some(now),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaun_core::clock::Clock;
    use zaun_testkit::FakeClock;

    #[test]
    fn expires_after_three_point_two_seconds_or_six_with_action() {
        let clock = FakeClock::baseline();
        let mut q = ToastQueue::new();
        q.push("Saved", ToastKind::Success, None, clock.mono());
        assert_eq!(q.tick(clock.mono()), Some(Duration::from_millis(3_200)));
        clock.advance(Duration::from_millis(3_199));
        assert_eq!(q.tick(clock.mono()).map(|d| d.as_millis()), Some(1));
        clock.advance(Duration::from_millis(1));
        q.tick(clock.mono());
        assert!(q.visible().is_empty());
        q.push(
            "Removed from Favourites",
            ToastKind::Info,
            Some(ToastAction {
                label: "Undo".into(),
                key: 1,
            }),
            clock.mono(),
        );
        assert_eq!(q.tick(clock.mono()), Some(Duration::from_secs(6)));
        clock.advance(Duration::from_secs(6));
        assert_eq!(q.tick(clock.mono()), None);
        assert!(q.visible().is_empty());
    }

    #[test]
    fn hover_pauses_and_resumes() {
        let clock = FakeClock::baseline();
        let mut q = ToastQueue::new();
        let id = q.push("Saved", ToastKind::Success, None, clock.mono());
        clock.advance(Duration::from_secs(2));
        q.hold(id, true, clock.mono());
        clock.advance(Duration::from_secs(60));
        q.tick(clock.mono());
        assert_eq!(q.visible().len(), 1);
        q.hold(id, false, clock.mono());
        assert_eq!(
            q.visible()[0].remaining(clock.mono()),
            Duration::from_millis(1_200)
        );
        clock.advance(Duration::from_millis(1_200));
        q.tick(clock.mono());
        assert!(q.visible().is_empty());
    }

    #[test]
    fn a_new_toast_replaces_the_one_showing() {
        let clock = FakeClock::baseline();
        let mut q = ToastQueue::new();
        let ids: Vec<_> = (0..3)
            .map(|i| q.push(format!("t{i}"), ToastKind::Info, None, clock.mono()))
            .collect();
        assert_eq!(q.visible().len(), 1);
        assert_eq!(q.waiting(), 0);
        assert_eq!(q.visible()[0].text, "t2");
        q.dismiss(ids[2], clock.mono());
        assert!(q.visible().is_empty());
    }
}

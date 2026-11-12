//! Toast queue (USER_EXPERIENCE §6): 5 s, 8 s with an action; paused while hovered or
//! focused; at most three on screen, the rest wait. Time comes from the injected clock's
//! monotonic `MonoInstant`, so tests drive it with `FakeClock`.

use std::collections::VecDeque;
use std::time::Duration;

use easyplay_core::clock::MonoInstant;

use crate::theme::tokens::{layout, timing};

/// Identifies one toast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToastId(pub u64);

/// The toast's tone (icon and accent of the leading glyph).
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
    Duration::from_secs_f32(if has_action {
        timing::TOAST_WITH_ACTION
    } else {
        timing::TOAST
    })
}

impl ToastQueue {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a toast. Returns its id.
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
    use easyplay_core::clock::Clock;
    use easyplay_testkit::FakeClock;

    #[test]
    fn expires_after_five_seconds_or_eight_with_action() {
        let clock = FakeClock::baseline();
        let mut q = ToastQueue::new();
        q.push("Saved", ToastKind::Success, None, clock.mono());
        q.push(
            "Removed from favourites",
            ToastKind::Info,
            Some(ToastAction {
                label: "Undo".into(),
                key: 1,
            }),
            clock.mono(),
        );
        assert_eq!(q.tick(clock.mono()), Some(Duration::from_secs(5)));
        clock.advance(Duration::from_millis(4_999));
        assert_eq!(q.tick(clock.mono()).map(|d| d.as_millis()), Some(1));
        clock.advance(Duration::from_millis(1));
        q.tick(clock.mono());
        assert_eq!(q.visible().len(), 1);
        clock.advance(Duration::from_secs(3));
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
            Duration::from_secs(3)
        );
        clock.advance(Duration::from_secs(3));
        q.tick(clock.mono());
        assert!(q.visible().is_empty());
    }

    #[test]
    fn at_most_three_visible_and_the_rest_wait() {
        let clock = FakeClock::baseline();
        let mut q = ToastQueue::new();
        let ids: Vec<_> = (0..5)
            .map(|i| q.push(format!("t{i}"), ToastKind::Info, None, clock.mono()))
            .collect();
        assert_eq!(q.visible().len(), 3);
        assert_eq!(q.waiting(), 2);
        q.dismiss(ids[0], clock.mono());
        assert_eq!(q.visible().len(), 3);
        assert_eq!(q.visible()[2].text, "t3");
        // The promoted toast's timer starts when it becomes visible.
        clock.advance(Duration::from_secs(5));
        q.tick(clock.mono());
        assert_eq!(q.visible().len(), 1);
        assert_eq!(q.visible()[0].text, "t4");
    }
}

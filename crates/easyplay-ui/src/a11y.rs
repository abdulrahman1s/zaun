//! Helpers that attach accessible names and roles; live-region announcements. See MODULES §8
//! and ACCESSIBILITY §4.
//!
//! [`Announcer`] keeps one visually hidden status node whose text is replaced to trigger an
//! announcement; messages are queued, de-duplicated and rate-limited (ACCESSIBILITY §4.3).

use std::collections::{HashMap, VecDeque};

use egui::accesskit::{Live, Role};
use egui::{Context, Id, Response, Sense, Ui, Vec2, WidgetInfo};

use crate::theme::tokens::timing::ANNOUNCE_INTERVAL;

/// A button named `label`.
pub fn button(resp: &Response, label: &str) {
    let enabled = resp.enabled();
    let label = label.to_owned();
    resp.widget_info(move || WidgetInfo::labeled(Role::Button, enabled, &label));
}

/// A two-state button (favourite, filter chip) named `label`.
pub fn toggle(resp: &Response, label: &str, pressed: bool) {
    let enabled = resp.enabled();
    let label = label.to_owned();
    resp.widget_info(move || WidgetInfo::selected(Role::Switch, enabled, pressed, &label));
}

/// A radio option or tab named `label`.
pub fn option(resp: &Response, role: Role, label: &str, selected: bool) {
    let enabled = resp.enabled();
    let label = label.to_owned();
    resp.widget_info(move || WidgetInfo::selected(role, enabled, selected, &label));
}

/// A non-interactive element with a role and a name (images, landmarks, groups).
pub fn labelled(ctx: &Context, id: Id, role: Role, label: &str) {
    ctx.accesskit_node_builder(id, |n| {
        n.set_role(role);
        n.set_label(label.to_owned());
    });
}

/// Attach a description (helper or error text) to a widget.
pub fn describe(resp: &Response, description: &str) {
    let description = description.to_owned();
    resp.ctx.accesskit_node_builder(resp.id, |n| {
        n.set_description(description);
    });
}

/// How urgently an announcement interrupts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Politeness {
    /// After the current speech.
    Polite,
    /// Interrupts.
    Assertive,
}

/// Search results are announced once typing pauses (ACCESSIBILITY §4.3).
pub const SEARCH_DEBOUNCE_S: f64 = 1.0;
/// Download progress is announced at most this often.
pub const DOWNLOAD_EVERY_S: f64 = 15.0;

/// The announcement queue, independent of egui so it can be tested with a fake clock.
/// Times are seconds on any monotonic scale.
///
/// - [`AnnounceQueue::push`] queues a message; a duplicate of the queued or current message
///   is dropped; assertive messages jump the queue.
/// - [`AnnounceQueue::debounce`] keeps only the latest message under a key until `delay`
///   has passed without a newer one (search results).
/// - [`AnnounceQueue::throttle`] drops a keyed message sent sooner than `every` after the
///   last one that went through (download progress).
/// - [`AnnounceQueue::tick`] exposes at most one new message per `interval`.
#[derive(Clone, Debug)]
pub struct AnnounceQueue {
    interval: f64,
    queue: VecDeque<(String, Politeness)>,
    current: Option<(String, Politeness)>,
    last_change: Option<f64>,
    debounced: Vec<(&'static str, String, Politeness, f64)>,
    throttled: HashMap<&'static str, f64>,
}

impl Default for AnnounceQueue {
    fn default() -> Self {
        Self::new(f64::from(ANNOUNCE_INTERVAL))
    }
}

impl AnnounceQueue {
    /// A queue releasing one message per `interval` seconds.
    pub fn new(interval: f64) -> Self {
        Self {
            interval,
            queue: VecDeque::new(),
            current: None,
            last_change: None,
            debounced: Vec::new(),
            throttled: HashMap::new(),
        }
    }

    /// Queue `text`.
    pub fn push(&mut self, text: String, politeness: Politeness) {
        let dup = self.current.as_ref().is_some_and(|(t, _)| *t == text)
            || self.queue.iter().any(|(t, _)| *t == text);
        if dup || text.trim().is_empty() {
            return;
        }
        if politeness == Politeness::Assertive {
            // After other assertive messages, before polite ones.
            let at = self
                .queue
                .iter()
                .position(|(_, p)| *p == Politeness::Polite)
                .unwrap_or(self.queue.len());
            self.queue.insert(at, (text, politeness));
        } else {
            self.queue.push_back((text, politeness));
        }
    }

    /// Queue `text` under `key` once `delay` seconds pass without another message for `key`.
    pub fn debounce(
        &mut self,
        key: &'static str,
        text: String,
        politeness: Politeness,
        now: f64,
        delay: f64,
    ) {
        self.debounced.retain(|(k, ..)| *k != key);
        self.debounced.push((key, text, politeness, now + delay));
    }

    /// Queue `text` unless a message for `key` went through less than `every` seconds ago.
    /// Returns whether it was queued.
    pub fn throttle(
        &mut self,
        key: &'static str,
        text: String,
        politeness: Politeness,
        now: f64,
        every: f64,
    ) -> bool {
        if self.throttled.get(key).is_some_and(|t| now - t < every) {
            return false;
        }
        self.throttled.insert(key, now);
        self.push(text, politeness);
        true
    }

    /// Advance to `now`. Returns the message to expose and, when more is waiting, how many
    /// seconds until the next call is useful.
    pub fn tick(&mut self, now: f64) -> (Option<&(String, Politeness)>, Option<f64>) {
        let mut due = Vec::new();
        self.debounced.retain(|(_, text, pol, at)| {
            if *at <= now {
                due.push((text.clone(), *pol));
                false
            } else {
                true
            }
        });
        for (t, p) in due {
            self.push(t, p);
        }
        let elapsed = self.last_change.map_or(f64::INFINITY, |t| now - t);
        let mut wait: Option<f64> = None;
        if !self.queue.is_empty() {
            if elapsed >= self.interval {
                self.current = self.queue.pop_front();
                self.last_change = Some(now);
                if !self.queue.is_empty() {
                    wait = Some(self.interval);
                }
            } else {
                wait = Some(self.interval - elapsed);
            }
        }
        if let Some(at) = self
            .debounced
            .iter()
            .map(|(.., at)| *at)
            .min_by(f64::total_cmp)
        {
            let w = (at - now).max(0.0);
            wait = Some(wait.map_or(w, |x| x.min(w)));
        }
        (self.current.as_ref(), wait)
    }

    /// The message currently exposed.
    pub fn current(&self) -> Option<&str> {
        self.current.as_ref().map(|(t, _)| t.as_str())
    }

    /// Messages waiting (tests).
    pub fn pending(&self) -> usize {
        self.queue.len() + self.debounced.len()
    }
}

/// The single live region of the window, stored in egui's data map.
pub struct Announcer;

impl Announcer {
    fn id() -> Id {
        Id::unique("easyplay.announcer")
    }

    fn with<R>(ctx: &Context, f: impl FnOnce(&mut AnnounceQueue) -> R) -> R {
        let r = ctx.data_mut(|d| f(d.get_temp_mut_or_default::<AnnounceQueue>(Self::id())));
        ctx.request_repaint();
        r
    }

    /// Queue `text`. Duplicates of the queued or current message are dropped.
    pub fn announce(ctx: &Context, text: impl Into<String>, politeness: Politeness) {
        let text = text.into();
        Self::with(ctx, |q| q.push(text, politeness));
    }

    /// Queue `text` once `delay` seconds pass without another message for `key`.
    pub fn announce_debounced(
        ctx: &Context,
        key: &'static str,
        text: impl Into<String>,
        politeness: Politeness,
        delay: f64,
    ) {
        let now = ctx.input(|i| i.time);
        let text = text.into();
        Self::with(ctx, |q| q.debounce(key, text, politeness, now, delay));
    }

    /// Queue `text` unless a message for `key` went out less than `every` seconds ago.
    pub fn announce_throttled(
        ctx: &Context,
        key: &'static str,
        text: impl Into<String>,
        politeness: Politeness,
        every: f64,
    ) {
        let now = ctx.input(|i| i.time);
        let text = text.into();
        Self::with(ctx, |q| q.throttle(key, text, politeness, now, every));
    }

    /// The message currently exposed (tests).
    pub fn current(ctx: &Context) -> Option<String> {
        ctx.data(|d| d.get_temp::<AnnounceQueue>(Self::id()))
            .and_then(|s| s.current().map(str::to_owned))
    }

    /// Advance the queue and paint the hidden status node. Call once per frame.
    pub fn show(ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let ctx = ui.ctx().clone();
        let (current, wait) = ctx.data_mut(|d| {
            let q = d.get_temp_mut_or_default::<AnnounceQueue>(Self::id());
            let (c, w) = q.tick(now);
            (c.cloned(), w)
        });
        if let Some(w) = wait {
            ctx.request_repaint_after_secs(w as f32);
        }
        let resp = ui.allocate_response(Vec2::ZERO, Sense::hover());
        if let Some((text, politeness)) = current {
            ctx.accesskit_node_builder(resp.id, |n| {
                n.set_role(Role::Status);
                n.set_label(text);
                n.set_live(match politeness {
                    Politeness::Polite => Live::Polite,
                    Politeness::Assertive => Live::Assertive,
                });
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use easyplay_core::clock::Clock;
    use easyplay_testkit::FakeClock;

    use super::*;

    fn secs(c: &FakeClock) -> f64 {
        c.mono().as_duration().as_secs_f64()
    }

    #[test]
    fn one_message_per_interval_in_order() {
        let clock = FakeClock::baseline();
        let mut q = AnnounceQueue::default();
        q.push("a".into(), Politeness::Polite);
        q.push("b".into(), Politeness::Polite);
        let (cur, wait) = q.tick(secs(&clock));
        assert_eq!(cur.map(|c| c.0.as_str()), Some("a"));
        assert_eq!(wait, Some(0.5));
        clock.advance(Duration::from_millis(200));
        assert_eq!(q.tick(secs(&clock)).0.map(|c| c.0.as_str()), Some("a"));
        clock.advance(Duration::from_millis(300));
        assert_eq!(q.tick(secs(&clock)).0.map(|c| c.0.as_str()), Some("b"));
        assert_eq!(q.pending(), 0);
    }

    #[test]
    fn duplicates_are_dropped_and_assertive_jumps_the_queue() {
        let clock = FakeClock::baseline();
        let mut q = AnnounceQueue::default();
        q.push("polite".into(), Politeness::Polite);
        q.push("polite".into(), Politeness::Polite);
        q.push("urgent".into(), Politeness::Assertive);
        q.push(String::new(), Politeness::Polite);
        assert_eq!(q.pending(), 2);
        assert_eq!(q.tick(secs(&clock)).0.map(|c| c.0.as_str()), Some("urgent"));
        // The current message is not repeated.
        q.push("urgent".into(), Politeness::Assertive);
        assert_eq!(q.pending(), 1);
    }

    #[test]
    fn search_results_wait_until_typing_pauses() {
        let clock = FakeClock::baseline();
        let mut q = AnnounceQueue::default();
        for n in ["5 games", "3 games", "1 game"] {
            q.debounce("search", n.into(), Politeness::Polite, secs(&clock), 1.0);
            clock.advance(Duration::from_millis(300));
            assert!(q.tick(secs(&clock)).0.is_none());
        }
        clock.advance(Duration::from_millis(700));
        assert_eq!(q.tick(secs(&clock)).0.map(|c| c.0.as_str()), Some("1 game"));
        assert_eq!(q.pending(), 0);
    }

    #[test]
    fn download_progress_at_most_every_fifteen_seconds() {
        let clock = FakeClock::baseline();
        let mut q = AnnounceQueue::default();
        let mut sent = 0;
        for _ in 0..60 {
            if q.throttle(
                "download",
                format!("Downloading, {sent}"),
                Politeness::Polite,
                secs(&clock),
                DOWNLOAD_EVERY_S,
            ) {
                sent += 1;
            }
            clock.advance(Duration::from_secs(1));
        }
        assert_eq!(sent, 4);
    }

    #[test]
    fn the_status_node_carries_the_message() {
        let ctx = Context::default();
        let mut node = None;
        ctx.enable_accesskit();
        for _ in 0..2 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                Announcer::announce(ui.ctx(), "Saved", Politeness::Polite);
                Announcer::show(ui);
            });
            out.textures_delta.clear();
            if let Some(update) = out.platform_output.accesskit_update {
                node = update.nodes.into_iter().find_map(|(_, n)| {
                    (n.role() == Role::Status).then(|| (n.label().map(str::to_owned), n.live()))
                });
            }
        }
        assert_eq!(Announcer::current(&ctx).as_deref(), Some("Saved"));
        assert_eq!(node, Some((Some("Saved".to_owned()), Some(Live::Polite))));
    }
}

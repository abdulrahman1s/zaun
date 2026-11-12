//! Helpers that attach accessible names and roles; live-region announcements. See MODULES §8
//! and ACCESSIBILITY §4.
//!
//! [`Announcer`] keeps one visually hidden status node whose text is replaced to trigger an
//! announcement; messages are queued, de-duplicated and rate-limited (ACCESSIBILITY §4.3).

use std::collections::VecDeque;

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

#[derive(Clone, Debug, Default)]
struct AnnouncerState {
    queue: VecDeque<(String, Politeness)>,
    current: Option<(String, Politeness)>,
    last_change: Option<f64>,
}

/// The single live region of the window.
pub struct Announcer;

impl Announcer {
    fn id() -> Id {
        Id::unique("easyplay.announcer")
    }

    /// Queue `text`. Duplicates of the queued or current message are dropped.
    pub fn announce(ctx: &Context, text: impl Into<String>, politeness: Politeness) {
        let text = text.into();
        ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<AnnouncerState>(Self::id());
            let dup = s.current.as_ref().is_some_and(|(t, _)| *t == text)
                || s.queue.iter().any(|(t, _)| *t == text);
            if !dup {
                if politeness == Politeness::Assertive {
                    s.queue.push_front((text, politeness));
                } else {
                    s.queue.push_back((text, politeness));
                }
            }
        });
        ctx.request_repaint();
    }

    /// The message currently exposed (tests).
    pub fn current(ctx: &Context) -> Option<String> {
        ctx.data(|d| d.get_temp::<AnnouncerState>(Self::id()))
            .and_then(|s| s.current.map(|(t, _)| t))
    }

    /// Advance the queue and paint the hidden status node. Call once per frame.
    pub fn show(ui: &mut Ui) {
        let now = ui.input(|i| i.time);
        let ctx = ui.ctx().clone();
        let (current, wait) = ctx.data_mut(|d| {
            let s = d.get_temp_mut_or_default::<AnnouncerState>(Self::id());
            let elapsed = s.last_change.map_or(f64::INFINITY, |t| now - t);
            let mut wait = None;
            if !s.queue.is_empty() {
                if elapsed >= f64::from(ANNOUNCE_INTERVAL) {
                    s.current = s.queue.pop_front();
                    s.last_change = Some(now);
                    if !s.queue.is_empty() {
                        wait = Some(ANNOUNCE_INTERVAL);
                    }
                } else {
                    wait = Some((f64::from(ANNOUNCE_INTERVAL) - elapsed) as f32);
                }
            }
            (s.current.clone(), wait)
        });
        if let Some(w) = wait {
            ctx.request_repaint_after_secs(w);
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

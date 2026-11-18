//! Start the overlay with a game and stop it when the game ends; after a crash, start it
//! again with the Chat engine's back-off and give up after as many exits in a row
//! (ADR-0037 §5). Every start has a generation: events of an overlay already replaced or
//! stopped are dropped, so a late exit never restarts anything.
use super::state::{OverlayDone, OverlayState};
use crate::chat::overlay::protocol::{EXIT_NO_GPU, EXIT_UNSUPPORTED};
use crate::chat::overlay::{OverlaySink, OverlayStatus};
use crate::model::{ChatStatus, SessionKind};
use crate::service::flows::chat::{MAX_FAILURES, backoff};
use crate::service::prelude::*;

impl AppService {
    /// Chat runs (starting or ready), `chat.overlay` is on, this desktop can show one, and a
    /// game runs.
    fn overlay_wanted(&self) -> bool {
        let p = &self.prefs.chat;
        p.enabled
            && p.overlay
            && matches!(
                self.flow.chat.snapshot.status,
                ChatStatus::Starting | ChatStatus::Ready
            )
            && self.deps.overlay.as_ref().is_some_and(|f| f.available())
            && self.flow.overlay.layer_shell != Some(false)
            && self
                .sessions
                .values()
                .any(|s| s.kind == SessionKind::Game && s.is_open())
    }

    /// Start or stop the overlay to match the setting and the running games, and send it
    /// what changed. Called whenever sessions, preferences or Chat change.
    pub(crate) fn overlay_sync(&mut self) {
        self.overlay_ask_desktop();
        if !self.prefs.chat.overlay {
            // Turning it off and on again tries again on a desktop that could not show it.
            self.flow.overlay.cannot = None;
        }
        if !self.overlay_wanted() || self.flow.overlay.cannot.is_some() {
            let st = &self.flow.overlay;
            if st.overlay.is_some() || st.restarting {
                tracing::info!("overlay: stopped (no game, Chat stopped, or turned off)");
            }
            // The next game, or the setting turned on again, gives it a new chance.
            if st.overlay.is_some() || st.restarting || st.given_up || st.failures > 0 {
                self.overlay_stop(false);
            }
            self.overlay_publish_status();
            return;
        }
        let st = &self.flow.overlay;
        if st.overlay.is_none() && !st.given_up && !st.restarting {
            self.overlay_start();
        }
        self.overlay_push();
        self.overlay_publish_status();
    }

    fn overlay_start(&mut self) {
        let Some(factory) = self.deps.overlay.clone() else {
            return;
        };
        let st = &mut self.flow.overlay;
        st.generation += 1;
        let generation = st.generation;
        let post = self.poster();
        let sink: OverlaySink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Overlay(
                OverlayDone::Event { generation, event },
            ))));
        });
        match factory.start(sink) {
            Ok(o) => {
                tracing::info!(generation, "overlay: started");
                self.flow.overlay.overlay = Some(o);
                if self.flow.overlay.chat.is_none() {
                    self.overlay_default_chat();
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "overlay: not started");
                self.flow.overlay.given_up = true;
            }
        }
    }

    /// Stop it (dropping it closes its input). `keep_verdict`: an overlay given up on stays
    /// given up (otherwise the next start is a fresh one).
    pub(super) fn overlay_stop(&mut self, keep_verdict: bool) {
        let st = &mut self.flow.overlay;
        let (given_up, failures) = if keep_verdict {
            (st.given_up, st.failures)
        } else {
            (false, 0)
        };
        *st = OverlayState {
            generation: st.generation + 1,
            next_toast: st.next_toast,
            cannot: st.cannot,
            asked: st.asked,
            layer_shell: st.layer_shell,
            given_up,
            failures,
            ..OverlayState::default()
        };
    }

    pub(super) fn on_overlay_exited(&mut self, status: Option<i32>, ready: bool) {
        tracing::warn!(?status, ready, "overlay: the overlay stopped");
        let st = &mut self.flow.overlay;
        st.overlay = None;
        st.open = false;
        st.sent = None;
        // A new process holds no pictures, and must not show the old toasts again.
        st.pictures.clear();
        st.toasts.clear();
        if !self.overlay_wanted() {
            self.overlay_publish_status();
            return;
        }
        let cannot = match status {
            Some(EXIT_UNSUPPORTED) => Some(OverlayStatus::Unsupported),
            Some(EXIT_NO_GPU) => Some(OverlayStatus::NoGpu),
            _ => None,
        };
        if let Some(why) = cannot {
            // The desktop does not change between games: not tried again until the setting
            // is turned off and on.
            tracing::warn!(
                ?why,
                "overlay: this desktop cannot show it; not started again"
            );
            self.flow.overlay.cannot = Some(why);
            self.overlay_give_up();
            return;
        }
        let st = &mut self.flow.overlay;
        if ready {
            st.failures = 0;
        }
        st.failures += 1;
        if st.failures >= MAX_FAILURES {
            tracing::warn!(failures = st.failures, "overlay: keeps stopping; given up");
            self.overlay_give_up();
            return;
        }
        let wait = backoff(st.failures);
        let wait = self
            .deps
            .overlay
            .as_ref()
            .map_or(wait, |f| f.restart_wait(wait));
        st.restarting = true;
        let generation = st.generation;
        self.overlay_after(wait, OverlayDone::Restart { generation });
        self.overlay_publish_status();
    }

    /// Not started again until the game ends or the setting changes.
    pub(super) fn overlay_give_up(&mut self) {
        self.flow.overlay.given_up = true;
        self.overlay_stop(true);
        self.overlay_publish_status();
    }

    /// What the hotkey and Settings read (`SnapshotHub::overlay`).
    pub(super) fn overlay_publish_status(&self) {
        let st = &self.flow.overlay;
        let status = if st.overlay.is_some() || st.restarting {
            OverlayStatus::Running
        } else if let Some(why) = st.cannot {
            why
        } else if st.layer_shell == Some(false) {
            OverlayStatus::Unsupported
        } else if st.given_up {
            OverlayStatus::Failing
        } else if !self.deps.overlay.as_ref().is_some_and(|f| f.available()) {
            OverlayStatus::Missing
        } else {
            OverlayStatus::Idle
        };
        if *self.hub.overlay.load() != status {
            self.hub.overlay.store(Arc::new(status));
        }
    }

    /// Once, at the service's start: ask the desktop on the job pool whether it has a layer
    /// shell, so a desktop that cannot show the overlay (GNOME) is known before any game, and
    /// Settings dims its rows even while the overlay is off.
    fn overlay_ask_desktop(&mut self) {
        if self.flow.overlay.asked {
            return;
        }
        self.flow.overlay.asked = true;
        let Some(factory) = self.deps.overlay.clone().filter(|f| f.available()) else {
            return;
        };
        let post = self.poster();
        // The answer comes back as a message; the handle is not needed.
        let _ = self.deps.jobs.submit(
            JobPriority::Background,
            JobLabel("overlay-layer-shell"),
            move |_| {
                let has = factory.has_layer_shell();
                post(Internal::Flow(Box::new(FlowDone::Overlay(
                    OverlayDone::LayerShell(has),
                ))));
            },
        );
    }

    /// Post `done` after `wait`, from a thread of its own.
    pub(super) fn overlay_after(&self, wait: Duration, done: OverlayDone) {
        let post = self.poster();
        let spawned = std::thread::Builder::new()
            .name("overlay-timer".into())
            .spawn(move || {
                std::thread::sleep(wait);
                post(Internal::Flow(Box::new(FlowDone::Overlay(done))));
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "overlay: timer not started");
        }
    }
}

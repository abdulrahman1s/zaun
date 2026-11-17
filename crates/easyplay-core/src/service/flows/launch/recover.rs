//! Session registry: recover dead supervisors and apply the watcher's reports (SVC-005, PROCESS_LIFECYCLE §9).
use crate::launch::LaunchState;
use crate::model::{Activity, LaunchStateTag, Session, SessionState};
use crate::service::prelude::*;
use crate::service::session_watch::SessionPoll;

fn activity_of(s: &Session) -> Activity {
    match s.state {
        SessionState::Starting => Activity::Launching(LaunchStateTag::Starting),
        SessionState::Running => Activity::Running {
            since: s.started_at,
        },
        SessionState::Lingering => Activity::Lingering,
        SessionState::Stopping => Activity::Stopping,
        SessionState::Ended => Activity::Idle,
    }
}

impl AppService {
    /// Close rows whose supervisor died (PROCESS_LIFECYCLE §9) and re-read the games.
    pub(crate) fn recover_sessions(&mut self) {
        let recovered = crate::session::recover(
            self.lib.db_mut(),
            &*self.deps.proc,
            &*self.deps.fs,
            &self.deps.paths,
            &*self.deps.clock,
            &crate::session::RecoveryOptions::default(),
        );
        match recovered {
            Ok(r) if !r.is_empty() => {
                for s in &r {
                    tracing::info!(session = %s.id, reason = s.reason.as_str(), "recovered");
                }
                self.poke_watcher();
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %e, "recovery failed"),
        }
    }

    /// A watcher report: update the registry, activities and statistics.
    pub(crate) fn on_sessions(&mut self, p: SessionPoll) {
        // Supervisors that died leave open rows: recover them.
        let dead = p.open.iter().any(|s| {
            crate::session::stop::supervisor_identity(s)
                .is_some_and(|id| !self.deps.proc.is_alive(id).unwrap_or(true))
        });
        let previous: Vec<SessionId> = self.sessions.keys().copied().collect();
        self.sessions = p.open.iter().map(|s| (s.id, s.clone())).collect();
        let mut changed = self.track_open(&p.open);
        self.drop_vanished(&previous, &p, &mut changed);
        if !p.ended.is_empty() {
            self.after_session_ended();
        }
        self.settle_ended(&p.ended, &mut changed);
        for g in &changed {
            if let Err(e) = self.lib.refresh_game(*g) {
                tracing::debug!(error = %e, "game refresh after session change failed");
            }
        }
        self.publish_sessions();
        if !changed.is_empty() {
            self.publish_library();
        }
        if dead {
            self.recover_sessions();
        }
    }

    /// Activities of the open sessions; returns the games whose activity changed.
    fn track_open(&mut self, open: &[Session]) -> Vec<GameId> {
        let mut changed = Vec::new();
        for s in open {
            let Some(g) = s.game_id else { continue };
            let a = activity_of(s);
            if self.lib.overlays().activity_of(g) != a {
                self.set_activity(g, a);
                changed.push(g);
            }
            if s.state == SessionState::Running
                && let Some((game, launch)) = self.launches.get(&s.id).copied()
            {
                self.emit(Event::LaunchProgress {
                    game,
                    launch,
                    state: LaunchState::Running { session: s.id },
                });
            }
        }
        changed
    }

    /// Sessions that left the open set without an end row (recovered elsewhere).
    fn drop_vanished(
        &mut self,
        previous: &[SessionId],
        p: &SessionPoll,
        changed: &mut Vec<GameId>,
    ) {
        for id in previous {
            if !self.sessions.contains_key(id)
                && !p.ended.iter().any(|s| s.id == *id)
                && let Some((g, _)) = self.launches.remove(id)
            {
                self.set_activity(g, Activity::Idle);
                changed.push(g);
            }
        }
    }
}

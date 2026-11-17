//! `CancelLaunch`: drop a launch before its hand-off (SVC-005, LOGIC_SPECS §3 "Cancellation").
use crate::service::prelude::*;

impl AppService {
    /// Before the hand-off the launch is dropped (a session that starts anyway is stopped at
    /// once); from `Starting` on it is `StopSession`.
    pub(crate) fn cancel_launch(&mut self, id: GameId) {
        if let Some(launch) = self.launching.get(&id).copied() {
            if self.flow.cancelled.insert(launch) {
                self.set_activity(id, Activity::Idle);
                self.publish_library();
                self.emit(Event::LaunchProgress {
                    game: id,
                    launch,
                    state: LaunchState::Cancelled,
                });
                self.notice(Notice::LaunchCancelled { game: id });
            }
            return;
        }
        if let Some(session) = self.running_session(id) {
            self.stop_session(session, StopMode::Graceful);
        }
    }

    /// A launch job finished after its launch was cancelled: release it, and stop a
    /// session that started anyway. Returns whether `launch` was cancelled.
    pub(super) fn settle_cancelled(
        &mut self,
        game: GameId,
        launch: LaunchId,
        result: Option<&SessionId>,
    ) -> bool {
        if !self.flow.cancelled.remove(&launch) {
            return false;
        }
        self.lib.end_flow(game);
        self.launching.remove(&game);
        if let Some(session) = result {
            if let Ok(Some(row)) = self.lib.db_mut().session(*session)
                && let Err(e) = self.deps.launcher.request_stop(&row, StopMode::Force)
            {
                tracing::warn!(error = %e, "stopping a cancelled launch failed");
            }
            self.watch(*session);
            self.poke_watcher();
        }
        self.set_activity(game, Activity::Idle);
        self.publish_library();
        // "Stop and remove" of a game that was still starting; with a session, its end
        // removes the game (`on_sessions`).
        if result.is_none()
            && let Some(options) = self.remove_after_stop.remove(&game)
            && let Err(e) = self.remove_game(game, options)
        {
            self.fail(e);
        }
        true
    }
}

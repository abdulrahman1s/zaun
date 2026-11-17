//! Sessions that ended: idle the game, close the launch's progress, finish a parked removal (SVC-005).
use crate::model::{Activity, EndReason, Session};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn settle_ended(&mut self, ended: &[Session], changed: &mut Vec<GameId>) {
        for s in ended {
            let Some(g) = s.game_id else { continue };
            self.set_activity(g, Activity::Idle);
            changed.push(g);
            if let Some((game, launch)) = self.launches.remove(&s.id) {
                let state = self.ended_state(s, game);
                self.emit(Event::LaunchProgress {
                    game,
                    launch,
                    state,
                });
            }
            if let Some(opts) = self.remove_after_stop.remove(&g)
                && let Err(e) = self.remove_game(g, opts)
            {
                self.fail(e);
            }
        }
    }

    fn ended_state(&mut self, s: &Session, game: GameId) -> LaunchState {
        match (s.end_reason, self.early_failure(s, game)) {
            (_, Some(u)) => LaunchState::Failed(Box::new(u)),
            (Some(EndReason::FailedToStart { code }), None) => {
                let u = self.describe(&Error::new(code).with("session", s.id), Some(game));
                LaunchState::Failed(Box::new(u))
            }
            (Some(reason), None) => LaunchState::Finished { reason },
            (None, None) => LaunchState::Finished {
                reason: EndReason::Interrupted,
            },
        }
    }
}

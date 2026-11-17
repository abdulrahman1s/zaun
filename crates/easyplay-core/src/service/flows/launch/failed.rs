//! How a launch ends without a session: failed, or parked behind a question (SVC-005).
use crate::launch::LaunchState;
use crate::model::Activity;
use crate::runtime::ProvisionNeed;
use crate::service::prelude::*;

/// The error of a launch that needs a Provision or Prepare session.
pub fn not_installed(need: &ProvisionNeed) -> Error {
    let mut e = err!(RtNotInstalled, protonpath = need.protonpath());
    if let ProvisionNeed::Slr { variant, build } = need {
        e = e.with("variant", variant.as_str()).with("build", build);
    }
    e
}

impl AppService {
    /// The launch stopped to ask a question. For the UI it is over: an answer that goes on
    /// starts a new launch, and without this the game would show its last phase for good.
    pub(super) fn launch_parked(&mut self, game: GameId, launch: LaunchId) {
        self.set_activity(game, Activity::Idle);
        self.publish_library();
        self.emit(Event::LaunchProgress {
            game,
            launch,
            state: LaunchState::Cancelled,
        });
    }

    pub(super) fn launch_failed(&mut self, game: GameId, launch: LaunchId, err: &Error) {
        tracing::info!(game = %game, code = err.code.as_str(), "launch failed: {err}");
        let u = self.describe(err, Some(game));
        self.set_activity(game, Activity::Idle);
        self.publish_library();
        self.emit(Event::LaunchProgress {
            game,
            launch,
            state: LaunchState::Failed(Box::new(u)),
        });
    }
}

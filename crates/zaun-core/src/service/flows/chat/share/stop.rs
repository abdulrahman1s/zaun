//! Stop sharing (ADR-0036): by the user, when the game ends, when Chat stops, or when the
//! friend is blocked or removed. Streams end with the game they show.
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn share_stop(&mut self) {
        if self.flow.chat.share.mine.is_some() {
            tracing::info!("stream: the user stopped sharing");
            self.share_close(true);
        }
    }

    /// The open sessions changed: a share whose game is no longer running ends.
    pub(crate) fn share_sessions_changed(&mut self) {
        let Some(game) = self.flow.chat.share.mine.as_ref().map(|s| s.game) else {
            return;
        };
        let running = self
            .sessions
            .values()
            .any(|s| s.game_id == Some(game) && s.is_open());
        if !running {
            tracing::info!(%game, "stream: the shared game ended");
            self.share_close(true);
        }
    }

    /// The friend is blocked or removed: no stream with them goes on.
    pub(crate) fn stream_drop_friend(&mut self, friend: i64) {
        if self
            .flow
            .chat
            .share
            .mine
            .as_ref()
            .is_some_and(|s| s.friend == friend)
        {
            self.share_close(true);
        }
        self.their_close(friend, true, false);
    }

    /// Chat stopped or its engine went away: every stream is gone without a word.
    pub(crate) fn stream_reset(&mut self) {
        let st = &mut self.flow.chat.share;
        st.mine = None;
        st.theirs.clear();
        st.watched = None;
        self.hub.frames.clear();
        let live = &mut self.flow.chat.snapshot.live;
        live.share = None;
        live.watch = None;
        live.stream = Default::default();
    }
}

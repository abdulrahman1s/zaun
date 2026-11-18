//! Share the running game with a friend (ADR-0036): the portal asks for the game's window,
//! then the friend's app is offered the stream.
use super::state::{MyShare, SharePhase};
use crate::chat::media::{ShareConfig, ice_servers};
use crate::config::schema::{ShareFps, ShareQuality};
use crate::model::{Session, SessionKind};
use crate::service::prelude::*;

/// `chat.share_quality` as a height at most (`None`: as in the game).
pub(super) fn share_height(q: ShareQuality) -> Option<u32> {
    match q {
        ShareQuality::P720 => Some(720),
        ShareQuality::P1080 => Some(1080),
        ShareQuality::P1440 => Some(1440),
        ShareQuality::Source => None,
    }
}

/// `chat.share_fps` as frames a second.
pub(super) fn share_fps(f: ShareFps) -> u32 {
    match f {
        ShareFps::F30 => 30,
        ShareFps::F60 => 60,
    }
}

impl AppService {
    pub(crate) fn share_start(&mut self, chat: ChatRef) -> Result<()> {
        let snap = &self.flow.chat.snapshot;
        if !snap.features.share {
            tracing::debug!(?chat, "stream: not shared (sharing is off)");
            return Ok(());
        }
        if self.flow.chat.share.mine.is_some() {
            tracing::debug!(?chat, "stream: already sharing (one share at a time)");
            return Ok(());
        }
        let ChatRef::Direct(friend) = chat else {
            bail!(ChtStreamFailed, detail = "streams are one to one for now");
        };
        let ready = snap.friend(friend).is_some_and(|f| f.ready && !f.blocked);
        ensure!(
            ready,
            ChtStreamFailed,
            detail = "not a friend who can watch"
        );
        // The engine holds one call per friend: a stream would end the call.
        let in_call = snap.live.call.as_ref().is_some_and(|c| c.chat == chat)
            || snap.live.ringing.as_ref().is_some_and(|r| r.chat == chat);
        ensure!(
            !in_call,
            ChtStreamFailed,
            detail = "sharing in a call with the same friend is not carried yet"
        );
        // One stream per friend, either way (the same call slot).
        ensure!(
            !self.flow.chat.share.theirs.contains_key(&friend),
            ChtStreamFailed,
            detail = "the friend is sharing a game with the user"
        );
        let Some(game) = self.running_game() else {
            bail!(ChtNoGameRunning);
        };
        let (Some(id), title, pid) = (game.game_id, game.game_name.clone(), game.sandbox_pid)
        else {
            bail!(ChtNoGameRunning);
        };
        let p = &self.prefs.chat;
        let config = ShareConfig {
            title: title.clone(),
            game: id.to_string(),
            pids: pid.into_iter().collect(),
            max_height: share_height(p.share_quality),
            fps: share_fps(p.share_fps),
            hardware: p.share_hw_encode,
            sound: p.share_sound,
            ice: ice_servers(&p.ice_servers),
            relay_only: p.relay_calls,
        };
        let number = self.stream_number();
        let media = self
            .stream_factory()?
            .share(config, self.stream_sink(number))?;
        tracing::info!(friend, game = %id, "stream: choosing the game's window");
        self.flow.chat.share.mine = Some(MyShare {
            number,
            friend,
            game: id,
            title,
            media,
            phase: SharePhase::Choosing,
            since: self.deps.clock.now(),
            watched: false,
            paused: false,
            sending: None,
        });
        Ok(())
    }

    /// The game to share: the newest open game session.
    fn running_game(&self) -> Option<&Session> {
        self.sessions
            .values()
            .filter(|s| s.kind == SessionKind::Game && s.is_open() && s.game_id.is_some())
            .max_by_key(|s| s.started_at)
    }
}

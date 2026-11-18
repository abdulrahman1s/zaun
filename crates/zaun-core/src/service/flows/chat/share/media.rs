//! What a stream's media reports (ADR-0036): the share's window is chosen, descriptions go
//! to the friend, the friend watches or stops, the picture pauses, and failures end the
//! stream. Events of a stream that is over are ignored.
use super::state::SharePhase;
use crate::chat::Request;
use crate::chat::media::{StreamEvent, StreamRequest};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_stream_media(&mut self, number: u64, event: StreamEvent) -> Result<()> {
        let st = &self.flow.chat.share;
        if st.mine.as_ref().is_some_and(|s| s.number == number) {
            return self.on_share_media(event);
        }
        let Some(friend) = st
            .theirs
            .iter()
            .find(|(_, t)| t.number == number)
            .map(|(f, _)| *f)
        else {
            return Ok(());
        };
        self.on_their_media(friend, event)
    }

    fn on_share_media(&mut self, event: StreamEvent) -> Result<()> {
        let Some(s) = self.flow.chat.share.mine.as_mut() else {
            return Ok(());
        };
        let friend = s.friend;
        match event {
            StreamEvent::Ready => {
                if s.phase != SharePhase::Choosing {
                    return Ok(());
                }
                s.phase = SharePhase::Offered;
                s.since = self.deps.clock.now();
                tracing::info!(friend, "stream: sharing; offering it to the friend");
                self.chat_request(Request::Stream(StreamRequest::Invite(friend)));
            }
            StreamEvent::Description(sdp) => {
                self.chat_request(Request::Stream(StreamRequest::Answer { friend, sdp }));
                return Ok(());
            }
            StreamEvent::Connected => {
                if s.phase == SharePhase::Connecting {
                    s.phase = SharePhase::Live;
                    self.chat_request(Request::Stream(StreamRequest::Connected(friend)));
                }
            }
            StreamEvent::Watching(on) => s.watched = on,
            StreamEvent::Paused(on) => s.paused = on,
            StreamEvent::Sending(info) => {
                tracing::info!(?info, "stream: sending");
                s.sending = Some(info);
            }
            StreamEvent::Interrupted | StreamEvent::Title(_) => return Ok(()),
            StreamEvent::Ended => {
                tracing::info!(friend, "stream: the game's window went away");
                self.share_close(true);
                return Ok(());
            }
            StreamEvent::Failed { code, detail } => {
                tracing::warn!(
                    friend,
                    code = code.as_str(),
                    detail,
                    "stream: sharing failed"
                );
                self.share_close(true);
                return Err(Error::new(code).with("detail", detail));
            }
        }
        self.stream_publish();
        Ok(())
    }

    fn on_their_media(&mut self, friend: i64, event: StreamEvent) -> Result<()> {
        let st = &mut self.flow.chat.share;
        let watched = st.watched == Some(friend);
        let Some(t) = st.theirs.get_mut(&friend) else {
            return Ok(());
        };
        match event {
            StreamEvent::Description(sdp) => {
                self.chat_request(Request::Stream(StreamRequest::Offer { friend, sdp }));
                return Ok(());
            }
            StreamEvent::Connected => {
                if !t.connected {
                    t.connected = true;
                    self.chat_request(Request::Stream(StreamRequest::Connected(friend)));
                }
            }
            StreamEvent::Title(title) => t.title = Some(title),
            StreamEvent::Paused(on) => t.paused = on,
            StreamEvent::Ready
            | StreamEvent::Interrupted
            | StreamEvent::Watching(_)
            | StreamEvent::Sending(_) => {
                return Ok(());
            }
            StreamEvent::Ended => {
                self.their_close(friend, true, true);
                return Ok(());
            }
            StreamEvent::Failed { code, detail } => {
                tracing::warn!(
                    friend,
                    code = code.as_str(),
                    detail,
                    "stream: a friend's stream failed"
                );
                self.their_close(friend, true, false);
                // Only a stream the user asked to see is worth a word.
                return if watched {
                    Err(Error::new(code).with("detail", detail))
                } else {
                    Ok(())
                };
            }
        }
        self.stream_publish();
        Ok(())
    }

    /// A stream that should be connected by now and is not has failed.
    pub(super) fn on_stream_timeout(&mut self, number: u64) -> Result<()> {
        let st = &self.flow.chat.share;
        if let Some(s) = st.mine.as_ref().filter(|s| s.number == number) {
            if s.phase == SharePhase::Connecting {
                self.share_close(true);
                bail!(
                    ChtStreamFailed,
                    detail = "the friend's app could not be reached"
                );
            }
            return Ok(());
        }
        let late = st
            .theirs
            .iter()
            .find(|(_, t)| t.number == number && !t.connected)
            .map(|(f, _)| *f);
        if let Some(friend) = late {
            let watched = st.watched == Some(friend);
            self.their_close(friend, true, false);
            ensure!(
                !watched,
                ChtStreamFailed,
                detail = "the friend's app could not be reached"
            );
        }
        Ok(())
    }
}

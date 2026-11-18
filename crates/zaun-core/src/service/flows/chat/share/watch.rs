//! Watch a friend's stream (ADR-0036): their app already took it in watch-only mode; watching
//! asks for the picture and points the stage at it. One stream is watched at a time.
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn watch_start(&mut self, chat: ChatRef) {
        let ChatRef::Direct(friend) = chat else {
            tracing::debug!(?chat, "stream: streams are one to one for now");
            return;
        };
        let st = &mut self.flow.chat.share;
        if st.watched == Some(friend) || !st.theirs.contains_key(&friend) {
            return;
        }
        if let Some(t) = st.watched.and_then(|f| st.theirs.get(&f)) {
            t.media.watch(false);
        }
        self.hub.frames.clear();
        st.watched = Some(friend);
        st.muted = false;
        if let Some(t) = st.theirs.get(&friend) {
            t.media.mute(false);
            t.media.watch(true);
        }
        self.flow.chat.snapshot.live.stream.ended = None;
        tracing::info!(friend, "stream: watching");
        self.stream_publish();
    }

    pub(crate) fn watch_stop(&mut self) {
        let st = &mut self.flow.chat.share;
        if let Some(t) = st.watched.take().and_then(|f| st.theirs.get(&f)) {
            t.media.watch(false);
        }
        self.hub.frames.clear();
        self.flow.chat.snapshot.live.stream.ended = None;
        self.stream_publish();
    }

    pub(crate) fn watch_mute(&mut self, on: bool) {
        let st = &mut self.flow.chat.share;
        let Some(t) = st.watched.and_then(|f| st.theirs.get(&f)) else {
            return;
        };
        t.media.mute(on);
        st.muted = on;
        self.stream_publish();
    }
}

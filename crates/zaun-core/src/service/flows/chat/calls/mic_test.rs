//! The microphone test (Settings › Chat, "Let's check"; ADR-0035): the chosen microphone,
//! its level on the page, and its sound played back half a second late on the chosen
//! speakers. Started only when asked; a call starting, Chat stopping or `MicTest(false)`
//! stops it.
use super::state::CallDone;
use crate::chat::media::{MicTestEvent, MicTestSink};
use crate::service::flows::chat::ChatDone;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn mic_test(&mut self, on: bool) -> Result<()> {
        if !on {
            if self.mic_test_stop() {
                self.chat_publish();
            }
            return Ok(());
        }
        if self.flow.chat.mic_test.is_some() || self.flow.chat.call.is_some() {
            tracing::debug!("call: no microphone test (one runs, or a call has the devices)");
            return Ok(());
        }
        let Some(factory) = self.deps.media.clone() else {
            bail!(ChtMediaFailed, detail = "this build has no call media");
        };
        self.flow.chat.next_call += 1;
        let number = self.flow.chat.next_call;
        let post = self.poster();
        let sink: MicTestSink = Arc::new(move |event| {
            post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Call(
                CallDone::MicTest { number, event },
            )))));
        });
        let test = factory.mic_test(self.audio_settings(), sink)?;
        self.flow.chat.mic_test = Some((number, test));
        self.flow.chat.snapshot.live.mic_test = Some(0);
        self.chat_publish();
        Ok(())
    }

    /// Stop the test if it runs (the microphone and speakers close). Whether one ran.
    pub(super) fn mic_test_stop(&mut self) -> bool {
        self.flow.chat.snapshot.live.mic_test = None;
        self.flow.chat.mic_test.take().is_some()
    }

    pub(super) fn on_mic_test(&mut self, number: u64, event: MicTestEvent) -> Result<()> {
        if self
            .flow
            .chat
            .mic_test
            .as_ref()
            .is_none_or(|(n, _)| *n != number)
        {
            return Ok(());
        }
        match event {
            MicTestEvent::Level(level) => {
                let live = &mut self.flow.chat.snapshot.live;
                // Two points of change are not worth a new snapshot.
                if live.mic_test.is_some_and(|l| l.abs_diff(level) < 2) {
                    return Ok(());
                }
                live.mic_test = Some(level);
                self.chat_publish();
                Ok(())
            }
            MicTestEvent::Failed { code, detail } => {
                tracing::warn!(
                    code = code.as_str(),
                    detail,
                    "call: the microphone test failed"
                );
                self.mic_test_stop();
                self.chat_publish();
                Err(Error::new(code).with("detail", detail))
            }
        }
    }
}

//! List the microphones and speakers for Settings (ADR-0035): asked of the sound server on
//! the job pool, published in `ChatSnapshot::audio_devices`.
use super::state::CallDone;
use crate::model::AudioDevices;
use crate::service::flows::chat::ChatDone;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn audio_devices_refresh(&mut self) {
        let Some(factory) = self.deps.media.clone() else {
            tracing::debug!("call: no call media, so no device list");
            return;
        };
        let post = self.poster();
        // The job pool may be shutting down; then nobody is waiting for the list either.
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("audio-devices"),
            move |_| {
                let r = factory.devices();
                post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Call(
                    CallDone::Devices(r),
                )))));
            },
        );
    }

    pub(super) fn on_audio_devices(&mut self, r: Result<AudioDevices>) -> Result<()> {
        let devices = r.unwrap_or_else(|e| {
            // Settings then offers the system default only; nothing to tell the user.
            tracing::info!(
                code = e.code.as_str(),
                "call: the sound devices could not be listed: {e}"
            );
            AudioDevices::default()
        });
        self.flow.chat.snapshot.audio_devices = Some(devices);
        self.chat_publish();
        Ok(())
    }
}

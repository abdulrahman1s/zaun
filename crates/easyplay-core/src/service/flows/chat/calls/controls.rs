//! Mute the microphone, deafen the speakers (ADR-0035).
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn call_mute(&mut self, on: bool) {
        let Some(c) = &mut self.flow.chat.call else {
            return;
        };
        c.muted = on;
        if let Some(m) = &c.media {
            m.set_muted(on);
        }
        self.call_publish();
    }

    pub(crate) fn call_deafen(&mut self, on: bool) {
        let Some(c) = &mut self.flow.chat.call else {
            return;
        };
        c.deafened = on;
        if let Some(m) = &c.media {
            m.set_deafened(on);
        }
        self.call_publish();
    }
}

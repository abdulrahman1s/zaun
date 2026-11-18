//! Change the shared picture's height and frame rate (ADR-0036): kept in the preferences
//! for the next share, and applied to the running one at once (the media reopens its
//! encoder and sends a keyframe; the portal and the connection stay).
use super::start::{share_fps, share_height};
use crate::config::PreferencePatch;
use crate::config::schema::ShareFps;
use crate::service::ShareHeight;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn share_set_quality(&mut self, height: ShareHeight, fps: u32) -> Result<()> {
        let rate = match fps {
            30 => ShareFps::F30,
            60 => ShareFps::F60,
            _ => bail!(CfgInvalidValue, key = "chat.share_fps", value = fps),
        };
        let patch = PreferencePatch::new()
            .set("chat.share_quality", height.as_str())
            .set("chat.share_fps", rate.as_str());
        self.set_preference(&patch)?;
        if let Some(s) = &self.flow.chat.share.mine {
            tracing::info!(?height, fps, "stream: the picture changes");
            s.media.set_quality(share_height(height), share_fps(rate));
        }
        self.stream_publish();
        Ok(())
    }
}

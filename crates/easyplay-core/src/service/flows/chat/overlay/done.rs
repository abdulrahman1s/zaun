//! The overlay reported, or one of its timers is due (ADR-0037).
use super::state::OverlayDone;
use crate::chat::overlay::{FromOverlay, OverlayEvent};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn on_overlay_done(&mut self, done: OverlayDone) -> Result<()> {
        match done {
            OverlayDone::Event { generation, event } => {
                if generation == self.flow.overlay.generation {
                    return self.on_overlay_event(event);
                }
            }
            OverlayDone::Restart { generation } => {
                let st = &mut self.flow.overlay;
                if generation == st.generation && st.restarting {
                    st.restarting = false;
                    self.overlay_sync();
                }
            }
            OverlayDone::LayerShell(has) => {
                tracing::info!(?has, "overlay: the desktop's layer shell");
                self.flow.overlay.layer_shell = has;
                self.overlay_sync();
                self.overlay_publish_status();
            }
            OverlayDone::Flush { generation } => {
                let st = &mut self.flow.overlay;
                if generation == st.generation {
                    st.flush_pending = false;
                    st.last_push = None;
                    self.overlay_push();
                }
            }
        }
        Ok(())
    }

    fn on_overlay_event(&mut self, event: OverlayEvent) -> Result<()> {
        match event {
            OverlayEvent::Said(FromOverlay::Ready) => {
                tracing::info!("overlay: ready");
                // The state it was sent at its start waits in its input (pictures included);
                // send it whole again in case it came before the surface was ready.
                let st = &mut self.flow.overlay;
                st.sent = None;
                st.last_push = None;
                self.overlay_push();
                Ok(())
            }
            OverlayEvent::Said(m) => self.on_overlay_said(m),
            OverlayEvent::Incompatible(v) => {
                tracing::warn!(version = v, "overlay: speaks another protocol; stopped");
                self.overlay_give_up();
                Ok(())
            }
            OverlayEvent::Exited { status, ready } => {
                self.on_overlay_exited(status, ready);
                Ok(())
            }
        }
    }
}

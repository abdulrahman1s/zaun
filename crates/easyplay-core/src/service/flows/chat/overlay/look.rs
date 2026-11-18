//! The "Show while playing" popover: the overlay's own switches are the Settings keys
//! (one setting, two places), so they are saved like any preference.
use crate::chat::overlay::Corner;
use crate::config::PreferencePatch;
use crate::config::schema::OverlayCorner;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn overlay_set_look(
        &mut self,
        toasts: bool,
        voice_widget: bool,
        corner: Corner,
    ) -> Result<()> {
        let corner = match corner {
            Corner::TopLeft => OverlayCorner::TopLeft,
            Corner::TopRight => OverlayCorner::TopRight,
            Corner::BottomLeft => OverlayCorner::BottomLeft,
            Corner::BottomRight => OverlayCorner::BottomRight,
        };
        let patch = PreferencePatch::new()
            .set("chat.overlay_toasts", toasts)
            .set("chat.overlay_voice_widget", voice_widget)
            .set("chat.overlay_corner", corner.as_str());
        self.set_preference(&patch)
    }
}

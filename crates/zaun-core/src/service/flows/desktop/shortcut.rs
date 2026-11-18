//! `SetShortcut`: create or remove a game's applications-menu entry (DESKTOP_INTEGRATION §6).
use super::DesktopDone;
use super::integration::integration_state_shortcuts;
use crate::artwork::ImageLimits;
use crate::artwork::codec::{pad_square, resize};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn set_shortcut(&mut self, id: GameId, enable: bool) -> Result<()> {
        let snap = self.lib.snapshot();
        let g = snap
            .get(id)
            .ok_or_else(|| err!(LibInvalidField, game = id))?;
        let name = g.name.to_string();
        let icon_file = g
            .icon
            .and_then(|k| self.deps.art_index.get(&k.0))
            .map(|(p, _)| p);
        let fs = Arc::clone(&self.deps.fs);
        let codec = self.deps.codec.clone();
        self.desktop_job("desktop-shortcut", move |d| {
            let r = if enable {
                let png = icon_file.and_then(|p| {
                    let codec = codec?;
                    let bytes = fs.read(&p, ImageLimits::DEFAULT.max_bytes).ok()?;
                    let img = codec.decode(&bytes, &ImageLimits::DEFAULT).ok()?;
                    codec.encode_png(&resize(&pad_square(&img), 256, 256)).ok()
                });
                d.shortcuts
                    .create(id, &name, png.as_deref(), false)
                    .map(|_| ())
            } else {
                d.shortcuts.remove(id)
            };
            match r {
                Ok(()) => DesktopDone::State(integration_state_shortcuts(d)),
                Err(e) => DesktopDone::Failed(e),
            }
        })
    }
}

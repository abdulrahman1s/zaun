//! Import a user-chosen artwork file: decode and normalise on the pool (ART-008, LOGIC_SPECS §14).
use super::fetched::ArtFetched;
use crate::artwork::ArtSource;
use crate::artwork::ImageLimits;
use crate::artwork::import::normalise;
use crate::model::{ArtOrigin, ArtSlot};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn import_artwork(
        &mut self,
        id: GameId,
        slot: ArtSlot,
        source: ArtSource,
    ) -> Result<()> {
        match source {
            ArtSource::SteamCdn(_) => self.fetch_online_artwork(id, true),
            ArtSource::File(path) => self.import_file(id, slot, path),
            ArtSource::ExecutableIcon(exe) => self.import_program_icon(id, slot, exe),
            ArtSource::Generated => self.clear_artwork(id, slot),
        }
    }

    fn import_file(&mut self, id: GameId, slot: ArtSlot, path: PathBuf) -> Result<()> {
        let codec = self
            .deps
            .codec
            .clone()
            .ok_or_else(|| Error::not_available("artwork codec"))?;
        let fs = Arc::clone(&self.deps.fs);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("artwork-import"),
            move |_| {
                let mut out = ArtFetched::new(id, true);
                let limits = ImageLimits::DEFAULT;
                match fs.read_follow(&path, limits.max_bytes) {
                    Ok(bytes) => match normalise(&*codec, slot, &bytes, &limits) {
                        Ok(n) => out.images.push((n, ArtOrigin::File)),
                        Err(e) => out.error = Some(err!(ArtUnreadable, reason = e)),
                    },
                    Err(e) => out.error = Some(e.into()),
                }
                post(Internal::Art(Box::new(out)));
            },
        );
        Ok(())
    }
}

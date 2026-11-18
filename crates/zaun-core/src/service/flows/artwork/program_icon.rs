//! *Use program icon*: the largest icon embedded in one of the game's programs (LOGIC_SPECS §14).
use super::fetched::ArtFetched;
use crate::artwork::icon_extract::ICON_LIMITS;
use crate::artwork::import::normalise;
use crate::model::{ArtOrigin, ArtSlot};
use crate::service::prelude::*;

impl AppService {
    pub(super) fn import_program_icon(
        &mut self,
        id: GameId,
        slot: ArtSlot,
        exe: Option<ExecutableId>,
    ) -> Result<()> {
        let codec = self
            .deps
            .codec
            .clone()
            .ok_or_else(|| Error::not_available("artwork codec"))?;
        let game = self.lib.game(id)?;
        let path = game
            .executables
            .iter()
            .find(|e| Some(e.id) == exe)
            .or_else(|| game.primary())
            .map(|e| e.canonical_path.clone());
        let icons = Arc::clone(&self.deps.icons);
        let post = self.poster();
        let _ = self.deps.jobs.submit(
            JobPriority::Interactive,
            JobLabel("artwork-icon"),
            move |_| {
                let mut out = ArtFetched::new(id, true);
                match path.map(|p| icons.largest_icon(&p)) {
                    Some(Ok(Some(b))) => match normalise(&*codec, slot, &b, &ICON_LIMITS) {
                        Ok(n) => out.images.push((n, ArtOrigin::ExecutableIcon)),
                        Err(e) => out.error = Some(err!(ArtUnreadable, reason = e)),
                    },
                    Some(Err(e)) => out.error = Some(e),
                    Some(Ok(None)) | None => out.error = Some(err!(ArtNoIcon)),
                }
                post(Internal::Art(Box::new(out)));
            },
        );
        Ok(())
    }
}

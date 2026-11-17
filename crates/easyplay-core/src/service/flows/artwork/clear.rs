//! *Remove* an artwork slot: it falls back to the icon or the generated placeholder (LOGIC_SPECS §14).
use crate::model::ArtSlot;
use crate::service::prelude::*;
use crate::storage::ArtworkRepo;

impl AppService {
    /// The stored file stays until maintenance collects it.
    pub(super) fn clear_artwork(&mut self, id: GameId, slot: ArtSlot) -> Result<()> {
        let rows = self.lib.db_mut().artwork_of(id).unwrap_or_default();
        let Some(row) = rows.into_iter().find(|r| r.slot == slot) else {
            return Ok(());
        };
        self.lib.db_mut().delete_artwork(id, slot)?;
        self.deps.art_index.remove(&row.hash);
        self.index_game_art(id);
        if let Err(e) = self.lib.refresh_game(id) {
            tracing::debug!(error = %e, "refresh after artwork removal failed");
        }
        self.publish_library();
        Ok(())
    }
}

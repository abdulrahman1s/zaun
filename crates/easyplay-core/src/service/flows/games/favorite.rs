//! Favourite, hidden and tags: small library edits that republish the library.
use crate::ids::TagName;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn set_favorite(&mut self, id: GameId, favorite: bool) -> Result<()> {
        self.lib.set_favorite(&[id], favorite)?;
        self.publish_library();
        Ok(())
    }

    pub(crate) fn set_hidden(&mut self, id: GameId, hidden: bool) -> Result<()> {
        self.lib.set_hidden(&[id], hidden)?;
        self.publish_library();
        Ok(())
    }

    pub(crate) fn set_tags(&mut self, id: GameId, tags: &[TagName]) -> Result<()> {
        self.lib.set_tags(id, tags)?;
        self.publish_library();
        Ok(())
    }
}

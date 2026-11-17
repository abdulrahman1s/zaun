//! Add a program to a game from the game page (UI-018, LOGIC_SPECS §8).
use crate::model::ExeRole;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn add_program(&mut self, id: GameId, path: &Path, role: ExeRole) -> Result<()> {
        self.lib.add_executable(id, path, role)?;
        self.publish_library();
        self.notice(Notice::ProgramAdded { game: id });
        Ok(())
    }
}

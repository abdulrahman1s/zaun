//! "Set up as a new game" (LOGIC_SPECS §1, §4): the file leaves its old game, then a draft follows.
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn set_up_as_new(&mut self, path: PathBuf, origin: OpenOrigin) -> Result<()> {
        use crate::storage::ExecutableRepo;
        let owner = self.lib.db_mut().executable_by_canonical_path(&path)?;
        if let Some((game, row)) = owner {
            ensure!(!self.is_busy(game), SesAlreadyRunning, game = game);
            self.lib.begin_flow(game)?;
            let detached = self.lib.detach_program(row.id);
            self.lib.end_flow(game);
            detached?;
            self.publish_library();
        }
        self.draft_job(path, origin);
        Ok(())
    }
}

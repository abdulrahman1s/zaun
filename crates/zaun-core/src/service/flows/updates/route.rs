//! The update commands and completions to their flows.

use super::UpdatesDone;
use crate::service::command::UpdateCommand;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn update_command(&mut self, c: UpdateCommand) -> Result<()> {
        match c {
            UpdateCommand::Check => {
                self.check_updates(true);
                Ok(())
            }
            UpdateCommand::Download => self.download_update(),
            UpdateCommand::Restart => self.restart_for_update(),
            UpdateCommand::OpenNotes => self.open_release_notes(),
        }
    }

    pub(crate) fn on_updates_done(&mut self, d: UpdatesDone) -> Result<()> {
        match d {
            UpdatesDone::Due { generation } => {
                if generation == self.flow.updates.generation {
                    self.check_updates(false);
                }
                Ok(())
            }
            UpdatesDone::Checked { manual, result } => self.on_checked(manual, result),
            UpdatesDone::Downloaded(r) => self.on_downloaded(r),
        }
    }
}

//! `ScanFolder` (GAME_LIBRARY §11): list the programs under a folder, answered through its query.
use std::path::PathBuf;

use crate::library::folder_scan;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn scan_folder(&mut self, folder: PathBuf, reply_to: QueryId) -> Result<()> {
        let fs = Arc::clone(&self.deps.fs);
        let clock = Arc::clone(&self.deps.clock);
        self.answer_on_pool("scan-folder", reply_to, move |_| {
            QueryResult::Folder(folder_scan::scan(&*fs, &*clock, &folder))
        });
        Ok(())
    }
}

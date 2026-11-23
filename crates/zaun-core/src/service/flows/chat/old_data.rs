//! The old Chat engine's databases (ADR-0042): measured at start-up, deleted when the user asks.
//! The native engine cannot read them, so friends from them are added again.
use super::state::ChatDone;
use crate::service::prelude::*;

/// The libsimplex engine's databases in the Chat folder (`simplex_chat.db`, `simplex_agent.db`
/// and their journals); the native store is `chat.db` and never matches.
const OLD_DATABASES: [&str; 2] = ["simplex_chat.db", "simplex_agent.db"];
const JOURNALS: [&str; 4] = ["", "-wal", "-shm", "-journal"];
/// Its scratch folders, removed only once empty: received files stay in `files/`.
const OLD_FOLDERS: [&str; 2] = ["tmp", "assets"];

fn old_files(dir: &Path) -> impl Iterator<Item = PathBuf> + '_ {
    OLD_DATABASES
        .iter()
        .flat_map(move |db| JOURNALS.iter().map(move |j| dir.join(format!("{db}{j}"))))
}

/// The bytes of the old engine's databases in `dir`; `None` when none is there.
pub(crate) fn measure(fs: &dyn Fs, dir: &Path) -> Option<u64> {
    let sizes: Vec<u64> = old_files(dir)
        .filter_map(|p| fs.metadata(&p).ok())
        .filter(|m| m.is_file())
        .map(|m| m.size)
        .collect();
    (!sizes.is_empty()).then(|| sizes.iter().sum())
}

/// Deletes the old databases, then whatever of the scratch folders is empty.
fn forget(fs: &dyn Fs, dir: &Path) -> Result<()> {
    for p in old_files(dir) {
        if fs.exists(&p) {
            fs.remove_file(&p)?;
        }
    }
    for f in OLD_FOLDERS {
        // A folder with something in it is the user's to keep.
        let _ = fs.remove_dir(&dir.join(f));
    }
    Ok(())
}

impl AppService {
    /// Start-up: Settings offers to delete what the old engine left, with its size.
    pub(crate) fn chat_check_old_data(&mut self) {
        self.old_data_job(false);
    }

    /// `ChatCommand::ForgetOldData`: delete it, then measure again.
    pub(crate) fn chat_forget_old_data(&mut self) {
        self.old_data_job(true);
    }

    fn old_data_job(&self, delete: bool) {
        let (fs, dir, post) = (
            Arc::clone(&self.deps.fs),
            self.deps.paths.chat_dir.clone(),
            self.poster(),
        );
        let label = if delete {
            "chat-forget-old"
        } else {
            "chat-old-data"
        };
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Background, JobLabel(label), move |_| {
                let r = if delete { forget(&*fs, &dir) } else { Ok(()) };
                let r = r.map(|()| measure(&*fs, &dir));
                post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::OldData(
                    r,
                )))));
            });
    }

    pub(super) fn on_old_data(&mut self, r: Result<Option<u64>>) -> Result<()> {
        let size = r?;
        if self.flow.chat.old_data != size {
            self.flow.chat.old_data = size;
            self.chat_publish();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_old_engines_databases_are_old_data() {
        let dir = Path::new("/c");
        let names: Vec<PathBuf> = old_files(dir).collect();
        assert!(names.contains(&dir.join("simplex_chat.db-wal")));
        assert!(names.contains(&dir.join("simplex_agent.db")));
        assert!(
            !names
                .iter()
                .any(|p| p.ends_with("chat.db") || p.ends_with("files"))
        );
    }
}

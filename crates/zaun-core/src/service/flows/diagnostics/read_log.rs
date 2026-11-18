//! `ReadLog`: one chunk of a session log file for the viewer (DEV-004, SCREEN_SPECS_2 §15).
use crate::fsx::{FileType, Fs};
use crate::service::command::LogFile;
use crate::service::event::LogChunk;
use crate::service::prelude::*;

/// Largest chunk returned (the viewer asks again from the new offset).
const LOG_CHUNK: usize = 1024 * 1024;
/// The first chunk is small so the viewer shows text at once.
const FIRST_CHUNK: usize = 64 * 1024;

impl AppService {
    pub(crate) fn read_log(
        &mut self,
        session: SessionId,
        file: LogFile,
        offset: u64,
        reply_to: QueryId,
    ) -> Result<()> {
        let fs = Arc::clone(&self.deps.fs);
        let at = self.deps.paths.session_dir(session).join(file.file_name());
        self.answer_on_pool("read-log", reply_to, move |_| {
            let path = match file {
                LogFile::Proton => proton_log(&*fs, &at).unwrap_or(at),
                _ => at,
            };
            let Ok(md) = fs.metadata(&path) else {
                return QueryResult::LogChunk(LogChunk {
                    session,
                    file,
                    offset,
                    bytes: Vec::new(),
                    size: 0,
                    missing: true,
                });
            };
            let len = if offset == 0 { FIRST_CHUNK } else { LOG_CHUNK };
            let bytes = fs.read_range(&path, offset, len).unwrap_or_default();
            QueryResult::LogChunk(LogChunk {
                session,
                file,
                offset,
                bytes,
                size: md.size,
                missing: !md.is_file(),
            })
        });
        Ok(())
    }
}

/// The Proton log in `dir`: the largest `*.log` regular file (Proton names it after the game
/// ID it was given, `steam-<id>.log`).
fn proton_log(fs: &dyn Fs, dir: &Path) -> Option<PathBuf> {
    fs.read_dir(dir)
        .ok()?
        .into_iter()
        .filter(|e| e.file_type == FileType::File)
        .filter(|e| e.name.to_str().is_some_and(|n| n.ends_with(".log")))
        .filter_map(|e| Some((fs.metadata(&e.path).ok()?.size, e.path)))
        .max()
        .map(|(_, p)| p)
}

//! Save and open a received file: only files inside the Chat folder, never run (ADR-0033).
use std::path::{Path, PathBuf};

use super::state::ChatDone;
use crate::chat::{FileKind, Request};
use crate::model::ChatRef;
use crate::service::prelude::*;

impl AppService {
    /// `ChatCommand::SaveFile`: ask the engine where the file is; `on_located` copies it.
    pub(super) fn chat_save_command(
        &mut self,
        chat: ChatRef,
        item: i64,
        to: PathBuf,
    ) -> Result<()> {
        ensure!(self.flow.chat.engine.is_some(), ChtUnavailable);
        let st = &mut self.flow.chat;
        st.next_tag += 1;
        let tag = st.next_tag;
        st.saves.insert(tag, to);
        self.chat_request(Request::LocateFile { chat, item, tag });
        Ok(())
    }

    /// The engine found where a file is (for `ChatCommand::SaveFile`).
    pub(super) fn on_located(&mut self, tag: u64, path: Option<PathBuf>) -> Result<()> {
        let Some(to) = self.flow.chat.saves.remove(&tag) else {
            return Ok(());
        };
        let path =
            path.ok_or_else(|| err!(ChtSaveFailed, detail = "the file is not on this computer"))?;
        self.chat_save_file(&path, &to)
    }

    /// Copy the Chat folder's file `path` to `to` (the user's choice in a save dialog), off
    /// the service thread: into a temporary file of its own beside `to`, then renamed over
    /// it (an existing file there is replaced; the dialog asked). Reports `Notice::
    /// ChatFileSaved` or `EP-CHT-007`.
    fn chat_save_file(&mut self, path: &Path, to: &Path) -> Result<()> {
        if !self.inside_chat_dir(path) {
            tracing::warn!("chat: a file outside the Chat folder was not saved");
            bail!(ChtSaveFailed, detail = "not a file of the Chat folder");
        }
        let (Some(dir), Some(name)) = (to.parent(), to.file_name()) else {
            bail!(ChtSaveFailed, detail = "no file name");
        };
        ensure!(
            to.is_absolute(),
            ChtSaveFailed,
            detail = "not an absolute path"
        );
        self.flow.chat.next_save += 1;
        let mut part = std::ffi::OsString::from(".");
        part.push(name);
        part.push(format!(".{}.easyplay-part", self.flow.chat.next_save));
        let tmp = dir.join(part);
        let folder = dir
            .file_name()
            .map_or_else(|| dir.to_string_lossy(), |n| n.to_string_lossy())
            .into_owned();
        let fs = Arc::clone(&self.deps.fs);
        let (from, to) = (path.to_owned(), to.to_owned());
        let post = self.poster();
        std::thread::Builder::new()
            .name("chat-save".into())
            .spawn(move || {
                let r = fs
                    .copy_file(&from, &tmp, 0o644)
                    .and_then(|_| fs.rename(&tmp, &to));
                let result = r.map(|()| folder).map_err(|e| {
                    let _ = fs.remove_file(&tmp);
                    tracing::warn!(error = %e, "chat: the file was not saved");
                    err!(ChtSaveFailed, detail = e.to_string())
                });
                post(Internal::Flow(Box::new(FlowDone::Chat(ChatDone::Saved(
                    result,
                )))));
            })
            .map_err(|e| err!(IntTaskFailed).with_source(e))?;
        Ok(())
    }

    /// Open a message's file: pictures and videos with the desktop's default application,
    /// other files shown in their folder.
    pub(super) fn chat_open_file(&mut self, path: &Path, kind: FileKind) {
        if !self.inside_chat_dir(path) {
            tracing::warn!("chat: a file outside the Chat folder was not opened");
            return;
        }
        let Some(desktop) = self.deps.desktop.clone() else {
            return;
        };
        let target = crate::desktop::open_uri::KnownPath::file(path.to_owned());
        let reveal = kind == FileKind::File;
        let spawned = std::thread::Builder::new()
            .name("chat-open".into())
            .spawn(move || {
                let r = if reveal {
                    desktop.opener.reveal(&target)
                } else {
                    desktop.opener.open(&target)
                };
                if let Err(e) = r {
                    tracing::warn!(error = %e, "chat: the file was not opened");
                }
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "chat: the opener did not start");
        }
    }

    fn inside_chat_dir(&self, path: &Path) -> bool {
        let fs = &self.deps.fs;
        fs.canonicalize(path)
            .ok()
            .zip(fs.canonicalize(&self.deps.paths.chat_dir).ok())
            .is_some_and(|(p, d)| p.starts_with(&d) && p != d)
    }
}

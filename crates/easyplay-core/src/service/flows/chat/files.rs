//! Send a file or picture: copy it into the Chat folder (the engine's sandbox sees nothing
//! else), make a picture's preview, send it from a thread of its own (ADR-0033).
use std::path::{Path, PathBuf};

use super::preview::{PREVIEW_BUDGET, PREVIEW_MAX_SOURCE, preview_jpeg};
use super::state::{ChatDone, OUTBOX_DIR};
use crate::chat::{FileKind, MAX_FILE, MAX_SEND, Request};
use crate::fsx::Fs;
use crate::model::ChatRef;
use crate::service::ClipKind;
use crate::service::prelude::*;

/// A file to send: a file on this computer, or a picture from the clipboard.
pub(super) enum Outgoing {
    Path(PathBuf),
    /// Already copied into place.
    Ready(PathBuf),
    Bytes {
        bytes: Vec<u8>,
        name: String,
    },
}

impl AppService {
    pub(super) fn chat_send_path(
        &mut self,
        chat: ChatRef,
        path: PathBuf,
        text: String,
    ) -> Result<()> {
        self.chat_send_file(chat, Outgoing::Path(path), text)
    }

    pub(super) fn chat_send_clipboard(
        &mut self,
        chat: ChatRef,
        bytes: Vec<u8>,
        kind: ClipKind,
        text: String,
    ) -> Result<()> {
        let ext = match kind {
            ClipKind::Png => "png",
            ClipKind::Jpeg => "jpg",
            ClipKind::Gif => "gif",
        };
        let name = format!("pasted-{}.{ext}", self.deps.clock.now().as_millis());
        self.chat_send_file(chat, Outgoing::Bytes { bytes, name }, text)
    }

    /// Refused when files are off or the file is larger than `chat::MAX_SEND`.
    fn chat_send_file(&mut self, chat: ChatRef, src: Outgoing, text: String) -> Result<()> {
        ensure!(
            self.flow.chat.snapshot.features.files && self.flow.chat.engine.is_some(),
            ChtRequestFailed,
            detail = "files are off"
        );
        let Some((name, size)) = self.outgoing_name_and_size(&src)? else {
            return Ok(());
        };
        ensure!(
            size <= MAX_SEND,
            ChtRequestFailed,
            detail = "the file is larger than 1 GiB"
        );
        let name = safe_name(&name);
        let budget = PREVIEW_BUDGET.saturating_sub(text.len());
        let kind = file_kind(Path::new(&name));
        self.flow.chat.next_file += 1;
        let stamp = self.deps.clock.now().as_millis();
        // A folder per file keeps its name, which is what the friend sees.
        let dir = (self.deps.paths.chat_dir)
            .join(OUTBOX_DIR)
            .join(format!("{stamp}-{}", self.flow.chat.next_file));
        let src = self.copy_small_now(src, &dir, &name, size)?;
        let fs = Arc::clone(&self.deps.fs);
        let generation = self.flow.chat.generation;
        let post = self.poster();
        std::thread::Builder::new()
            .name("chat-file".into())
            .spawn(move || {
                let result = prepare_file(&*fs, src, &dir, &name, kind, size, budget).map(
                    |(path, preview)| Request::SendFile {
                        chat,
                        path,
                        kind,
                        preview,
                        duration: None,
                        text,
                    },
                );
                post(Internal::Flow(Box::new(FlowDone::Chat(
                    ChatDone::Prepared { generation, result },
                ))));
            })
            .map_err(|e| err!(IntTaskFailed).with_source(e))?;
        Ok(())
    }

    /// The name and size of what is sent; `None` for a file already in place.
    fn outgoing_name_and_size(&self, src: &Outgoing) -> Result<Option<(String, u64)>> {
        Ok(match src {
            Outgoing::Path(p) => {
                let m = self.deps.fs.metadata_follow(p)?;
                ensure!(!m.is_dir(), ChtRequestFailed, detail = "a folder");
                let name = p.file_name().map(|n| n.to_string_lossy().into_owned());
                Some((name.unwrap_or_default(), m.size))
            }
            Outgoing::Bytes { bytes, name } => Some((name.clone(), bytes.len() as u64)),
            Outgoing::Ready(_) => None,
        })
    }

    /// A small file is copied before this returns (a caller may remove its own copy then); a
    /// large one on the thread.
    fn copy_small_now(&self, src: Outgoing, dir: &Path, name: &str, size: u64) -> Result<Outgoing> {
        let Outgoing::Path(p) = src else {
            return Ok(src);
        };
        if size > MAX_FILE {
            return Ok(Outgoing::Path(p));
        }
        let dest = dir.join(name);
        let fs = &self.deps.fs;
        fs.create_dir_all(dir, 0o700)
            .and_then(|()| fs.copy_file(&p, &dest, 0o600))?;
        Ok(Outgoing::Ready(dest))
    }
}

/// What a file sent from this computer is, by its name.
pub(super) fn file_kind(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "gif" => FileKind::Gif,
        "jpg" | "jpeg" | "png" | "webp" | "bmp" => FileKind::Image,
        "mp4" | "m4v" | "mov" | "webm" | "mkv" | "avi" => FileKind::Video,
        _ => FileKind::File,
    }
}

/// A file name for the Chat folder and for the friend: the last component, no control
/// characters, not empty, at most 120 characters.
fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let clean: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let clean = clean.trim().trim_start_matches('.').to_owned();
    if clean.is_empty() {
        "file".to_owned()
    } else {
        clean
    }
}

/// Put the file to send at `dir/name` (0600, the folder 0700) and make a picture's preview.
fn prepare_file(
    fs: &dyn Fs,
    src: Outgoing,
    dir: &Path,
    name: &str,
    kind: FileKind,
    size: u64,
    budget: usize,
) -> Result<(PathBuf, Option<Vec<u8>>)> {
    let dest = match src {
        Outgoing::Ready(p) => p,
        Outgoing::Path(p) => {
            fs.create_dir_all(dir, 0o700)?;
            let dest = dir.join(name);
            fs.copy_file(&p, &dest, 0o600)?;
            dest
        }
        Outgoing::Bytes { bytes, .. } => {
            fs.create_dir_all(dir, 0o700)?;
            let dest = dir.join(name);
            fs.write_new(&dest, &bytes, 0o600)?;
            dest
        }
    };
    let preview = (kind == FileKind::Image && size <= PREVIEW_MAX_SOURCE)
        .then(|| fs.read(&dest, PREVIEW_MAX_SOURCE).ok())
        .flatten()
        .and_then(|b| preview_jpeg(&b, budget));
    Ok((dest, preview))
}

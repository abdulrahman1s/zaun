//! File requests: send, receive, cancel, open, locate.
use super::*;

/// A file to send, as `Request::SendFile` carries it.
pub(super) struct OutFile<'a> {
    pub path: &'a Path,
    pub kind: FileKind,
    pub preview: Option<&'a [u8]>,
    pub duration: Option<u32>,
    pub text: &'a str,
}

impl Codec {
    pub(super) fn render_send_file(
        &mut self,
        chat: ChatRef,
        f: &OutFile<'_>,
        out: &mut Output,
    ) -> Option<(RequestKind, String)> {
        if !self.files_on() {
            out.events.push(failed(
                RequestKind::SendFile,
                ErrorCode::ChtRequestFailed,
                "files are off: custom relays without a file server".to_owned(),
            ));
            return None;
        }
        let name = f
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let m = serde_json::json!([{
            "fileSource": { "filePath": f.path },
            "msgContent": content(f),
            "mentions": {},
        }]);
        tracing::debug!(name, "chat: sending a file");
        Some((
            RequestKind::SendFile,
            format!("/_send {} json {m}", chat_ref(chat)),
        ))
    }

    pub(super) fn render_file_action(
        &mut self,
        chat: ChatRef,
        item: i64,
        cancel: bool,
        out: &mut Output,
    ) {
        let Some(f) = self.files.borrow().get(&(chat, item)).cloned() else {
            out.events.push(failed(
                RequestKind::File,
                ErrorCode::ChtRequestFailed,
                "no file known for that message".to_owned(),
            ));
            return;
        };
        let cmd = if cancel {
            format!("/fcancel {}", f.id)
        } else {
            format!("/freceive {} encrypt=off", f.id)
        };
        self.send(out, Pending::File { cancel }, cmd);
    }

    pub(super) fn render_open_file(&self, chat: ChatRef, item: i64, out: &mut Output) {
        let f = self.files.borrow().get(&(chat, item)).cloned();
        match f.and_then(|f| Some((f.local?, f.kind))) {
            Some((path, kind)) => out.events.push(ChatEvent::OpenFile { path, kind }),
            None => out.events.push(failed(
                RequestKind::File,
                ErrorCode::ChtRequestFailed,
                "that file is not on this computer".to_owned(),
            )),
        }
    }

    pub(super) fn render_locate_file(&self, chat: ChatRef, item: i64, tag: u64, out: &mut Output) {
        let path = (self.files.borrow().get(&(chat, item))).and_then(|f| f.local.clone());
        out.events.push(ChatEvent::Located { tag, path });
    }
}

/// The message content of a file: a GIF goes as a picture (the engine's apps show it as one,
/// and EasyPlay draws the fetched file).
fn content(f: &OutFile<'_>) -> Value {
    let image = f.preview.map_or_else(|| PREVIEW.to_owned(), data_uri);
    let text = f.text;
    match f.kind {
        FileKind::Image | FileKind::Gif => {
            serde_json::json!({ "type": "image", "text": text, "image": image })
        }
        FileKind::Video => serde_json::json!({
            "type": "video", "text": text, "image": image,
            "duration": f.duration.unwrap_or(0),
        }),
        FileKind::File => serde_json::json!({ "type": "file", "text": text }),
    }
}

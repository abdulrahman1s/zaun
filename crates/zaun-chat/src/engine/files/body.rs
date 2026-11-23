//! A file message's content and how its item shows (Protocol.hs `MsgContent`; the picture and GIF rules of the old codec).

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zaun_chat_wire::FileKind;
use zaun_chat_wire::model::{FileState, Media, MessageBody};

use crate::chat::content::MsgContent;
use crate::chat::image::ImageData;
use crate::engine::profile::picture_of;
use crate::engine::records::file::Way;

/// A 1x1 PNG: the preview SimpleX requires with a picture or video when the user has none.
pub(super) const PREVIEW: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

fn data_uri(bytes: &[u8]) -> String {
    let kind = if bytes.starts_with(b"\x89PNG") {
        "png"
    } else {
        "jpg"
    };
    format!("data:image/{kind};base64,{}", STANDARD.encode(bytes))
}

/// What the message says it is: a GIF goes as a picture, which the apps show as one.
pub(super) fn content(
    kind: FileKind,
    text: &str,
    preview: Option<&[u8]>,
    duration: Option<u32>,
) -> MsgContent {
    let image = ImageData(preview.map_or_else(|| PREVIEW.to_owned(), data_uri));
    let text = text.to_owned();
    match kind {
        FileKind::Image | FileKind::Gif => MsgContent::Image { text, image },
        FileKind::Video => MsgContent::Video {
            text,
            image,
            duration: i64::from(duration.unwrap_or(0)),
        },
        FileKind::File => MsgContent::File { text },
    }
}

fn is_gif(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"))
}

/// What a friend's message makes of its file: voice and the rest are shown as files.
pub(super) fn kind_of(content: &MsgContent, name: &str) -> FileKind {
    match content {
        MsgContent::Image { .. } if is_gif(name) => FileKind::Gif,
        MsgContent::Image { .. } => FileKind::Image,
        MsgContent::Video { .. } => FileKind::Video,
        MsgContent::Text { .. }
        | MsgContent::Link { .. }
        | MsgContent::Voice { .. }
        | MsgContent::File { .. }
        | MsgContent::Report { .. }
        | MsgContent::Chat { .. }
        | MsgContent::Unknown { .. } => FileKind::File,
    }
}

/// The preview picture of a received message; the placeholder and anything but JPEG or PNG are none.
pub(super) fn preview_of(content: &MsgContent) -> Option<Vec<u8>> {
    let (MsgContent::Image { image, .. } | MsgContent::Video { image, .. }) = content else {
        return None;
    };
    if image.0 == PREVIEW {
        return None;
    }
    picture_of(&image.0).map(|p| p.bytes)
}

pub(super) fn duration_of(content: &MsgContent) -> Option<u32> {
    match content {
        MsgContent::Video { duration, .. } => u32::try_from(*duration).ok(),
        MsgContent::Text { .. }
        | MsgContent::Link { .. }
        | MsgContent::Image { .. }
        | MsgContent::Voice { .. }
        | MsgContent::File { .. }
        | MsgContent::Report { .. }
        | MsgContent::Chat { .. }
        | MsgContent::Unknown { .. } => None,
    }
}

/// A GIF that is here (one we send always is) is shown as one; anything else as a picture, video or file with its state.
pub(super) fn show(kind: FileKind, media: Media) -> MessageBody {
    let here = matches!(
        media.state,
        FileState::Sending { .. } | FileState::Sent | FileState::Received(_)
    );
    match (kind, media.local.as_ref()) {
        (FileKind::Gif, Some(p)) if here => MessageBody::Gif(p.to_string_lossy().into_owned()),
        (FileKind::Image | FileKind::Gif, _) => MessageBody::Image(media),
        (FileKind::Video, _) => MessageBody::Video(media),
        (FileKind::File, _) => MessageBody::File(media),
    }
}

pub(super) fn media_of(body: &MessageBody) -> Option<&Media> {
    match body {
        MessageBody::Image(m) | MessageBody::Video(m) | MessageBody::File(m) => Some(m),
        MessageBody::Text(_)
        | MessageBody::Notice(_)
        | MessageBody::Voice { .. }
        | MessageBody::Gif(_)
        | MessageBody::Call(_)
        | MessageBody::Stream(_)
        | MessageBody::Deleted => None,
    }
}

/// The state of a file item; a GIF that is here is `Sent` or `Received` by its path.
pub(super) fn state_of(body: &MessageBody, way: Way) -> Option<FileState> {
    match (body, way) {
        (MessageBody::Gif(_), Way::Sent) => Some(FileState::Sent),
        (MessageBody::Gif(p), Way::Received) => Some(FileState::Received(PathBuf::from(p))),
        (other, _) => media_of(other).map(|m| m.state.clone()),
    }
}

/// `body` with the file's new state and local copy.
pub(super) fn with_state(
    body: &MessageBody,
    kind: FileKind,
    state: FileState,
    local: Option<PathBuf>,
) -> Option<MessageBody> {
    let mut media = match body {
        MessageBody::Gif(p) => {
            let name = Path::new(p)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let size = fs::metadata(p).map_or(0, |m| m.len());
            Media {
                name,
                size,
                preview: None,
                duration: None,
                caption: String::new(),
                local: None,
                state: FileState::Failed,
            }
        }
        other => media_of(other)?.clone(),
    };
    media.state = state;
    media.local = local;
    Some(show(kind, media))
}

/// The friend edited the text of their message: for a file it is the caption. A GIF that is
/// here has no caption to change.
pub(crate) fn edit_caption(body: &mut MessageBody, text: String) {
    match body {
        MessageBody::Image(m) | MessageBody::Video(m) | MessageBody::File(m) => m.caption = text,
        MessageBody::Gif(_) => {}
        MessageBody::Text(_)
        | MessageBody::Notice(_)
        | MessageBody::Voice { .. }
        | MessageBody::Call(_)
        | MessageBody::Stream(_)
        | MessageBody::Deleted => *body = MessageBody::Text(text),
    }
}

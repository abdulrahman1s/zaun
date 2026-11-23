//! Core's chat model types as the wire's.

use zaun_chat_wire as w;
use zaun_chat_wire::model as wm;

use crate::chat::engine::FileKind;
use crate::model::{ChatProfile, ChatRef, MyStatus, Picture};

pub(super) fn chat_ref(c: ChatRef) -> wm::ChatRef {
    match c {
        ChatRef::Direct(id) => wm::ChatRef::Direct(id),
        ChatRef::Group(id) => wm::ChatRef::Group(id),
    }
}

pub(super) fn file_kind(k: FileKind) -> w::FileKind {
    match k {
        FileKind::Image => w::FileKind::Image,
        FileKind::Gif => w::FileKind::Gif,
        FileKind::Video => w::FileKind::Video,
        FileKind::File => w::FileKind::File,
    }
}

pub(super) fn picture(p: Picture) -> wm::Picture {
    let mime = if p.bytes().starts_with(b"\x89PNG") {
        "image/png"
    } else {
        "image/jpeg"
    };
    wm::Picture {
        bytes: p.bytes().to_vec(),
        mime: mime.to_owned(),
    }
}

pub(super) fn profile(p: ChatProfile) -> wm::ChatProfile {
    wm::ChatProfile {
        name: p.name,
        about: p.about,
        paper: p.paper,
        status: match p.status {
            MyStatus::Online => wm::MyStatus::Online,
            MyStatus::Away => wm::MyStatus::Away,
            MyStatus::Busy => wm::MyStatus::Busy,
            MyStatus::Invisible => wm::MyStatus::Invisible,
        },
        picture: p.picture.map(picture),
    }
}

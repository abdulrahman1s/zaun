//! Files over XFTP (codec: a_sent_picture_…, a_small_received_picture_…, a_large_picture_…,
//! a_video_and_a_file_…, sending_says_what_the_file_is_…, locating_a_file_…).

mod cancel;
mod fake;
mod locate;
mod receive;
mod send;

use std::path::PathBuf;

use base64::Engine as _;
use zaun_chat_wire::model::{ChatMessage, ChatRef, FileState, Media, MessageBody};
use zaun_chat_wire::{Event, FileKind, Request, RequestKind};

use self::fake::Fake;
use super::{FakeAgent, KEY, T, messages};
use crate::chat::container::MsgContainer;
use crate::chat::content::MsgContent;
use crate::chat::file::{FileDescr, FileInvitation};
use crate::chat::ids::SharedMsgId;
use crate::chat::image::ImageData;
use crate::chat::{ChatEvent, parse_chat_messages};
use crate::engine::agent_port::ConnId;
use crate::engine::files::{FileDone, FileKey};
use crate::engine::jobs::JobDone;
use crate::xftp::description::{
    ChunkReplica, FileChunk, FileDescription, Party, encode_description, split_description,
};
use crate::xftp::{XftpServer, preset_servers};

pub(super) const KB: u64 = 1024;

pub(super) fn setup() -> (T, Fake) {
    let mut t = T::started();
    let fake = Fake::default();
    t.e.files.transfers = Box::new(fake.clone());
    (t, fake)
}

/// Runs every job result that is waiting, as the loop would.
pub(super) fn settle(t: &mut T) -> Vec<Event> {
    while let Ok(d) = t.e.jobs.rx.try_recv() {
        t.e.on_job(d);
    }
    t.out()
}

pub(super) fn put(t: &T, name: &str) -> PathBuf {
    let p = t.dir.0.join("files").join(name);
    std::fs::write(&p, b"bytes of the file").unwrap();
    p
}

pub(super) fn description(server: &XftpServer) -> String {
    let chunk = FileChunk {
        chunk_no: 1,
        chunk_size: 65536,
        digest: vec![7; 32],
        replicas: vec![ChunkReplica {
            server: server.clone(),
            replica_id: vec![1; 24],
            replica_key: vec![2; 48],
        }],
    };
    encode_description(&FileDescription {
        party: Party::Recipient,
        size: 65536,
        digest: vec![3; 64],
        key: vec![4; 32],
        nonce: vec![5; 24],
        chunk_size: 65536,
        chunks: vec![chunk],
        redirect: None,
    })
}

pub(super) fn known() -> String {
    description(&preset_servers()[0])
}

pub(super) fn jpeg() -> ImageData {
    let b64 = base64::engine::general_purpose::STANDARD
        .encode([0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3, 4, 5, 6]);
    ImageData(format!("data:image/jpg;base64,{b64}"))
}

pub(super) fn offered(content: MsgContent, name: &str, size: u64) -> ChatEvent {
    let mut c = MsgContainer::simple(content);
    c.file = Some(FileInvitation::xftp(name, size));
    ChatEvent::MsgNew(c)
}

pub(super) fn picture(name: &str, size: u64) -> ChatEvent {
    offered(
        MsgContent::Image {
            text: "look".into(),
            image: jpeg(),
        },
        name,
        size,
    )
}

pub(super) fn shared_of(t: &mut T, friend: i64, item: i64) -> Vec<u8> {
    t.e.item(friend, item).unwrap().unwrap().shared_id.unwrap()
}

pub(super) fn parts(shared: &[u8], text: &str, size: usize) -> Vec<ChatEvent> {
    split_description(text, size, size)
        .into_iter()
        .map(|p| ChatEvent::MsgFileDescr {
            msg_id: SharedMsgId(shared.to_vec()),
            file_descr: FileDescr {
                file_descr_text: p.text,
                file_descr_part_no: i64::from(p.part_no),
                file_descr_complete: p.complete,
            },
            file_expires: None,
            file_badge: None,
        })
        .collect()
}

/// Everything the engine sent, as chat events.
pub(super) fn sent_events(t: &T) -> Vec<ChatEvent> {
    t.e.agent
        .sent()
        .iter()
        .flat_map(|(_, b)| parse_chat_messages(b).unwrap())
        .map(|m| m.event)
        .collect()
}

pub(super) fn media(m: &ChatMessage) -> &Media {
    match &m.body {
        MessageBody::Image(x) | MessageBody::Video(x) | MessageBody::File(x) => x,
        other => panic!("{other:?}"),
    }
}

pub(super) fn send(t: &mut T, bob: i64, path: PathBuf, kind: FileKind) -> Vec<Event> {
    t.req(Request::SendFile {
        chat: ChatRef::Direct(bob),
        path,
        kind,
        preview: None,
        duration: None,
        text: String::new(),
    })
}

pub(super) fn send_named(t: &mut T, bob: i64, name: &str, kind: FileKind) -> Vec<Event> {
    let path = put(t, name);
    send(t, bob, path, kind)
}

pub(super) fn failed(out: &[Event], kind: RequestKind) -> bool {
    out.iter()
        .any(|e| matches!(e, Event::Failed { request, .. } if *request == kind))
}

pub(super) fn peer_offers(t: &mut T, conn: &ConnId, event: ChatEvent) -> ChatMessage {
    messages(&t.peer_says(conn, 1, event)).remove(0)
}

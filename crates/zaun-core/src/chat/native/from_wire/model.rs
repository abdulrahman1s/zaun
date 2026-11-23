//! The wire's profile, friends and groups as core's. A picture is checked again here: a bad
//! one is dropped, never the profile it came with (ADR-0038).

use base64::Engine as _;
use zaun_chat_wire as w;
use zaun_chat_wire::model as wm;

use crate::chat::engine::{FileKind, RequestKind};
use crate::chat::picture;
use crate::clock::Timestamp;
use crate::model::{
    ChatFeatures, ChatGroup, ChatProfile, ChatRef, Friend, FriendRequest, GroupMember, Membership,
    MyStatus, Picture, Playing, Presence,
};

pub(super) fn picture(p: wm::Picture) -> Option<Picture> {
    let kind = match p.mime.as_str() {
        "image/jpeg" => "jpeg",
        "image/png" => "png",
        other => {
            tracing::warn!(
                mime = other,
                "chat: the helper sent a picture of an unknown type"
            );
            return None;
        }
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&p.bytes);
    let found = picture::from_data_uri(&format!("data:image/{kind};base64,{b64}"));
    if found.is_none() {
        tracing::warn!(len = p.bytes.len(), "chat: the helper sent a bad picture");
    }
    found
}

pub(super) fn time(ms: i64) -> Timestamp {
    Timestamp::from_millis(ms)
}

pub(super) fn chat_ref(c: wm::ChatRef) -> ChatRef {
    match c {
        wm::ChatRef::Direct(id) => ChatRef::Direct(id),
        wm::ChatRef::Group(id) => ChatRef::Group(id),
    }
}

pub(super) fn file_kind(k: w::FileKind) -> FileKind {
    match k {
        w::FileKind::Image => FileKind::Image,
        w::FileKind::Gif => FileKind::Gif,
        w::FileKind::Video => FileKind::Video,
        w::FileKind::File => FileKind::File,
    }
}

pub(super) fn request_kind(k: w::RequestKind) -> RequestKind {
    match k {
        w::RequestKind::Start => RequestKind::Start,
        w::RequestKind::SetProfile => RequestKind::SetProfile,
        w::RequestKind::CreateInvite => RequestKind::CreateInvite,
        w::RequestKind::Connect => RequestKind::Connect,
        w::RequestKind::Answer => RequestKind::Answer,
        w::RequestKind::Address => RequestKind::Address,
        w::RequestKind::Send => RequestKind::Send,
        w::RequestKind::MarkRead => RequestKind::MarkRead,
        w::RequestKind::History => RequestKind::History,
        w::RequestKind::Refresh => RequestKind::Refresh,
        w::RequestKind::Group => RequestKind::Group,
        w::RequestKind::RemoveFriend => RequestKind::RemoveFriend,
        w::RequestKind::SendFile => RequestKind::SendFile,
        w::RequestKind::Edit => RequestKind::Edit,
        w::RequestKind::Delete => RequestKind::Delete,
        w::RequestKind::React => RequestKind::React,
        w::RequestKind::Mute => RequestKind::Mute,
        w::RequestKind::Block => RequestKind::Block,
        w::RequestKind::Code => RequestKind::Code,
        w::RequestKind::Members => RequestKind::Members,
        w::RequestKind::File => RequestKind::File,
        w::RequestKind::GroupLink => RequestKind::GroupLink,
        w::RequestKind::Call => RequestKind::Call,
    }
}

pub(in crate::chat::native) fn features(f: wm::ChatFeatures) -> ChatFeatures {
    ChatFeatures {
        presence: f.presence,
        calls: f.calls,
        group_voice: f.group_voice,
        share: f.share,
        voice: f.voice,
        gifs: f.gifs,
        files: f.files,
        reactions: f.reactions,
        replies: f.replies,
        edit: f.edit,
        mute: f.mute,
        block: f.block,
        verify: f.verify,
        mentions: f.mentions,
        link_previews: f.link_previews,
    }
}

pub(super) fn profile(p: wm::ChatProfile) -> ChatProfile {
    ChatProfile {
        name: p.name,
        about: p.about,
        paper: p.paper,
        status: match p.status {
            wm::MyStatus::Online => MyStatus::Online,
            wm::MyStatus::Away => MyStatus::Away,
            wm::MyStatus::Busy => MyStatus::Busy,
            wm::MyStatus::Invisible => MyStatus::Invisible,
        },
        picture: p.picture.and_then(picture),
    }
}

fn presence(p: wm::Presence) -> Presence {
    match p {
        wm::Presence::Online => Presence::Online,
        wm::Presence::Away => Presence::Away,
        wm::Presence::Busy => Presence::Busy,
        wm::Presence::Offline => Presence::Offline,
    }
}

fn playing(p: wm::Playing) -> Playing {
    Playing {
        title: p.title,
        minutes: p.minutes,
        live: p.live,
    }
}

pub(super) fn friend(f: wm::Friend) -> Friend {
    Friend {
        id: f.id,
        name: f.name,
        ready: f.ready,
        presence: presence(f.presence),
        playing: f.playing.map(playing),
        last_seen: f.last_seen.map(time),
        about: f.about,
        paper: f.paper,
        since: f.since.map(time),
        verified: f.verified,
        blocked: f.blocked,
        muted: f.muted,
        seen: f.seen,
        picture: f.picture.and_then(picture),
    }
}

pub(super) fn request(r: wm::FriendRequest) -> FriendRequest {
    FriendRequest {
        id: r.id,
        name: r.name,
    }
}

pub(super) fn member(m: wm::GroupMember) -> GroupMember {
    GroupMember {
        id: m.id,
        name: m.name,
        friend: m.friend,
        me: m.me,
        owner: m.owner,
        presence: presence(m.presence),
        playing: m.playing.map(playing),
        blocked: m.blocked,
        picture: m.picture.and_then(picture),
    }
}

pub(super) fn group(g: wm::ChatGroup) -> ChatGroup {
    ChatGroup {
        id: g.id,
        name: g.name,
        membership: match g.membership {
            wm::Membership::Invited => Membership::Invited,
            wm::Membership::Joining => Membership::Joining,
            wm::Membership::Member => Membership::Member,
            wm::Membership::Left => Membership::Left,
        },
        members: g.members,
        member_list: g.member_list.into_iter().map(member).collect(),
        muted: g.muted,
        voice: g.voice,
        link: g.link,
        picture: g.picture.and_then(picture),
        owner: g.owner,
    }
}

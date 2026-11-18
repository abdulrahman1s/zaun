//! Things that happened in a conversation, on the wire: the same cases as
//! `model::ChatNotice`, so the overlay words them with the app's own sentences.
use serde::{Deserialize, Serialize};

use crate::model::ChatNotice;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "n", rename_all = "snake_case")]
pub enum Notice {
    Connected,
    GroupCreated,
    YouJoined,
    MemberJoined { who: String },
    MemberAdded { by: Option<String>, who: String },
    MemberLeft { who: String },
    MemberRemoved { by: Option<String>, who: String },
    YouLeft,
    YouRemoved { by: String },
    GroupDeleted,
    GroupNameChanged { name: String },
    NameChanged { from: String, to: String },
    FriendDeleted,
    GroupInvitation { group: String, sent: bool },
    CodeChanged,
}

impl From<&ChatNotice> for Notice {
    fn from(n: &ChatNotice) -> Self {
        match n.clone() {
            ChatNotice::Connected => Self::Connected,
            ChatNotice::GroupCreated => Self::GroupCreated,
            ChatNotice::YouJoined => Self::YouJoined,
            ChatNotice::MemberJoined(who) => Self::MemberJoined { who },
            ChatNotice::MemberAdded { by, who } => Self::MemberAdded { by, who },
            ChatNotice::MemberLeft(who) => Self::MemberLeft { who },
            ChatNotice::MemberRemoved { by, who } => Self::MemberRemoved { by, who },
            ChatNotice::YouLeft => Self::YouLeft,
            ChatNotice::YouRemoved(by) => Self::YouRemoved { by },
            ChatNotice::GroupDeleted => Self::GroupDeleted,
            ChatNotice::GroupNameChanged(name) => Self::GroupNameChanged { name },
            ChatNotice::NameChanged { from, to } => Self::NameChanged { from, to },
            ChatNotice::FriendDeleted => Self::FriendDeleted,
            ChatNotice::GroupInvitation { group, sent } => Self::GroupInvitation { group, sent },
            ChatNotice::CodeChanged => Self::CodeChanged,
        }
    }
}

impl From<Notice> for ChatNotice {
    fn from(n: Notice) -> Self {
        match n {
            Notice::Connected => Self::Connected,
            Notice::GroupCreated => Self::GroupCreated,
            Notice::YouJoined => Self::YouJoined,
            Notice::MemberJoined { who } => Self::MemberJoined(who),
            Notice::MemberAdded { by, who } => Self::MemberAdded { by, who },
            Notice::MemberLeft { who } => Self::MemberLeft(who),
            Notice::MemberRemoved { by, who } => Self::MemberRemoved { by, who },
            Notice::YouLeft => Self::YouLeft,
            Notice::YouRemoved { by } => Self::YouRemoved(by),
            Notice::GroupDeleted => Self::GroupDeleted,
            Notice::GroupNameChanged { name } => Self::GroupNameChanged(name),
            Notice::NameChanged { from, to } => Self::NameChanged { from, to },
            Notice::FriendDeleted => Self::FriendDeleted,
            Notice::GroupInvitation { group, sent } => Self::GroupInvitation { group, sent },
            Notice::CodeChanged => Self::CodeChanged,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_notice_survives_the_wire_unchanged() {
        let all = [
            ChatNotice::Connected,
            ChatNotice::MemberAdded {
                by: None,
                who: "Lucía".into(),
            },
            ChatNotice::YouRemoved("Mira".into()),
            ChatNotice::NameChanged {
                from: "K".into(),
                to: "Kit".into(),
            },
            ChatNotice::GroupInvitation {
                group: "Friday Crew".into(),
                sent: true,
            },
        ];
        for n in all {
            let wire = serde_json::to_string(&Notice::from(&n)).unwrap();
            let back: Notice = serde_json::from_str(&wire).unwrap();
            assert_eq!(ChatNotice::from(back), n);
        }
    }
}

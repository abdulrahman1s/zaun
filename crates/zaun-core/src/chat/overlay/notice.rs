//! Things that happened in a conversation, on the wire: the same cases as
//! `model::ChatNotice`, so the overlay words them with the app's own sentences.
use serde::{Deserialize, Serialize};

use crate::model::{ChatNotice, EncryptionState, IntegrityProblem};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "n", rename_all = "snake_case")]
pub enum Notice {
    Connected,
    GroupCreated,
    YouJoined,
    MemberJoined {
        who: String,
    },
    MemberAdded {
        by: Option<String>,
        who: String,
    },
    MemberLeft {
        who: String,
    },
    MemberRemoved {
        by: Option<String>,
        who: String,
    },
    YouLeft,
    YouRemoved {
        by: String,
    },
    GroupDeleted,
    GroupNameChanged {
        name: String,
    },
    NameChanged {
        from: String,
        to: String,
    },
    FriendDeleted,
    GroupInvitation {
        group: String,
        sent: bool,
    },
    CodeChanged,
    MemberBlocked {
        by: Option<String>,
        who: String,
        blocked: bool,
    },
    AddressChanged {
        by_you: bool,
        done: bool,
        member: Option<String>,
    },
    Encryption {
        state: Encryption,
    },
    BadSignature,
    Integrity {
        problem: Integrity,
    },
    Undecryptable {
        count: u32,
    },
    DirectInvitation {
        from: String,
    },
}

/// [`EncryptionState`] on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encryption {
    Repaired,
    Allowed,
    Required,
    Started,
    Agreed,
}

/// [`IntegrityProblem`] on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "p", rename_all = "snake_case")]
pub enum Integrity {
    Skipped { count: u64 },
    OutOfOrder,
    Altered,
    Duplicate,
}

impl From<EncryptionState> for Encryption {
    fn from(s: EncryptionState) -> Self {
        match s {
            EncryptionState::Repaired => Self::Repaired,
            EncryptionState::Allowed => Self::Allowed,
            EncryptionState::Required => Self::Required,
            EncryptionState::Started => Self::Started,
            EncryptionState::Agreed => Self::Agreed,
        }
    }
}

impl From<Encryption> for EncryptionState {
    fn from(s: Encryption) -> Self {
        match s {
            Encryption::Repaired => Self::Repaired,
            Encryption::Allowed => Self::Allowed,
            Encryption::Required => Self::Required,
            Encryption::Started => Self::Started,
            Encryption::Agreed => Self::Agreed,
        }
    }
}

impl From<IntegrityProblem> for Integrity {
    fn from(p: IntegrityProblem) -> Self {
        match p {
            IntegrityProblem::Skipped(count) => Self::Skipped { count },
            IntegrityProblem::OutOfOrder => Self::OutOfOrder,
            IntegrityProblem::Altered => Self::Altered,
            IntegrityProblem::Duplicate => Self::Duplicate,
        }
    }
}

impl From<Integrity> for IntegrityProblem {
    fn from(p: Integrity) -> Self {
        match p {
            Integrity::Skipped { count } => Self::Skipped(count),
            Integrity::OutOfOrder => Self::OutOfOrder,
            Integrity::Altered => Self::Altered,
            Integrity::Duplicate => Self::Duplicate,
        }
    }
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
            ChatNotice::MemberBlocked { by, who, blocked } => {
                Self::MemberBlocked { by, who, blocked }
            }
            ChatNotice::AddressChanged {
                by_you,
                done,
                member,
            } => Self::AddressChanged {
                by_you,
                done,
                member,
            },
            ChatNotice::Encryption(s) => Self::Encryption { state: s.into() },
            ChatNotice::BadSignature => Self::BadSignature,
            ChatNotice::Integrity(p) => Self::Integrity { problem: p.into() },
            ChatNotice::Undecryptable { count } => Self::Undecryptable { count },
            ChatNotice::DirectInvitation(from) => Self::DirectInvitation { from },
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
            Notice::MemberBlocked { by, who, blocked } => Self::MemberBlocked { by, who, blocked },
            Notice::AddressChanged {
                by_you,
                done,
                member,
            } => Self::AddressChanged {
                by_you,
                done,
                member,
            },
            Notice::Encryption { state } => Self::Encryption(state.into()),
            Notice::BadSignature => Self::BadSignature,
            Notice::Integrity { problem } => Self::Integrity(problem.into()),
            Notice::Undecryptable { count } => Self::Undecryptable { count },
            Notice::DirectInvitation { from } => Self::DirectInvitation(from),
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
            ChatNotice::MemberBlocked {
                by: Some("Mira".into()),
                who: "K".into(),
                blocked: true,
            },
            ChatNotice::AddressChanged {
                by_you: true,
                done: false,
                member: Some("K".into()),
            },
            ChatNotice::Encryption(EncryptionState::Required),
            ChatNotice::BadSignature,
            ChatNotice::Integrity(IntegrityProblem::Skipped(3)),
            ChatNotice::Integrity(IntegrityProblem::Altered),
            ChatNotice::Undecryptable { count: 2 },
            ChatNotice::DirectInvitation("Mira".into()),
        ];
        for n in all {
            let wire = serde_json::to_string(&Notice::from(&n)).unwrap();
            let back: Notice = serde_json::from_str(&wire).unwrap();
            assert_eq!(ChatNotice::from(back), n);
        }
    }
}

//! The wire's conversation notices as core's; who moved or asked comes from the item's author.

use zaun_chat_wire::model as wm;

use crate::model::{ChatNotice, EncryptionState, IntegrityProblem};

pub(super) fn notice(n: wm::ChatNotice, by: &wm::MessageAuthor) -> ChatNotice {
    match n {
        wm::ChatNotice::Connected => ChatNotice::Connected,
        wm::ChatNotice::GroupCreated => ChatNotice::GroupCreated,
        wm::ChatNotice::YouJoined => ChatNotice::YouJoined,
        wm::ChatNotice::MemberJoined(n) => ChatNotice::MemberJoined(n),
        wm::ChatNotice::MemberAdded { by, who } => ChatNotice::MemberAdded { by, who },
        wm::ChatNotice::MemberLeft(n) => ChatNotice::MemberLeft(n),
        wm::ChatNotice::MemberBlocked { by, who, blocked } => {
            ChatNotice::MemberBlocked { by, who, blocked }
        }
        wm::ChatNotice::MemberRemoved { by, who } => ChatNotice::MemberRemoved { by, who },
        wm::ChatNotice::YouLeft => ChatNotice::YouLeft,
        wm::ChatNotice::YouRemoved(n) => ChatNotice::YouRemoved(n),
        wm::ChatNotice::GroupDeleted => ChatNotice::GroupDeleted,
        wm::ChatNotice::GroupNameChanged(n) => ChatNotice::GroupNameChanged(n),
        wm::ChatNotice::NameChanged { from, to } => ChatNotice::NameChanged { from, to },
        wm::ChatNotice::FriendDeleted => ChatNotice::FriendDeleted,
        wm::ChatNotice::GroupInvitation { group, sent } => {
            ChatNotice::GroupInvitation { group, sent }
        }
        wm::ChatNotice::CodeChanged => ChatNotice::CodeChanged,
        wm::ChatNotice::AddressSwitch { phase, member } => ChatNotice::AddressChanged {
            by_you: *by == wm::MessageAuthor::Me,
            done: phase == wm::SwitchPhase::Completed,
            member,
        },
        wm::ChatNotice::RatchetSync { status, .. } => ChatNotice::Encryption(match status {
            wm::SyncStatus::Ok => EncryptionState::Repaired,
            wm::SyncStatus::Allowed => EncryptionState::Allowed,
            wm::SyncStatus::Required => EncryptionState::Required,
            wm::SyncStatus::Started => EncryptionState::Started,
            wm::SyncStatus::Agreed => EncryptionState::Agreed,
        }),
        wm::ChatNotice::BadSignature => ChatNotice::BadSignature,
        wm::ChatNotice::Integrity(e) => ChatNotice::Integrity(match e {
            wm::IntegrityError::Skipped { from, to } => {
                IntegrityProblem::Skipped(to.saturating_sub(from).saturating_add(1).unsigned_abs())
            }
            wm::IntegrityError::BadId(_) => IntegrityProblem::OutOfOrder,
            wm::IntegrityError::BadHash => IntegrityProblem::Altered,
            wm::IntegrityError::Duplicate => IntegrityProblem::Duplicate,
        }),
        wm::ChatNotice::DecryptionError { count, .. } => ChatNotice::Undecryptable { count },
        wm::ChatNotice::DirectInvitation => ChatNotice::DirectInvitation(match by {
            wm::MessageAuthor::Member(name) => name.clone(),
            wm::MessageAuthor::Me | wm::MessageAuthor::Friend => String::new(),
        }),
    }
}

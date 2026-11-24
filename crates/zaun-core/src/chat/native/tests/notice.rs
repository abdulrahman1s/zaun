//! What simplex-client adds to the pipe: its failure codes, and the notices and delivery state
//! core words in the conversation.

use zaun_chat_wire::model as wm;
use zaun_chat_wire::{Event, RequestKind};

use crate::chat::engine::ChatEvent;
use crate::chat::native::from_wire::event;
use crate::error::ErrorCode;
use crate::model::{ChatMessage, ChatNotice, Delivery, EncryptionState, IntegrityProblem};

fn code_of(code: &str) -> ErrorCode {
    let ChatEvent::Failed { code, .. } = event(Event::Failed {
        request: RequestKind::Send,
        code: code.into(),
        detail: String::new(),
    }) else {
        panic!("not Failed");
    };
    code
}

fn arrived(author: wm::MessageAuthor, body: wm::MessageBody, d: wm::Delivery) -> ChatMessage {
    let m = wm::ChatMessage {
        id: 1,
        chat: wm::ChatRef::Group(2),
        author,
        body,
        at: 0,
        delivery: d,
        unread: false,
        reply: None,
        reactions: Vec::new(),
        edited: false,
        mentions: Vec::new(),
    };
    let ChatEvent::Messages(mut out) = event(Event::Messages(vec![m])) else {
        panic!("not Messages");
    };
    out.remove(0)
}

fn notice(author: wm::MessageAuthor, n: wm::ChatNotice) -> ChatNotice {
    let m = arrived(author, wm::MessageBody::Notice(n), wm::Delivery::Received);
    let crate::model::MessageBody::Notice(n) = m.body else {
        panic!("not a notice");
    };
    n
}

#[test]
fn the_engines_codes_become_zauns_of_the_same_number_and_024_the_picture_code() {
    use ErrorCode as C;
    for (sx, zn) in [
        ("SX-CHT-004", C::ChtNoConnection),
        ("SX-CHT-005", C::ChtBadLink),
        ("SX-CHT-006", C::ChtRequestFailed),
        ("SX-CHT-012", C::ChtCallFailed),
        ("SX-CHT-017", C::ChtStreamFailed),
        ("SX-CHT-018", C::ChtReactionNotAllowed),
        ("SX-CHT-019", C::ChtFileTooLarge),
        ("SX-CHT-020", C::ChtTextTooLong),
        ("SX-CHT-021", C::ChtTooManyMentions),
        ("SX-CHT-022", C::ChtInvalidName),
        ("SX-CHT-023", C::ChtNotAllowed),
        ("SX-CHT-024", C::ChtPictureTooLarge),
    ] {
        assert_eq!(code_of(sx), zn, "{sx}");
        if sx != "SX-CHT-024" {
            assert_eq!(zn.as_str(), sx.replace("SX-", "ZN-"));
        }
    }
    assert_eq!(code_of("SX-CHT-999"), C::ChtRequestFailed);
}

#[test]
fn an_address_switch_is_the_users_when_the_user_wrote_the_item() {
    let switch = |phase| wm::ChatNotice::AddressSwitch {
        phase,
        member: Some("Bo".into()),
    };
    assert_eq!(
        notice(wm::MessageAuthor::Me, switch(wm::SwitchPhase::Completed)),
        ChatNotice::AddressChanged {
            by_you: true,
            done: true,
            member: Some("Bo".into())
        }
    );
    let theirs = notice(
        wm::MessageAuthor::Member("Bo".into()),
        switch(wm::SwitchPhase::Started),
    );
    assert!(matches!(
        theirs,
        ChatNotice::AddressChanged {
            by_you: false,
            done: false,
            ..
        }
    ));
}

#[test]
fn a_direct_invitation_names_the_member_who_wrote_it() {
    let n = notice(
        wm::MessageAuthor::Member("Mira".into()),
        wm::ChatNotice::DirectInvitation,
    );
    assert_eq!(n, ChatNotice::DirectInvitation("Mira".into()));
}

#[test]
fn skipped_message_ids_count_both_ends_and_the_other_notices_map_one_to_one() {
    let friend = || wm::MessageAuthor::Friend;
    let skipped = wm::IntegrityError::Skipped { from: 3, to: 5 };
    assert_eq!(
        notice(friend(), wm::ChatNotice::Integrity(skipped)),
        ChatNotice::Integrity(IntegrityProblem::Skipped(3))
    );
    assert_eq!(
        notice(
            friend(),
            wm::ChatNotice::Integrity(wm::IntegrityError::BadHash)
        ),
        ChatNotice::Integrity(IntegrityProblem::Altered)
    );
    let sync = wm::ChatNotice::RatchetSync {
        status: wm::SyncStatus::Ok,
        member: None,
    };
    assert_eq!(
        notice(friend(), sync),
        ChatNotice::Encryption(EncryptionState::Repaired)
    );
    let undecrypted = wm::ChatNotice::DecryptionError {
        error: wm::DecryptError::Header,
        count: 4,
    };
    assert_eq!(
        notice(friend(), undecrypted),
        ChatNotice::Undecryptable { count: 4 }
    );
    let blocked = wm::ChatNotice::MemberBlocked {
        by: None,
        who: "Kit".into(),
        blocked: false,
    };
    assert_eq!(
        notice(friend(), blocked),
        ChatNotice::MemberBlocked {
            by: None,
            who: "Kit".into(),
            blocked: false
        }
    );
}

#[test]
fn a_message_the_friend_got_changed_is_delivered_altered() {
    let m = arrived(
        wm::MessageAuthor::Me,
        wm::MessageBody::Text("hi".into()),
        wm::Delivery::DeliveredBadHash,
    );
    assert_eq!(m.delivery, Delivery::DeliveredAltered);
}

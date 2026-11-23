//! Security codes and the verified flag (codec: a_security_code_is_shown_…, the_list_says_who_is_…).

use zaun_chat_wire::model::{ChatNotice, ChatRef, MessageBody};
use zaun_chat_wire::{Event, Request, RequestKind};

use super::{T, messages};
use crate::engine::agent_port::AgentFailure;

/// SHA-256 of "ratchet ad one" and "ratchet ad two" as a big-endian decimal number, in fives.
const ONE: &str =
    "58130 46837 43252 69100 82714 94408 80668 27822 93719 15935 43594 81043 13010 29201 39022 63";
const TWO: &str =
    "10285 93355 16678 35540 52843 77498 45742 76409 08515 29042 15851 02656 33383 41003 61748 152";

fn code(out: &[Event]) -> String {
    out.iter()
        .find_map(|e| {
            if let Event::Code { code, .. } = e {
                Some(code.clone())
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("{out:?}"))
}

fn verified(out: &[Event]) -> bool {
    out.iter()
        .find_map(|e| {
            if let Event::Verified { matched, .. } = e {
                Some(*matched)
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("{out:?}"))
}

fn friend_with_code(t: &mut T, name: &str, ad: &[u8]) -> i64 {
    t.e.agent.ratchet_ad = ad.to_vec();
    t.friend(name).0
}

fn listed_verified(t: &mut T, id: i64) -> bool {
    let Event::Chats { friends, .. } = t.req(Request::Refresh).remove(0) else {
        panic!()
    };
    friends.iter().find(|f| f.id == id).unwrap().verified
}

#[test]
fn a_security_code_is_shown_and_a_match_verifies_the_friend() {
    let mut t = T::started();
    let bob = friend_with_code(&mut t, "bob", b"ratchet ad one");
    assert_eq!(code(&t.req(Request::ShowCode(bob))), ONE);
    let wrong = t.req(Request::Verify {
        friend: bob,
        code: "12345 67890".into(),
    });
    assert!(!verified(&wrong));
    assert!(!listed_verified(&mut t, bob));
    let out = t.req(Request::Verify {
        friend: bob,
        code: format!(" {ONE}\n"),
    });
    assert!(verified(&out));
    assert!(listed_verified(&mut t, bob));
}

#[test]
fn spaces_do_not_count_when_codes_are_compared() {
    let mut t = T::started();
    let bob = friend_with_code(&mut t, "bob", b"ratchet ad one");
    let bare = ONE.replace(' ', "");
    assert!(verified(&t.req(Request::Verify {
        friend: bob,
        code: bare
    })));
}

#[test]
fn verifying_with_no_code_takes_the_verification_back() {
    let mut t = T::started();
    let bob = friend_with_code(&mut t, "bob", b"ratchet ad one");
    t.req(Request::Verify {
        friend: bob,
        code: ONE.into(),
    });
    assert!(!verified(&t.req(Request::Verify {
        friend: bob,
        code: "  ".into()
    })));
    assert!(!listed_verified(&mut t, bob));
}

#[test]
fn a_code_that_changes_after_verification_ends_it_and_says_so() {
    let mut t = T::started();
    let bob = friend_with_code(&mut t, "bob", b"ratchet ad one");
    t.req(Request::Verify {
        friend: bob,
        code: ONE.into(),
    });
    assert!(
        t.req(Request::ShowCode(bob))
            .iter()
            .all(|e| !matches!(e, Event::Messages(_)))
    );
    t.e.agent.ratchet_ad = b"ratchet ad two".to_vec();
    let out = t.req(Request::ShowCode(bob));
    let [m] = messages(&out).try_into().unwrap();
    assert_eq!(m.body, MessageBody::Notice(ChatNotice::CodeChanged));
    assert_eq!(m.chat, ChatRef::Direct(bob));
    assert!(
        out.iter()
            .any(|e| matches!(e, Event::Code { code, .. } if code == TWO))
    );
    assert!(!listed_verified(&mut t, bob));
    let again = t.req(Request::ShowCode(bob));
    assert!(messages(&again).is_empty(), "told once");
}

#[test]
fn the_list_says_who_is_muted_verified_blocked_and_since_when() {
    let mut t = T::started();
    let bob = friend_with_code(&mut t, "bob", b"ratchet ad one");
    let (carol, _) = t.friend("carol");
    t.req(Request::SetMuted {
        chat: ChatRef::Direct(bob),
        on: true,
    });
    t.req(Request::SetBlocked {
        friend: bob,
        on: true,
    });
    t.req(Request::Verify {
        friend: bob,
        code: ONE.into(),
    });
    let Event::Chats { friends, chats, .. } = t.req(Request::Refresh).remove(0) else {
        panic!()
    };
    let b = friends.iter().find(|f| f.id == bob).unwrap();
    assert!(b.muted && b.verified && b.blocked);
    assert!(b.since.is_some_and(|s| s > 1_700_000_000_000));
    assert!(
        !chats.iter().any(|p| p.chat == ChatRef::Direct(bob)),
        "blocked: not in conversations"
    );
    let c = friends.iter().find(|f| f.id == carol).unwrap();
    assert!(!c.muted && !c.verified && !c.blocked);
}

#[test]
fn a_code_the_agent_cannot_give_fails_the_request() {
    let mut t = T::started();
    let (bob, _) = t.friend("bob");
    t.e.agent.fail_next = Some(AgentFailure::Other("no ratchet".into()));
    let out = t.req(Request::ShowCode(bob));
    assert!(
        matches!(
            out.as_slice(),
            [Event::Failed {
                request: RequestKind::Code,
                ..
            }]
        ),
        "{out:?}"
    );
    let out = t.req(Request::ShowCode(999));
    assert!(
        matches!(
            out.as_slice(),
            [Event::Failed {
                request: RequestKind::Code,
                ..
            }]
        ),
        "{out:?}"
    );
}

//! A connection repaired and moved by the official CLI 7.1.4 (or 7.0.3 with `ZAUN_SIMPLEX_BIN=7.0`
//! for the CLI): its ratchet re-synced (`/_sync … force=on`), then its queue switched (`/_switch`),
//! each followed by messages both ways. Same servers and variables as `live_cli`.

use serde_json::{Value, json};
use zaun_chat_wire::model::{ChatRef, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::Dir;
use super::live_cli::{Native, cli_event, cli_send, connect, engine_msg, until, var};
use crate::agent::tests::cli::{Cli, kind, received_text};

fn codes(n: &mut Native, c: &mut Cli, cid: i64, bob: i64) -> (String, String) {
    n.events.clear();
    n.req(Request::ShowCode(bob));
    let ours = until(n, c, "alice's code", |n, _| {
        n.events.iter().find_map(|e| match e {
            Event::Code { code, .. } => Some(code.clone()),
            _ => None,
        })
    });
    let r = c.cmd(&format!("/_get code @{cid}"));
    assert_eq!(kind(&r), "contactCode", "{r}");
    let theirs = r["connectionCode"].as_str().unwrap().to_owned();
    (ours, theirs)
}

fn both_ways(n: &mut Native, c: &mut Cli, cid: i64, bob: i64, tag: &str) {
    let to_bob = format!("to bob after {tag}");
    // Sending is refused until our side's re-sync is done (`ratchetSyncSendProhibited`), as in 7.1.4.
    until(n, c, "alice may send", |n, _| {
        n.events.clear();
        n.req(Request::Send {
            chat: ChatRef::Direct(bob),
            text: to_bob.clone(),
            reply_to: None,
            mentions: Vec::new(),
        });
        let refused = n.events.iter().any(|e| matches!(e, Event::Failed { .. }));
        (!refused).then_some(())
    });
    until(n, c, &format!("bob reads {to_bob:?}"), |_, c| {
        c.take(|e| received_text(e, cid).filter(|t| *t == to_bob))
    });
    let to_alice = format!("to alice after {tag}");
    cli_send(
        c,
        cid,
        json!({"msgContent": {"type": "text", "text": to_alice}}),
    );
    until(n, c, &format!("alice reads {to_alice:?}"), |n, _| {
        engine_msg(n, |m| m.body == MessageBody::Text(to_alice.clone()))
    });
}

#[test]
#[ignore = "needs a local smp-server, xftp-server and CLI; see live_cli"]
fn a_connection_the_cli_resyncs_and_then_switches_keeps_working_with_a_new_agreed_code() {
    let dir = Dir::new();
    let mut n = Native::start(&dir);
    let mut c = Cli::open(var("ZAUN_TEST_CLI").parse().unwrap());
    let (cid, bob) = connect(&mut n, &mut c);
    both_ways(&mut n, &mut c, cid, bob, "connecting");
    let (before, theirs) = codes(&mut n, &mut c, cid, bob);
    assert_eq!(before, theirs);

    for round in 1..=3 {
        let r = c.cmd(&format!("/_sync @{cid} force=on"));
        assert_eq!(kind(&r), "contactRatchetSyncStarted", "{r}");
        until(&mut n, &mut c, "the CLI's ratchet in sync again", |_, c| {
            cli_event(c, "contactRatchetSync", |e| {
                let s = e.pointer("/ratchetSyncProgress/ratchetSyncStatus");
                s.and_then(Value::as_str) == Some("ok")
            })
        });
        both_ways(&mut n, &mut c, cid, bob, &format!("re-sync {round}"));
        let (ours, theirs) = codes(&mut n, &mut c, cid, bob);
        assert_eq!(ours, theirs, "both sides show the same new code");
        assert_ne!(ours, before, "a new ratchet is a new code");
    }

    let r = c.cmd(&format!("/_switch @{cid}"));
    assert_eq!(kind(&r), "contactSwitchStarted", "{r}");
    until(&mut n, &mut c, "the CLI's switch completed", |_, c| {
        cli_event(c, "contactSwitch", |e| {
            e.pointer("/switchProgress/switchPhase")
                .and_then(Value::as_str)
                == Some("completed")
        })
    });
    both_ways(&mut n, &mut c, cid, bob, "the switch");
    both_ways(&mut n, &mut c, cid, bob, "the switch, again");

    // The CLI saw no skipped, repeated or out-of-order message along the way.
    let chat = c.cmd(&format!("/_get chat @{cid} count=200")).to_string();
    assert!(chat.contains("\"chatItems\""), "{chat}");
    assert!(!chat.contains("IntegrityError"), "{chat}");
    assert!(!chat.contains("decryptionError"), "{chat}");
}

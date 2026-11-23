//! Interop with the official CLI 7.1.4 over a local smp-server, both roles, full and short links, PQ off and on.
//! Start the server and two CLIs with `-p <port> -s <server>`, then run with
//! `ZAUN_TEST_SMP`, `ZAUN_TEST_CLI_A=<port of the CLI that invites>` and `ZAUN_TEST_CLI_B=<port of the one that joins>`.
use std::time::Duration;

use serde_json::Value;

use super::chat_bytes::{profile_name, text_of, x_info, x_msg};
use super::cli::{Cli, connected, kind, received_text, send_text};
use super::live::live_node;
use crate::agent::client::{AgentEvent, ConnId};
use crate::agent::links::ConnectionLink;
use crate::agent::short_link::{ContactShortLinkData, encode_short_link_data};
use crate::agent::testing::{Dir, Node};
use crate::chat::profile::Profile;
use crate::smp::{Clock, SystemClock};

fn cli(var: &str) -> Cli {
    let port = std::env::var(var).unwrap_or_else(|_| panic!("{var} is not set"));
    Cli::open(port.parse().unwrap())
}

fn rnd() -> String {
    let b = crate::crypto::random_vec(3).unwrap();
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Runs the agent and reads the CLI until `f` finds what it waits for.
fn until<T>(n: &mut Node, c: &mut Cli, mut f: impl FnMut(&mut Node, &mut Cli) -> Option<T>) -> T {
    let clock = SystemClock::new();
    loop {
        if let Some(t) = f(n, c) {
            return t;
        }
        let cli: Vec<&str> = c.events.iter().map(kind).collect();
        assert!(
            clock.now_ms() < 60_000,
            "timed out; agent {:?}; cli {cli:?}",
            n.events
        );
        n.step(Duration::from_millis(5));
        c.poll(Duration::from_millis(5));
    }
}

fn has(n: &Node, e: &AgentEvent) -> bool {
    n.events.iter().any(|x| x == e)
}

/// The status of every text the CLI sent in this chat.
fn snd_status(chat: &Value) -> Vec<String> {
    let items = chat.pointer("/chat/chatItems").and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .filter(|ci| ci.pointer("/chatDir/type").and_then(Value::as_str) == Some("directSnd"))
        .filter(|ci| {
            ci.pointer("/content/msgContent/type")
                .and_then(Value::as_str)
                == Some("text")
        })
        .filter_map(|ci| {
            ci.pointer("/meta/itemStatus/type")?
                .as_str()
                .map(str::to_owned)
        })
        .collect()
}

/// Texts both ways, a few times so the ratchet steps and the hash chains are exercised; receipts come back from the CLI.
fn exchange(n: &mut Node, c: &mut Cli, conn: ConnId, cid: i64) {
    for i in 0..3 {
        let t1 = format!("from the cli {i} {}", rnd());
        send_text(c, cid, &t1);
        let id = until(n, c, |n, _| {
            let (id, body) = n.take(|e| match e {
                AgentEvent::Msg {
                    conn: x,
                    msg_id,
                    body,
                    ..
                } if *x == conn => Some((*msg_id, body.clone())),
                _ => None,
            })?;
            // The CLI also sends events of its own after connecting, such as `x.info.probe`.
            if text_of(&body).as_deref() == Some(t1.as_str()) {
                return Some(id);
            }
            n.agent.ack(&mut n.store, conn, id, None).unwrap();
            None
        });
        n.agent
            .ack(&mut n.store, conn, id, Some(Vec::new()))
            .unwrap();
        let t2 = format!("from zaun {i} {} ✓ ünï", rnd());
        let sent = n.agent.send(&mut n.store, conn, x_msg(&t2)).unwrap();
        until(n, c, |_, c| {
            c.take(|e| received_text(e, cid).filter(|t| *t == t2))
        });
        until(n, c, |n, _| {
            n.take(|e| match e {
                AgentEvent::Rcvd {
                    conn: x, msg_id, ..
                } if *x == conn && *msg_id == sent => Some(()),
                _ => None,
            })
        });
    }
    // Our receipts: the CLI's own messages show as received by us.
    until(n, c, |_, c| {
        let chat = c.cmd(&format!("/_get chat @{cid} count=30"));
        let st = snd_status(&chat);
        (!st.is_empty() && st.iter().all(|s| s == "sndRcvd")).then_some(())
    });
    let chat = c.cmd(&format!("/_get chat @{cid} count=30"));
    let text = chat.to_string();
    assert!(
        !text.contains("rcvIntegrityError") && !text.contains("rcvDecryptionError"),
        "{text}"
    );
}

#[test]
#[ignore = "needs a local smp-server and the CLI; see the module docs"]
fn the_cli_invites_and_we_join_by_full_and_short_link_then_text_both_ways() {
    let mut c = cli("ZAUN_TEST_CLI_A");
    let dir = Dir::new();
    let mut n = live_node(&dir, "joiner");
    for (i, (field, pq)) in [("connFullLink", false), ("connShortLink", true)]
        .into_iter()
        .enumerate()
    {
        let name = format!("zaunj{i}{}", rnd());
        let inv = c.cmd("/_connect 1");
        assert_eq!(kind(&inv), "invitation", "{inv}");
        let link = inv
            .pointer(&format!("/connLinkInvitation/{field}"))
            .and_then(Value::as_str);
        let link = ConnectionLink::parse(link.expect("a link")).unwrap();
        let conn = n
            .agent
            .join(&mut n.store, &link, x_info(&name), pq)
            .unwrap();
        let cid = until(&mut n, &mut c, |n, c| {
            has(n, &AgentEvent::Con { conn })
                .then(|| c.take(|e| connected(e, &name)))
                .flatten()
        });
        let info = n.take(|e| match e {
            AgentEvent::Info { conn: x, conn_info } if *x == conn => Some(conn_info.clone()),
            _ => None,
        });
        assert!(
            info.and_then(|i| profile_name(&i)).is_some(),
            "the CLI's x.info"
        );
        let ratchet = n.agent.conn(conn).unwrap().ratchet.as_ref().unwrap();
        assert_eq!(ratchet.pq_support().0, pq);
        exchange(&mut n, &mut c, conn, cid);
    }
}

#[test]
#[ignore = "needs a local smp-server and the CLI; see the module docs"]
fn we_invite_and_the_cli_joins_by_full_and_short_link_then_text_both_ways() {
    let mut c = cli("ZAUN_TEST_CLI_B");
    let dir = Dir::new();
    let mut n = live_node(&dir, "inviter");
    // The last case: a short link with no profile in its data, as `create_invitation` makes.
    for (i, (short, pq, profile)) in [
        (false, false, true),
        (true, true, true),
        (true, false, false),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("zauni{i}{}", rnd());
        let data = encode_short_link_data(&ContactShortLinkData::new(Profile::new(&name, "")));
        let data = if profile { data.unwrap().0 } else { Vec::new() };
        let conn = n
            .agent
            .create_invitation_with(&mut n.store, pq, short, data)
            .unwrap();
        let (full, short_link) = until(&mut n, &mut c, |n, _| {
            n.take(|e| match e {
                AgentEvent::Invitation {
                    conn: x,
                    full,
                    short,
                } if *x == conn => Some((full.clone(), short.clone())),
                _ => None,
            })
        });
        let link = if short {
            short_link.expect("a short link")
        } else {
            full
        };
        let r = c.cmd(&format!("/connect {link}"));
        assert_ne!(kind(&r), "chatCmdError", "{r}");
        let info = until(&mut n, &mut c, |n, _| {
            n.take(|e| match e {
                AgentEvent::Conf { conn: x, conn_info } if *x == conn => Some(conn_info.clone()),
                _ => None,
            })
        });
        assert!(profile_name(&info).is_some(), "the CLI's x.info");
        n.agent.allow(&mut n.store, conn, x_info(&name)).unwrap();
        let cid = until(&mut n, &mut c, |n, c| {
            has(n, &AgentEvent::Con { conn })
                .then(|| c.take(|e| connected(e, &name)))
                .flatten()
        });
        let ratchet = n.agent.conn(conn).unwrap().ratchet.as_ref().unwrap();
        assert_eq!(ratchet.pq_support().0, pq);
        exchange(&mut n, &mut c, conn, cid);
    }
}

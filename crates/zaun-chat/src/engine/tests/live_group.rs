//! Groups against two official CLIs 7.1.4 (bob and carol): the engine as host, as a member a CLI
//! invites, and joining by a group link. Same servers as `live_cli`, plus `ZAUN_TEST_CLI2` for the
//! second CLI (started with `--user-display-name carol`); the first is bob.

use std::time::Duration;

use serde_json::{Value, json};
use zaun_chat_wire::model::{ChatRef, Membership, MessageAuthor, MessageBody};
use zaun_chat_wire::{Event, Request};

use super::Dir;
use super::live_cli::{Native, engine_msg, var};
use crate::agent::tests::cli::{Cli, connected, kind};
use crate::engine::{Clock, SystemClock};

struct World {
    n: Native,
    bob: Cli,
    carol: Cli,
}

impl World {
    fn start(dir: &Dir) -> World {
        World {
            n: Native::start(dir),
            bob: Cli::open(var("ZAUN_TEST_CLI").parse().unwrap()),
            carol: Cli::open(var("ZAUN_TEST_CLI2").parse().unwrap()),
        }
    }

    /// Runs all three until `f` finds what it waits for.
    fn until<T>(&mut self, what: &str, mut f: impl FnMut(&mut World) -> Option<T>) -> T {
        let clock = SystemClock;
        let start = clock.wall_ms();
        loop {
            if let Some(t) = f(self) {
                return t;
            }
            assert!(
                clock.wall_ms() - start < 60_000,
                "timed out waiting for {what}; engine {:?}; bob {:?}; carol {:?}",
                self.n.events.iter().rev().take(3).collect::<Vec<_>>(),
                short(&self.bob.events),
                short(&self.carol.events),
            );
            self.n.step(Duration::from_millis(5));
            self.bob.poll(Duration::from_millis(5));
            self.carol.poll(Duration::from_millis(5));
        }
    }

    fn cli(&mut self, who: Who) -> &mut Cli {
        match who {
            Who::Bob => &mut self.bob,
            Who::Carol => &mut self.carol,
        }
    }
}

#[derive(Clone, Copy)]
enum Who {
    Bob,
    Carol,
}

impl Who {
    const fn name(self) -> &'static str {
        match self {
            Self::Bob => "bob",
            Self::Carol => "carol",
        }
    }
}

fn short(events: &[Value]) -> Vec<String> {
    let s = events
        .iter()
        .map(|e| e.to_string().chars().take(300).collect());
    s.collect()
}

/// The engine and a CLI become friends: (the CLI's contact id for alice, the engine's friend id).
fn befriend(w: &mut World, who: Who) -> (i64, i64) {
    w.n.events.clear();
    w.n.req(Request::CreateInvite);
    let link = w.until("the invitation", |w| {
        w.n.events.iter().find_map(|e| match e {
            Event::Invite(l) => Some(l.clone()),
            _ => None,
        })
    });
    let r = w.cli(who).cmd(&format!("/connect {link}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let cid = w.until("the CLI's contact", |w| {
        w.cli(who).take(|e| connected(e, "alice"))
    });
    let friend = w.until("the engine's friend", |w| {
        let f = w.n.e.state.friends.values();
        f.filter(|f| f.name == who.name() && f.ready)
            .map(|f| f.id)
            .next()
    });
    (cid, friend)
}

/// Bob and carol become contacts of each other: bob's contact id for carol.
fn cli_friends(w: &mut World) -> i64 {
    let r = w.bob.cmd("/_connect 1");
    assert_eq!(kind(&r), "invitation", "{r}");
    let link = r
        .pointer("/connLinkInvitation/connFullLink")
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    let r = w.carol.cmd(&format!("/_connect 1 {link}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    w.until("bob's contact carol", |w| {
        w.bob.take(|e| connected(e, "carol"))
    })
}

fn group_id(e: &Value) -> Option<i64> {
    e.pointer("/groupInfo/groupId")?.as_i64()
}

/// A CLI accepts the invitation it got: its group id there.
fn cli_joins(w: &mut World, who: Who) -> i64 {
    let gid = w.until("the CLI's invitation", |w| {
        w.cli(who)
            .take(|e| (kind(e) == "receivedGroupInvitation").then(|| group_id(e))?)
    });
    let r = w.cli(who).cmd(&format!("/_join #{gid}"));
    assert_eq!(kind(&r), "userAcceptedGroupSent", "{r}");
    w.until("the CLI joined", |w| {
        w.cli(who)
            .take(|e| (kind(e) == "userJoinedGroup" && group_id(e) == Some(gid)).then_some(()))
    });
    gid
}

fn cli_group_send(w: &mut World, who: Who, gid: i64, text: &str) {
    let items = json!([{ "msgContent": { "type": "text", "text": text } }]);
    let r = w.cli(who).cmd(&format!("/_send #{gid} json {items}"));
    assert_eq!(kind(&r), "newChatItems", "{r}");
}

/// A text in group `gid` from the member named `from`, as a CLI received it.
fn cli_got(e: &Value, gid: i64, from: &str, text: &str) -> bool {
    kind(e) == "newChatItems"
        && e.get("chatItems")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|ci| {
                    ci.pointer("/chatInfo/groupInfo/groupId")
                        .and_then(Value::as_i64)
                        == Some(gid)
                        && ci
                            .pointer("/chatItem/chatDir/groupMember/memberProfile/displayName")
                            .and_then(Value::as_str)
                            == Some(from)
                        && ci
                            .pointer("/chatItem/content/msgContent/text")
                            .and_then(Value::as_str)
                            == Some(text)
                })
            })
}

fn wait_cli_got(w: &mut World, who: Who, gid: i64, from: &str, text: &str) {
    w.until(&format!("{} reads {text:?}", who.name()), |w| {
        w.cli(who)
            .take(|e| cli_got(e, gid, from, text).then_some(()))
    });
}

fn wait_engine_got(w: &mut World, group: i64, from: &str, text: &str) {
    w.until(&format!("alice reads {text:?}"), |w| {
        engine_msg(&mut w.n, |m| {
            m.chat == ChatRef::Group(group)
                && m.author == MessageAuthor::Member(from.into())
                && m.body == MessageBody::Text(text.into())
        })
    });
}

fn engine_send(w: &mut World, group: i64, text: &str) {
    w.n.req(Request::Send {
        chat: ChatRef::Group(group),
        text: text.into(),
        reply_to: None,
        mentions: Vec::new(),
    });
}

/// The engine's group named `name` once the page shows its membership as `m`.
fn engine_group(w: &mut World, name: &str, m: Membership) -> i64 {
    w.until(&format!("the engine's group {name} as {m:?}"), |w| {
        w.n.req(Request::Refresh);
        w.n.events.iter().rev().find_map(|e| match e {
            Event::Chats { groups, .. } => groups
                .iter()
                .find(|g| g.name == name && g.membership == m)
                .map(|g| g.id),
            _ => None,
        })
    })
}

#[test]
#[ignore = "needs a local smp-server and two CLIs; see the module comment"]
fn the_engine_hosts_a_group_that_two_official_clis_join_and_talk_in() {
    let dir = Dir::new();
    let mut w = World::start(&dir);
    let (_, bob) = befriend(&mut w, Who::Bob);
    let (_, carol) = befriend(&mut w, Who::Carol);
    w.n.req(Request::NewGroup("team".into()));
    let g = engine_group(&mut w, "team", Membership::Member);
    w.n.req(Request::AddMember {
        group: g,
        friend: bob,
    });
    let bg = cli_joins(&mut w, Who::Bob);
    w.n.req(Request::AddMember {
        group: g,
        friend: carol,
    });
    let cg = cli_joins(&mut w, Who::Carol);
    // The host introduced carol to bob: they connect to each other.
    w.until("bob and carol connected", |w| {
        w.bob.take(|e| {
            let m = e.pointer("/member/memberProfile/displayName")?.as_str()?;
            (kind(e) == "connectedToGroupMember" && m == "carol").then_some(())
        })
    });

    engine_send(&mut w, g, "hello team");
    wait_cli_got(&mut w, Who::Bob, bg, "alice", "hello team");
    wait_cli_got(&mut w, Who::Carol, cg, "alice", "hello team");
    cli_group_send(&mut w, Who::Bob, bg, "hi from bob");
    wait_engine_got(&mut w, g, "bob", "hi from bob");
    wait_cli_got(&mut w, Who::Carol, cg, "bob", "hi from bob");
}

#[test]
#[ignore = "needs a local smp-server and two CLIs; see the module comment"]
fn the_engine_joins_a_clis_group_and_meets_a_member_who_joins_after_it() {
    let dir = Dir::new();
    let mut w = World::start(&dir);
    let (alice_at_bob, _) = befriend(&mut w, Who::Bob);
    let carol_at_bob = cli_friends(&mut w);
    let r = w.bob.cmd(&format!(
        "/_group 1 {}",
        json!({"displayName": "club", "fullName": ""})
    ));
    assert_eq!(kind(&r), "groupCreated", "{r}");
    let bg = group_id(&r).unwrap();
    let r = w.bob.cmd(&format!("/_add #{bg} {alice_at_bob} member"));
    assert_eq!(kind(&r), "sentGroupInvitation", "{r}");
    let g = engine_group(&mut w, "club", Membership::Invited);
    w.n.req(Request::JoinGroup(g));
    engine_group(&mut w, "club", Membership::Member);

    cli_group_send(&mut w, Who::Bob, bg, "welcome alice");
    wait_engine_got(&mut w, g, "bob", "welcome alice");
    engine_send(&mut w, g, "thanks bob");
    wait_cli_got(&mut w, Who::Bob, bg, "alice", "thanks bob");

    // Carol joins after the engine: bob forwards the engine its invitation to her.
    let r = w.bob.cmd(&format!("/_add #{bg} {carol_at_bob} member"));
    assert_eq!(kind(&r), "sentGroupInvitation", "{r}");
    let cg = cli_joins(&mut w, Who::Carol);
    w.until("carol connected to alice", |w| {
        w.carol.take(|e| {
            let m = e.pointer("/member/memberProfile/displayName")?.as_str()?;
            (kind(e) == "connectedToGroupMember" && m == "alice").then_some(())
        })
    });
    cli_group_send(&mut w, Who::Carol, cg, "hi alice");
    wait_engine_got(&mut w, g, "carol", "hi alice");
    engine_send(&mut w, g, "hi carol");
    wait_cli_got(&mut w, Who::Carol, cg, "alice", "hi carol");
}

#[test]
#[ignore = "needs a local smp-server and two CLIs; see the module comment"]
fn the_engine_joins_a_clis_group_by_its_link() {
    let dir = Dir::new();
    let mut w = World::start(&dir);
    let r = w.bob.cmd(&format!(
        "/_group 1 {}",
        json!({"displayName": "open", "fullName": ""})
    ));
    assert_eq!(kind(&r), "groupCreated", "{r}");
    let bg = group_id(&r).unwrap();
    let r = w.bob.cmd(&format!("/_create link #{bg} member"));
    assert_eq!(kind(&r), "groupLinkCreated", "{r}");
    let link = r
        .pointer("/groupLink/connLinkContact/connShortLink")
        .or_else(|| r.pointer("/groupLink/connLinkContact/connFullLink"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    w.n.req(Request::Connect(link));
    let g = engine_group(&mut w, "open", Membership::Member);
    w.until("bob sees alice join", |w| {
        w.bob.take(|e| {
            let m = e.pointer("/member/memberProfile/displayName")?.as_str()?;
            (kind(e) == "joinedGroupMember" || kind(e) == "connectedToGroupMember")
                .then_some(())
                .filter(|()| m == "alice")
        })
    });
    cli_group_send(&mut w, Who::Bob, bg, "glad you came");
    wait_engine_got(&mut w, g, "bob", "glad you came");
    engine_send(&mut w, g, "glad to be here");
    wait_cli_got(&mut w, Who::Bob, bg, "alice", "glad to be here");
}

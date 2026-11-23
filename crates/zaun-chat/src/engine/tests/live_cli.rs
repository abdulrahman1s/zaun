//! The engine against the official CLI 7.1.4 over a local smp-server and xftp-server. Run with
//! `ZAUN_TEST_SMP`, `ZAUN_TEST_XFTP`, `ZAUN_TEST_CLI=<port of a CLI started with -p, -s, --xftp-server,
//! --files-folder /home/cli/files>` and `ZAUN_TEST_CLI_HOME=<that CLI's home on this computer>`.

use std::path::PathBuf;
use std::time::Duration;

use crossbeam_channel::{Receiver, Select};
use serde_json::{Value, json};
use zaun_chat_wire::model::{
    CallRequest, CallSignal, ChatMessage, ChatRef, FileState, MessageAuthor, MessageBody, Picture,
};
use zaun_chat_wire::{Event, FileKind, Hello, Request};

use super::{Dir, KEY};
use crate::agent::tests::cli::{Cli, connected, kind, received_text};
use crate::engine::jobs::JobDone;
use crate::engine::log::Log;
use crate::engine::{Clock, Engine, NativeAgent, SystemClock};
use crate::smp::SmpEvent;
use crate::store::Store;

pub(super) fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set"))
}

pub(super) struct Native {
    pub(super) e: Engine<NativeAgent>,
    smp: Receiver<SmpEvent>,
    jobs: Receiver<JobDone>,
    pub(super) events: Vec<Event>,
}

impl Native {
    pub(super) fn start(dir: &Dir) -> Native {
        let relays = vec![var("ZAUN_TEST_SMP"), var("ZAUN_TEST_XFTP")];
        let hello = Hello::new(dir.0.clone(), KEY.into(), "alice".into(), relays);
        let log = Log::new(std::io::stderr());
        let (agent, smp) = NativeAgent::new(&hello.relays, log.clone());
        let store = Store::open(&dir.0.join("chat.db"), KEY).unwrap();
        let mut e = Engine::new(store, agent, Box::new(SystemClock), log);
        e.start(&hello).unwrap();
        let jobs = e.job_results();
        Native {
            e,
            smp,
            jobs,
            events: Vec::new(),
        }
    }

    /// One turn of the run loop, at most `wait` long.
    pub(super) fn step(&mut self, wait: Duration) {
        let mut sel = Select::new();
        let net = sel.recv(&self.smp);
        let job = sel.recv(&self.jobs);
        match sel.select_timeout(wait.min(self.e.sleep_for())) {
            Ok(op) if op.index() == net => {
                if let Ok(ev) = op.recv(&self.smp) {
                    self.e.on_smp(ev);
                }
            }
            Ok(op) if op.index() == job => {
                if let Ok(d) = op.recv(&self.jobs) {
                    self.e.on_job(d);
                }
            }
            Ok(_) | Err(_) => self.e.tick(),
        }
        while let Some(ev) = self.e.pop_out() {
            self.events.push(ev);
        }
    }

    pub(super) fn req(&mut self, r: Request) {
        self.e.on_request(r);
        while let Some(ev) = self.e.pop_out() {
            self.events.push(ev);
        }
    }
}

/// Runs both sides until `f` finds what it waits for.
pub(super) fn until<T>(
    n: &mut Native,
    c: &mut Cli,
    what: &str,
    mut f: impl FnMut(&mut Native, &mut Cli) -> Option<T>,
) -> T {
    let clock = SystemClock;
    let start = clock.wall_ms();
    loop {
        if let Some(t) = f(n, c) {
            return t;
        }
        assert!(
            clock.wall_ms() - start < 60_000,
            "timed out waiting for {what}; engine {:?}; cli {:?}",
            n.events.iter().rev().take(3).collect::<Vec<_>>(),
            c.events
                .iter()
                .map(|e| {
                    let s = e.to_string();
                    s.chars().take(400).collect::<String>()
                })
                .collect::<Vec<_>>()
        );
        n.step(Duration::from_millis(5));
        c.poll(Duration::from_millis(5));
    }
}

/// A message the engine reported (new or changed) that `f` accepts.
pub(super) fn engine_msg(n: &mut Native, f: impl Fn(&ChatMessage) -> bool) -> Option<ChatMessage> {
    let i = n.events.iter().position(|e| match e {
        Event::Messages(m) | Event::Updated(m) => m.iter().any(&f),
        _ => false,
    })?;
    match n.events.remove(i) {
        Event::Messages(m) | Event::Updated(m) => m.into_iter().find(|m| f(m)),
        _ => None,
    }
}

pub(super) fn cli_event(c: &mut Cli, k: &str, f: impl Fn(&Value) -> bool) -> Option<Value> {
    c.take(|e| (kind(e) == k && f(e)).then(|| e.clone()))
}

fn item_text(e: &Value) -> Option<&str> {
    e.pointer("/chatItem/chatItem/content/msgContent/text")?
        .as_str()
}

pub(super) fn connect(n: &mut Native, c: &mut Cli) -> (i64, i64) {
    n.req(Request::CreateInvite);
    let link = until(n, c, "the invitation", |n, _| {
        n.events.iter().find_map(|e| {
            if let Event::Invite(l) = e {
                Some(l.clone())
            } else {
                None
            }
        })
    });
    let r = c.cmd(&format!("/connect {link}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    let cid = until(n, c, "the CLI's contact", |_, c| {
        c.take(|e| connected(e, "alice"))
    });
    let friend = until(n, c, "the engine's friend", |n, _| {
        n.e.state
            .friends
            .values()
            .find(|f| f.name == "bob" && f.ready)
            .map(|f| f.id)
    });
    (cid, friend)
}

pub(super) fn cli_send(c: &mut Cli, cid: i64, content: Value) -> i64 {
    let r = c.cmd(&format!("/_send @{cid} json {}", json!([content])));
    assert_eq!(kind(&r), "newChatItems", "{r}");
    r.pointer("/chatItems/0/chatItem/meta/itemId")
        .and_then(Value::as_i64)
        .unwrap()
}

#[test]
#[ignore = "needs a local smp-server, xftp-server and CLI; see the module comment"]
fn the_engine_and_the_official_cli_chat_edit_react_delete_verify_share_files_and_call() {
    let dir = Dir::new();
    let mut n = Native::start(&dir);
    let mut c = Cli::open(var("ZAUN_TEST_CLI").parse().unwrap());
    let (cid, bob) = connect(&mut n, &mut c);
    let chat = ChatRef::Direct(bob);

    // Text both ways.
    n.req(Request::Send {
        chat,
        text: "hi bob".into(),
        reply_to: None,
        mentions: Vec::new(),
    });
    let mine = engine_msg(&mut n, |m| m.author == MessageAuthor::Me).unwrap();
    until(&mut n, &mut c, "bob reads the text", |_, c| {
        c.take(|e| received_text(e, cid).filter(|t| t == "hi bob"))
    });
    let bobs_item = cli_send(
        &mut c,
        cid,
        json!({"msgContent": {"type": "text", "text": "hi alice"}}),
    );
    let theirs = until(&mut n, &mut c, "alice reads the text", |n, _| {
        engine_msg(n, |m| m.body == MessageBody::Text("hi alice".into()))
    });

    // The engine's edit, reaction and deletion reach the CLI.
    n.req(Request::Edit {
        chat,
        item: mine.id,
        text: "hi bob!".into(),
    });
    until(&mut n, &mut c, "bob sees the edit", |_, c| {
        cli_event(c, "chatItemUpdated", |e| item_text(e) == Some("hi bob!"))
    });
    n.req(Request::React {
        chat,
        item: theirs.id,
        emoji: "❤️".into(),
        on: true,
    });
    until(&mut n, &mut c, "bob sees the reaction", |_, c| {
        cli_event(c, "chatItemReaction", |e| {
            e.get("added") == Some(&json!(true))
        })
    });
    n.req(Request::Delete {
        chat,
        item: mine.id,
    });
    until(&mut n, &mut c, "bob sees the deletion", |_, c| {
        cli_event(c, "chatItemsDeleted", |_| true)
    });

    // The CLI's edit, reaction and deletion reach the engine.
    let upd = json!({"msgContent": {"type": "text", "text": "hi alice!"}, "mentions": {}});
    let r = c.cmd(&format!("/_update item @{cid} {bobs_item} json {upd}"));
    assert_eq!(kind(&r), "chatItemUpdated", "{r}");
    until(&mut n, &mut c, "alice sees the edit", |n, _| {
        engine_msg(n, |m| {
            m.id == theirs.id && m.edited && m.body == MessageBody::Text("hi alice!".into())
        })
    });
    let r = c.cmd(&format!(
        "/_reaction @{cid} {bobs_item} on {}",
        json!({"type": "emoji", "emoji": "🚀"})
    ));
    assert_eq!(kind(&r), "chatItemReaction", "{r}");
    until(&mut n, &mut c, "alice sees the reaction", |n, _| {
        engine_msg(n, |m| {
            m.id == theirs.id && m.reactions.iter().any(|r| r.emoji == "🚀" && !r.mine)
        })
    });
    let r = c.cmd(&format!("/_delete item @{cid} {bobs_item} broadcast"));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "alice sees the deletion", |n, _| {
        engine_msg(n, |m| m.id == theirs.id && m.body == MessageBody::Deleted)
    });

    // Both sides show the same security code.
    n.req(Request::ShowCode(bob));
    let code = until(&mut n, &mut c, "alice's code", |n, _| {
        n.events.iter().find_map(|e| {
            if let Event::Code { code, .. } = e {
                Some(code.clone())
            } else {
                None
            }
        })
    });
    let r = c.cmd(&format!("/_get code @{cid}"));
    assert_eq!(
        r.get("connectionCode").and_then(Value::as_str),
        Some(code.as_str()),
        "{r}"
    );

    // The picture arrives in the profile.
    let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe0];
    jpeg.extend(0..64u8);
    n.req(Request::SetPicture(Some(Picture {
        bytes: jpeg,
        mime: "image/jpeg".into(),
    })));
    until(&mut n, &mut c, "bob sees the picture", |_, c| {
        cli_event(c, "contactUpdated", |e| {
            e.pointer("/toContact/profile/image")
                .and_then(Value::as_str)
                .is_some_and(|i| i.starts_with("data:image/jpg"))
        })
    });

    // A file each way, byte for byte.
    let bytes: Vec<u8> = (0..200_000u32).map(|i| (i * 7 % 251) as u8).collect();
    let files = dir.0.join("files");
    std::fs::create_dir_all(&files).unwrap();
    std::fs::write(files.join("from-alice.bin"), &bytes).unwrap();
    n.req(Request::SendFile {
        chat,
        path: files.join("from-alice.bin"),
        kind: FileKind::File,
        preview: None,
        duration: None,
        text: String::new(),
    });
    let file_id = until(&mut n, &mut c, "bob is offered the file", |_, c| {
        cli_event(c, "newChatItems", |e| {
            e.pointer("/chatItems/0/chatItem/file/fileId").is_some()
        })
        .and_then(|e| e.pointer("/chatItems/0/chatItem/file/fileId")?.as_i64())
    });
    let r = c.cmd(&format!("/freceive {file_id}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "bob has the file", |_, c| {
        cli_event(c, "rcvFileComplete", |_| true)
    });
    let home = PathBuf::from(var("ZAUN_TEST_CLI_HOME"));
    assert_eq!(
        std::fs::read(home.join("files/from-alice.bin")).unwrap(),
        bytes
    );

    std::fs::write(home.join("files/from-bob.bin"), &bytes[..150_000]).unwrap();
    cli_send(
        &mut c,
        cid,
        json!({"filePath": "/home/cli/files/from-bob.bin", "msgContent": {"type": "file", "text": ""}}),
    );
    let offered = until(&mut n, &mut c, "alice is offered the file", |n, _| {
        engine_msg(
            n,
            |m| matches!(&m.body, MessageBody::File(x) if x.name == "from-bob.bin"),
        )
    });
    // The description may still be on its way: ask until the fetch starts or completes.
    n.req(Request::ReceiveFile {
        chat,
        item: offered.id,
    });
    let local = until(&mut n, &mut c, "alice has the file", |n, _| {
        engine_msg(n, |m| m.id == offered.id && matches!(&m.body, MessageBody::File(x) if matches!(x.state, FileState::Received(_))))
            .and_then(|m| match m.body { MessageBody::File(x) => match x.state { FileState::Received(p) => Some(p), _ => None }, _ => None })
    });
    assert_eq!(std::fs::read(local).unwrap(), &bytes[..150_000]);

    // A call invitation from the app rings here, with the frame key.
    let call = json!({"media": "audio", "capabilities": {"encryption": true}});
    let r = c.cmd(&format!("/_call invite @{cid} {call}"));
    assert!(!kind(&r).contains("Error"), "{r}");
    until(&mut n, &mut c, "alice is called", |n, _| {
        n.events.iter().find_map(|e| match e {
            Event::Call(CallSignal::Invited {
                usable: true,
                key: Some(_),
                ..
            }) => Some(()),
            _ => None,
        })
    });
    n.req(Request::Call(CallRequest::Decline(bob)));
    until(&mut n, &mut c, "bob's call ends", |_, c| {
        cli_event(c, "callEnded", |_| true)
    });
}

//! Two native helpers through a real smp-server. Start a local smp-server and run
//! `ZAUN_TEST_SMP=smp://<fingerprint>[:<password>]@127.0.0.1:<port> cargo test -p zaun-chat --test it engine_live -- --ignored`.
#![allow(
    clippy::disallowed_methods,
    reason = "the tests start the helper they test and wait on real time"
)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use zaun_chat_wire::model::{ChatMessage, ChatRef, Delivery, Friend, MessageBody};
use zaun_chat_wire::{EvLine, Event, Hello, ReqLine, Request, decode_line, encode_line};

const WAIT: Duration = Duration::from_secs(60);

fn relay() -> String {
    let Ok(addr) = std::env::var("ZAUN_TEST_SMP") else {
        panic!("ZAUN_TEST_SMP is not set")
    };
    addr
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("zaun-live-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One `zaun-chat` process and what it said.
struct Helper {
    child: Child,
    stdin: Option<ChildStdin>,
    events: Receiver<Event>,
}

impl Helper {
    fn spawn(dir: &Path, name: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_zaun-chat"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let hello = Hello::new(dir.to_owned(), "key".into(), name.into(), vec![relay()]);
        writeln!(stdin, "{}", encode_line(&hello).unwrap()).unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let (tx, events) = channel();
        std::thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                let Ok(EvLine { ev }) = decode_line(&line) else {
                    continue;
                };
                if tx.send(ev).is_err() {
                    return;
                }
            }
        });
        Self {
            child,
            stdin: Some(stdin),
            events,
        }
    }

    fn send(&mut self, req: Request) {
        let line = encode_line(&ReqLine { req }).unwrap();
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    /// The first later event `pick` takes; the ones before it are dropped.
    fn wait<T>(&mut self, what: &str, pick: impl Fn(&Event) -> Option<T>) -> T {
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let ev = self
                .events
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
            if let Event::Failed {
                request,
                code,
                detail,
            } = &ev
            {
                panic!("{request:?} failed: {code} {detail}");
            }
            if let Some(t) = pick(&ev) {
                return t;
            }
        }
    }

    fn ready_friend(&mut self) -> Friend {
        self.wait("a ready friend", |e| match e {
            Event::Chats { friends, .. } => friends.iter().find(|f| f.ready).cloned(),
            _ => None,
        })
    }

    fn text(&mut self, what: &str, text: &str) -> ChatMessage {
        self.wait(what, |e| match e {
            Event::Messages(ms) => ms
                .iter()
                .find(|m| m.body == MessageBody::Text(text.to_owned()))
                .cloned(),
            _ => None,
        })
    }

    fn delivered(&mut self, text: &str) {
        self.wait("a delivery receipt", |e| match e {
            Event::Updated(ms) => ms
                .iter()
                .any(|m| {
                    m.body == MessageBody::Text(text.to_owned())
                        && m.delivery == Delivery::Delivered
                })
                .then_some(()),
            _ => None,
        });
    }

    /// Closes stdin; the helper must exit 0 within 5 seconds.
    fn close(mut self) {
        drop(self.stdin.take());
        let started = Instant::now();
        let status = self.child.wait().unwrap();
        assert!(started.elapsed() < Duration::from_secs(5), "slow exit");
        assert_eq!(status.code(), Some(0));
    }
}

fn say(from: &mut Helper, friend: &Friend, text: &str) {
    from.send(Request::Send {
        chat: ChatRef::Direct(friend.id),
        text: text.to_owned(),
        reply_to: None,
        mentions: Vec::new(),
    });
}

#[test]
#[ignore = "needs a local smp-server; set ZAUN_TEST_SMP"]
fn two_native_engines_connect_exchange_messages_with_receipts_and_survive_a_restart() {
    let (dir_a, dir_b) = (Dir::new("alice"), Dir::new("bob"));
    let mut a = Helper::spawn(&dir_a.0, "alice");
    let mut b = Helper::spawn(&dir_b.0, "bob");

    a.send(Request::CreateInvite);
    let link = a.wait("an invitation", |e| match e {
        Event::Invite(l) => Some(l.clone()),
        _ => None,
    });
    b.send(Request::Connect(link));
    let (for_a, for_b) = (a.ready_friend(), b.ready_friend());
    assert_eq!((for_a.name.as_str(), for_b.name.as_str()), ("bob", "alice"));

    say(&mut a, &for_a, "hello bob");
    b.text("alice's text", "hello bob");
    a.delivered("hello bob");

    b.close();
    let mut b = Helper::spawn(&dir_b.0, "bob");
    say(&mut a, &for_a, "while you restarted");
    b.text("the text after the restart", "while you restarted");
    a.delivered("while you restarted");
    say(&mut b, &for_b, "welcome back");
    a.text("bob's text after the restart", "welcome back");

    a.close();
    b.close();
}

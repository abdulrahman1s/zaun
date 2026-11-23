//! The run loop behind the pipes (codec: requests_made_while_starting_…, a_database_that_does_not_open_…).

use std::io::{Cursor, Write};
use std::sync::{Arc, Mutex};

use crossbeam_channel::never;
use zaun_chat_wire::{EvLine, Event, Hello, ReqLine, Request, decode_line, encode_line};

use super::{Dir, FakeAgent, KEY};
use crate::engine::clock::FakeClock;
use crate::wire::entry::{EXIT_DATABASE, EXIT_FATAL, EXIT_OK, run_with};

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn input(hello: &Hello, requests: &[Request]) -> Cursor<Vec<u8>> {
    let mut s = encode_line(hello).unwrap() + "\n";
    for r in requests {
        s += &(encode_line(&ReqLine { req: r.clone() }).unwrap() + "\n");
    }
    Cursor::new(s.into_bytes())
}

fn run(stdin: Cursor<Vec<u8>>) -> (i32, Vec<Event>) {
    let out = Shared::default();
    let code = run_with(
        stdin,
        out.clone(),
        std::io::sink(),
        Box::new(FakeClock::new(0)),
        |_, _| (FakeAgent::default(), never()),
    );
    let text = String::from_utf8(out.0.lock().unwrap().clone()).unwrap();
    let events = text
        .lines()
        .map(|l| decode_line::<EvLine>(l).unwrap().ev)
        .collect();
    (code, events)
}

#[test]
fn requests_made_while_starting_wait_for_the_start_and_closing_stdin_exits_zero() {
    let dir = Dir::new();
    let (code, events) = run(input(&dir.hello("alice"), &[Request::Refresh]));
    assert_eq!(code, EXIT_OK);
    assert!(matches!(events[0], Event::Features { .. }));
    assert!(matches!(events[1], Event::Ready(_)));
    assert!(matches!(events[2], Event::Chats { .. }));
    assert!(
        matches!(events[3], Event::Chats { .. }),
        "the refresh, after the start"
    );
    assert_eq!(events.len(), 4);
}

#[test]
fn a_database_that_does_not_open_with_the_key_exits_3() {
    let dir = Dir::new();
    assert_eq!(run(input(&dir.hello("alice"), &[])).0, EXIT_OK);
    let mut wrong = dir.hello("alice");
    wrong.key = format!("{KEY} but not");
    let (code, events) = run(input(&wrong, &[]));
    assert_eq!((code, events.len()), (EXIT_DATABASE, 0));
}

#[test]
fn a_first_line_that_is_not_a_hello_exits_4() {
    assert_eq!(run(Cursor::new(b"{\"req\":1}\n".to_vec())).0, EXIT_FATAL);
    let dir = Dir::new();
    let mut old = dir.hello("alice");
    old.v = 1;
    assert_eq!(run(input(&old, &[])).0, EXIT_FATAL);
}

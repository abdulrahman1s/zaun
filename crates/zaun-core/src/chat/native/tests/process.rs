//! The helper as a real process: a shell script that speaks the wire.

use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zaun_chat_wire::model::ChatProfile;
use zaun_chat_wire::{EvLine, Event, encode_line};

use crate::chat::engine::{
    ChatEvent, Engine, EngineConfig, EngineFactory, EventSink, Request, RequestKind,
};
use crate::chat::native::NativeFactory;
use crate::error::ErrorCode;
use crate::fsx::{Fs as _, RealFs};

/// Writing a script and starting it races with other forks (ETXTBSY): one at a time.
static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());
const WAIT: Duration = Duration::from_secs(10);

fn line(ev: Event) -> String {
    encode_line(&EvLine { ev }).unwrap()
}

fn start(dir: &Path, script: &str) -> (Box<dyn Engine>, Receiver<ChatEvent>) {
    let path = dir.join("helper");
    RealFs
        .write_new(
            &path,
            format!(
                "#!/bin/sh
{script}
"
            )
            .as_bytes(),
            0o755,
        )
        .unwrap();
    let factory = NativeFactory {
        helper: path,
        bwrap: None,
        log: Some(dir.join("log")),
    };
    assert!(factory.available());
    let (tx, rx) = channel();
    let sink: EventSink = Arc::new(move |e| {
        let _ = tx.send(e);
    });
    let config = EngineConfig {
        dir: dir.to_owned(),
        key: "k".into(),
        name: "Ada".into(),
        relays: vec![],
    };
    (factory.start(config, sink).unwrap(), rx)
}

fn next(rx: &Receiver<ChatEvent>) -> ChatEvent {
    rx.recv_timeout(WAIT).expect("an event")
}

#[test]
fn the_helper_is_started_with_a_hello_and_its_events_reach_the_sink() {
    let _g = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let ready = line(Event::Ready(ChatProfile {
        name: "Ada".into(),
        ..ChatProfile::default()
    }));
    let code = line(Event::Code {
        friend: 2,
        code: "7".into(),
    });
    // The hello names the data folder and the name; a bad line in between is skipped.
    let script = format!(
        "read hello\ncase \"$hello\" in *'\"v\":2'*'\"name\":\"Ada\"'*) ;; *) exit 9;; esac\n\
         printf '%s\\n' '{ready}' 'not json' '{code}'\nwhile read _; do :; done"
    );
    let (engine, rx) = start(dir.path(), &script);
    assert!(matches!(next(&rx), ChatEvent::Ready(p) if p.name == "Ada"));
    assert_eq!(
        next(&rx),
        ChatEvent::Code {
            friend: 2,
            code: "7".into()
        }
    );
    drop(engine);
    assert_eq!(
        next(&rx),
        ChatEvent::Exited {
            status: Some(0),
            started: true
        }
    );
}

#[test]
fn a_request_reaches_the_helper_as_a_line() {
    let _g = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let code = line(Event::Code {
        friend: 2,
        code: "echo".into(),
    });
    let script = format!(
        "read hello\nwhile read req; do case \"$req\" in *show_code*) printf '%s\\n' '{code}';; esac; done"
    );
    let (engine, rx) = start(dir.path(), &script);
    engine.request(Request::Refresh);
    engine.request(Request::ShowCode(2));
    assert!(matches!(next(&rx), ChatEvent::Code { code, .. } if code == "echo"));
}

#[test]
fn exit_status_3_is_a_database_failure_before_the_exit() {
    let _g = ONE_AT_A_TIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = tempfile::tempdir().unwrap();
    let (_engine, rx) = start(dir.path(), "read hello\nexit 3");
    assert!(matches!(
        next(&rx),
        ChatEvent::Failed {
            request: RequestKind::Start,
            code: ErrorCode::ChtDatabase,
            ..
        }
    ));
    assert_eq!(
        next(&rx),
        ChatEvent::Exited {
            status: Some(3),
            started: false
        }
    );
}

#[test]
fn a_missing_helper_is_unavailable() {
    let factory = NativeFactory {
        helper: "/nonexistent/zaun-chat".into(),
        bwrap: None,
        log: None,
    };
    assert!(!factory.available());
    let sink: EventSink = Arc::new(|_| {});
    let config = EngineConfig {
        dir: "/nonexistent".into(),
        key: "k".into(),
        name: "n".into(),
        relays: vec![],
    };
    let err = factory.start(config, sink).err().unwrap();
    assert_eq!(err.code, ErrorCode::ChtUnavailable);
}

//! The SMP server test on its own thread per relay (Client.hs `runSMPServerTest`: connect, NEW, SKEY or KEY, DEL, disconnect).

use std::thread;
use std::time::Instant;

use crossbeam_channel::{Sender, unbounded};

use super::tester::{RelayDone, RelayTester};
use crate::crypto::{Alg, KeyPair};
use crate::engine::jobs::JobDone;
use crate::smp::{
    Answer, Command, ConnConfig, NewQueue, QueueMode, Request, Response, SmpConnection,
    SmpEventKind, SmpServer, SubscriptionMode,
};

/// Where a test failed, as `ProtocolTestStep` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Connect,
    CreateQueue,
    SecureQueue,
    DeleteQueue,
}

/// A connected relay: the one thing the sequence needs.
pub(super) trait Session {
    fn call(&self, request: Request) -> Answer;
}

impl Session for SmpConnection {
    fn call(&self, request: Request) -> Answer {
        SmpConnection::call(self, request)
    }
}

/// NEW, then SKEY (a queue the sender secures) or KEY, then DEL, each answered or the test fails at that step.
pub(super) fn sequence(s: &dyn Session, basic_auth: Option<String>) -> Result<(), (Step, String)> {
    let at = |step| move |e: String| (step, e);
    let keys = |alg| KeyPair::generate(alg).map_err(|e| e.to_string());
    let (rcv, dh, snd) = (
        keys(Alg::Ed25519).map_err(at(Step::CreateQueue))?,
        keys(Alg::X25519).map_err(at(Step::CreateQueue))?,
        keys(Alg::X25519).map_err(at(Step::CreateQueue))?,
    );
    let mut new = NewQueue::messaging(rcv.public.clone(), dh.public, SubscriptionMode::Subscribe);
    new.basic_auth = basic_auth;
    let request = Request::new(
        Command::New(Box::new(new)),
        Vec::new(),
        Some(rcv.private.clone()),
    );
    let ids = match s.call(request).map_err(|e| e.to_string()) {
        Ok(Response::Ids(ids)) => ids,
        Ok(other) => return Err((Step::CreateQueue, format!("unexpected {}", other.name()))),
        Err(e) => return Err((Step::CreateQueue, e)),
    };
    let secure = match ids.queue_mode {
        Some(QueueMode::Messaging) => {
            Request::new(Command::Skey(snd.public), ids.snd_id, Some(snd.private))
        }
        Some(QueueMode::Contact) | None => Request::new(
            Command::Key(snd.public),
            ids.rcv_id.clone(),
            Some(rcv.private.clone()),
        ),
    };
    expect_ok(s, secure, Step::SecureQueue)?;
    let del = Request::new(Command::Del, ids.rcv_id, Some(rcv.private));
    expect_ok(s, del, Step::DeleteQueue)
}

fn expect_ok(s: &dyn Session, request: Request, step: Step) -> Result<(), (Step, String)> {
    match s.call(request) {
        Ok(Response::Ok) => Ok(()),
        Ok(other) => Err((step, format!("unexpected {}", other.name()))),
        Err(e) => Err((step, e.to_string())),
    }
}

#[allow(
    clippy::disallowed_methods,
    reason = "a test thread's own deadline, off the engine's clock"
)]
/// Waits for the session to come up; the connection retries forever, so the first failed
/// attempt or the deadline ends the wait.
fn connect(server: &SmpServer, cfg: &ConnConfig) -> Result<SmpConnection, (Step, String)> {
    let (tx, rx) = unbounded();
    let conn = SmpConnection::open(server.clone(), cfg.clone(), tx)
        .map_err(|e| (Step::Connect, e.to_string()))?;
    let deadline = Instant::now() + cfg.connect_timeout;
    let failed = loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(e) => match e.kind {
                SmpEventKind::Up(_) => return Ok(conn),
                SmpEventKind::ConnectFailed(err) => break err.to_string(),
                SmpEventKind::Down(_)
                | SmpEventKind::Msg { .. }
                | SmpEventKind::End { .. }
                | SmpEventKind::Deld { .. }
                | SmpEventKind::Reply { .. }
                | SmpEventKind::Unexpected { .. } => {}
            },
            Err(_) => break "timed out".to_owned(),
        }
    };
    conn.close();
    Err((Step::Connect, failed))
}

/// Tests `address` and posts the answer before the connection is closed, which may wait for the socket.
fn run_and_post(address: &str, post: impl FnOnce(Option<String>)) {
    let why = |(step, e): (Step, String)| Some(format!("{step:?}: {e}"));
    let server: SmpServer = match address.parse() {
        Ok(s) => s,
        Err(e) => return post(why((Step::Connect, format!("{e}")))),
    };
    match connect(&server, &ConnConfig::default()) {
        Err(e) => post(why(e)),
        Ok(conn) => {
            post(
                sequence(&conn, server.basic_auth.clone())
                    .err()
                    .and_then(why),
            );
            conn.close();
        }
    }
}

pub struct SmpTester;

impl RelayTester for SmpTester {
    fn test(&mut self, server: String, run: u64, tx: Sender<JobDone>) {
        let post = move |server: String, failure: Option<String>| {
            let _ = tx.send(JobDone::Relay(RelayDone {
                run,
                server,
                failure,
            })); // the engine is gone
        };
        let spawned = {
            let (server, post) = (server.clone(), post.clone());
            thread::Builder::new()
                .name("zaun-relay-test".into())
                .spawn(move || {
                    let address = server.clone();
                    run_and_post(&address, |f| post(server, f));
                })
        };
        if let Err(e) = spawned {
            post(server, Some(format!("no thread: {e}")));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::smp::{QueueIds, SmpError};

    /// Answers NEW with ids of a queue in `mode`, everything else with `rest`, and notes each command.
    struct Script {
        mode: Option<QueueMode>,
        new: Option<SmpError>,
        rest: Result<(), SmpError>,
        seen: RefCell<Vec<(&'static str, Vec<u8>)>>,
    }

    impl Script {
        fn new(mode: Option<QueueMode>) -> Self {
            Self {
                mode,
                new: None,
                rest: Ok(()),
                seen: RefCell::new(Vec::new()),
            }
        }

        fn names(&self) -> Vec<&'static str> {
            self.seen.borrow().iter().map(|(n, _)| *n).collect()
        }
    }

    impl Session for Script {
        fn call(&self, r: Request) -> Answer {
            let name = match &r.command {
                Command::New(_) => "NEW",
                Command::Skey(_) => "SKEY",
                Command::Key(_) => "KEY",
                Command::Del => "DEL",
                _ => "other",
            };
            self.seen.borrow_mut().push((name, r.entity_id.clone()));
            if name == "NEW" {
                if let Some(e) = &self.new {
                    return Err(e.clone());
                }
                let dh = KeyPair::generate(Alg::X25519).unwrap().public;
                return Ok(Response::Ids(QueueIds {
                    rcv_id: b"rcv".to_vec(),
                    snd_id: b"snd".to_vec(),
                    rcv_dh_key: dh,
                    queue_mode: self.mode,
                    link_id: None,
                    service_id: None,
                    ntf_creds: None,
                }));
            }
            self.rest.clone().map(|()| Response::Ok)
        }
    }

    #[test]
    fn a_messaging_queue_is_secured_by_the_sender_key_then_deleted() {
        let s = Script::new(Some(QueueMode::Messaging));
        assert_eq!(sequence(&s, None), Ok(()));
        assert_eq!(s.names(), ["NEW", "SKEY", "DEL"]);
        let seen = s.seen.borrow();
        assert_eq!(seen[1].1, b"snd", "SKEY goes to the sender id");
        assert_eq!(seen[2].1, b"rcv", "DEL goes to the recipient id");
    }

    #[test]
    fn a_queue_the_sender_cannot_secure_is_secured_by_the_recipient_with_key() {
        let s = Script::new(None);
        assert_eq!(sequence(&s, None), Ok(()));
        assert_eq!(s.names(), ["NEW", "KEY", "DEL"]);
        assert_eq!(s.seen.borrow()[1].1, b"rcv");
    }

    #[test]
    fn a_failure_names_the_step_it_happened_at() {
        let mut s = Script::new(Some(QueueMode::Messaging));
        s.new = Some(SmpError::Timeout);
        assert_eq!(sequence(&s, None).unwrap_err().0, Step::CreateQueue);
        assert_eq!(s.names(), ["NEW"], "nothing is sent after a failure");
        let mut s = Script::new(Some(QueueMode::Messaging));
        s.rest = Err(SmpError::Timeout);
        assert_eq!(sequence(&s, None).unwrap_err().0, Step::SecureQueue);
        assert_eq!(s.names(), ["NEW", "SKEY"]);
    }

    #[test]
    fn the_password_of_a_relay_goes_into_new() {
        struct Capture(RefCell<Option<String>>);
        impl Session for Capture {
            fn call(&self, r: Request) -> Answer {
                if let Command::New(n) = r.command {
                    *self.0.borrow_mut() = n.basic_auth;
                }
                Err(SmpError::Timeout)
            }
        }
        let c = Capture(RefCell::new(None));
        let _ = sequence(&c, Some("pw".into()));
        assert_eq!(c.0.borrow().as_deref(), Some("pw"));
    }
}

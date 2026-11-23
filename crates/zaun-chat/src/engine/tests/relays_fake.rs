//! A relay tester that records what it was asked and answers when a test says so.

use std::sync::{Arc, Mutex, PoisonError};

use crossbeam_channel::Sender;

use super::T;
use crate::engine::jobs::JobDone;
use crate::engine::relays::{RelayDone, RelayTester};

pub const LIVE: &str = "smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.1:5223";
pub const DEAD: &str = "smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.2:5999";
pub const THIRD: &str = "smp://jZxOZYTgS4Q3Mp5r4wJW5ruxuQAPb8y5pa4a1DI2l6Y=@127.0.0.3:5223";

/// Every (server, run) a check asked for, in order.
#[derive(Clone, Debug, Default)]
pub struct Fake(pub Arc<Mutex<Vec<(String, u64)>>>);

impl Fake {
    pub fn asked(&self) -> Vec<(String, u64)> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn run(&self) -> u64 {
        self.asked().last().map_or(0, |(_, r)| *r)
    }
}

impl RelayTester for Fake {
    fn test(&mut self, server: String, run: u64, _: Sender<JobDone>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((server, run));
    }
}

/// Started with `relays` as the user's own, the tester a fake.
pub fn started(relays: &[&str]) -> (T, Fake) {
    let mut t = T::new();
    let fake = Fake::default();
    t.e.relays.tester = Some(Box::new(fake.clone()));
    let mut hello = t.dir.hello("alice");
    hello.relays = relays.iter().map(|r| (*r).to_owned()).collect();
    t.e.start(&hello).unwrap();
    t.out();
    (t, fake)
}

impl T {
    /// `server` answers the running check: it passes, or fails with `why`.
    pub fn relay_answers(&mut self, fake: &Fake, server: &str, passes: bool) {
        let failure = (!passes).then(|| "BROKER NETWORK".to_owned());
        self.relay_answers_run(fake.run(), server, failure);
    }

    pub fn relay_answers_run(&mut self, run: u64, server: &str, failure: Option<String>) {
        let server = server.to_owned();
        self.e.on_job(JobDone::Relay(RelayDone {
            run,
            server,
            failure,
        }));
        self.out();
    }

    /// Time passes and the loop's timer runs.
    pub fn relay_time(&mut self, secs: u64) {
        self.clock.advance(std::time::Duration::from_secs(secs));
        self.e.tick();
        self.out();
    }
}

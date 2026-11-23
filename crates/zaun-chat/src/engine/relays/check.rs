//! A check: every own relay tested at once, the silent ones kept off new queues while another answers.

use std::collections::HashSet;

use zaun_chat_wire::Hello;

use super::{CHECK_DEADLINE, CHECK_EVERY, RelayDone, SmpTester};
use crate::engine::agent_port::AgentPort;
use crate::engine::machine::Engine;
use crate::smp::SmpServer;

pub(super) struct Check {
    run: u64,
    waiting: HashSet<String>,
    passed: HashSet<String>,
    /// Wall ms after which the silent are counted as failed.
    deadline: i64,
}

fn ms(d: std::time::Duration) -> i64 {
    i64::try_from(d.as_millis()).unwrap_or(i64::MAX)
}

impl<A: AgentPort> Engine<A> {
    /// The own relays of a Hello, and the tester that will ask them.
    pub(crate) fn configure_relays(&mut self, hello: &Hello) {
        let mut known: Vec<(String, SmpServer)> = Vec::new();
        for r in hello.relays.iter().filter(|r| r.starts_with("smp://")) {
            match r.parse::<SmpServer>() {
                Ok(s) if !known.iter().any(|(_, k)| *k == s) => known.push((r.clone(), s)),
                Ok(_) | Err(_) => {}
            }
        }
        let rel = &mut self.relays;
        rel.own_hosts = known
            .iter()
            .filter_map(|(_, s)| s.hosts.first().cloned())
            .collect();
        rel.known = known.into_iter().map(|(r, _)| r).collect();
        rel.disabled.clear();
        rel.check = None;
        rel.last_check = None;
        rel.after.clear();
        rel.tester.get_or_insert_with(|| Box::new(SmpTester));
    }

    /// Starts a check unless one runs, one started less than `CHECK_EVERY` ago, or fewer than two
    /// relays are named (a lone relay is never disabled). Whether one started.
    pub(crate) fn start_check(&mut self) -> bool {
        let now = self.clock.wall_ms();
        let tx = self.jobs.tx.clone();
        let rel = &mut self.relays;
        let recent = rel.last_check.is_some_and(|l| now - l < ms(CHECK_EVERY));
        if rel.check.is_some() || recent || rel.known.len() < 2 {
            return false;
        }
        let Some(tester) = rel.tester.as_mut() else {
            return false;
        };
        rel.run += 1;
        rel.last_check = Some(now);
        for server in &rel.known {
            tester.test(server.clone(), rel.run, tx.clone());
        }
        rel.check = Some(Check {
            run: rel.run,
            waiting: rel.known.iter().cloned().collect(),
            passed: HashSet::new(),
            deadline: now + ms(CHECK_DEADLINE),
        });
        true
    }

    pub(crate) fn on_relay_done(&mut self, done: RelayDone) {
        let Some(c) = self.relays.check.as_mut().filter(|c| c.run == done.run) else {
            return;
        };
        if !c.waiting.remove(&done.server) {
            return;
        }
        match done.failure {
            None => {
                c.passed.insert(done.server);
            }
            Some(why) => self.log.line(format_args!(
                "relay {} did not answer its test: {why}",
                host_of(&done.server)
            )),
        }
        if self
            .relays
            .check
            .as_ref()
            .is_some_and(|c| c.waiting.is_empty())
        {
            self.finish_check();
        }
    }

    /// Every answer is in, or the deadline passed: disable what stayed silent and enable what
    /// answered, unless nothing answered (then the network is down, not the relays).
    fn finish_check(&mut self) {
        let Some(c) = self.relays.check.take() else {
            return;
        };
        if c.passed.is_empty() {
            self.log
                .line(format_args!("no relay answered; none is disabled"));
        } else {
            let silent = self
                .relays
                .known
                .iter()
                .filter(|r| !c.passed.contains(*r))
                .cloned()
                .collect();
            self.set_disabled(silent);
        }
        self.flush_retries();
    }

    fn set_disabled(&mut self, silent: std::collections::BTreeSet<String>) {
        if silent == self.relays.disabled {
            return;
        }
        for r in silent.symmetric_difference(&self.relays.disabled) {
            let on = !silent.contains(r);
            self.log.line(format_args!(
                "relay {} is {} for new queues",
                host_of(r),
                if on { "enabled" } else { "disabled" }
            ));
        }
        let newly = silent.difference(&self.relays.disabled).next().is_some();
        self.relays.disabled = silent;
        let list: Vec<String> = self.relays.disabled.iter().cloned().collect();
        self.agent.set_disabled_relays(&list);
        if newly {
            self.restart_pending_queues();
        }
    }

    /// The deadline of a running check, or when the next check is due while a relay is disabled.
    #[must_use]
    pub(crate) fn next_relay_wake(&self) -> Option<i64> {
        let rel = &self.relays;
        match (&rel.check, rel.disabled.is_empty()) {
            (Some(c), _) => Some(c.deadline),
            (None, false) => rel.last_check.map(|l| l + ms(CHECK_EVERY)),
            (None, true) => None,
        }
    }

    pub(crate) fn relay_tick(&mut self, now: i64) {
        if self.next_relay_wake().is_none_or(|at| at > now) {
            return;
        }
        if self.relays.check.is_some() {
            self.finish_check();
        } else {
            self.start_check();
        }
    }
}

/// A relay's host, for the log (no key fingerprint or password).
fn host_of(server: &str) -> &str {
    server.rsplit('@').next().unwrap_or(server)
}

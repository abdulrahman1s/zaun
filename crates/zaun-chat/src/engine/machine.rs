//! The engine: state, store and agent owned by one thread, and the events waiting to go out (ADR-0012).

use std::collections::VecDeque;
use std::time::Duration;

use zaun_chat_wire::Event;

use super::agent_port::AgentPort;
use super::clock::Clock;
use super::files::Files;
use super::jobs::Jobs;
use super::log::Log;
use super::state::State;
use crate::store::Store;

/// The run loop stops taking requests and SMP events while this many events wait for the
/// writer, so a core that stops reading holds the outbox to about this (a tick may add a few).
pub const OUT_CAP: usize = 1024;

/// The longest the loop sleeps: housekeeping (closing idle relays) runs at least this often.
pub const IDLE: Duration = Duration::from_secs(5);

/// A wake already due still waits this long, so a timer that stays due cannot spin the loop.
pub const MIN_SLEEP: Duration = Duration::from_millis(10);

pub struct Engine<A: AgentPort> {
    pub(crate) store: Store,
    pub(crate) agent: A,
    pub(crate) clock: Box<dyn Clock>,
    pub(crate) state: State,
    pub(crate) log: Log,
    pub(crate) files: Files,
    pub(crate) jobs: Jobs,
    out: VecDeque<Event>,
}

impl<A: AgentPort> Engine<A> {
    pub fn new(store: Store, agent: A, clock: Box<dyn Clock>, log: Log) -> Self {
        Self {
            store,
            agent,
            clock,
            state: State::default(),
            log,
            files: Files::default(),
            jobs: Jobs::default(),
            out: VecDeque::new(),
        }
    }

    pub(crate) fn emit(&mut self, event: Event) {
        self.out.push_back(event);
    }

    /// Something waits for the writer.
    #[must_use]
    pub fn has_out(&self) -> bool {
        !self.out.is_empty() || self.state.chats_dirty
    }

    #[must_use]
    pub fn out_len(&self) -> usize {
        self.out.len()
    }

    /// The next event for the writer. The chat list is built only when it is next, so changes
    /// made while it waits go out in one list.
    pub fn pop_out(&mut self) -> Option<Event> {
        if let Some(e) = self.out.pop_front() {
            return Some(e);
        }
        if !self.state.chats_dirty {
            return None;
        }
        self.state.chats_dirty = false;
        Some(self.chats())
    }

    /// Everything the agent queued, handled until it queues no more.
    pub fn pump(&mut self) {
        loop {
            let events = self.agent.drain_events();
            if events.is_empty() {
                return;
            }
            for e in events {
                self.on_agent(e);
            }
        }
    }

    /// Where job results arrive; the run loop waits on it with the rest.
    #[must_use]
    pub fn job_results(&self) -> crossbeam_channel::Receiver<super::jobs::JobDone> {
        self.jobs.rx.clone()
    }

    pub fn on_smp(&mut self, event: A::Smp) {
        self.agent.on_smp(&mut self.store, event);
        self.pump();
    }

    /// How long the loop may sleep before the agent has timers to run, at most `IDLE`.
    #[must_use]
    pub fn sleep_for(&self) -> Duration {
        let wake = [self.agent.next_wake(), self.next_call_wake()];
        let Some(at) = wake.into_iter().flatten().min() else {
            return IDLE;
        };
        let due = u64::try_from(at - self.clock.wall_ms()).unwrap_or(0);
        Duration::from_millis(due).clamp(MIN_SLEEP, IDLE)
    }

    pub fn tick(&mut self) {
        let now = self.clock.wall_ms();
        self.agent.on_tick(&mut self.store, now);
        self.expire_calls(now);
        self.pump();
    }
}

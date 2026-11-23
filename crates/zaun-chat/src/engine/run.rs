//! The engine thread's loop: requests, SMP events, the writer and the agent's timers, one at a time (ADR-0012).

use crossbeam_channel::{Receiver, Select, Sender, never};
use zaun_chat_wire::{Event, Request};

use super::agent_port::AgentPort;
use super::machine::{Engine, OUT_CAP};

/// Runs until the requests end (stdin closed) or the writer is gone, then hands what is left
/// to the writer. The agent's network is closed; dropping the engine closes the store.
pub fn run<A: AgentPort>(
    mut engine: Engine<A>,
    requests: &Receiver<Request>,
    smp: Receiver<A::Smp>,
    events: &Sender<Event>,
) {
    serve(&mut engine, requests, smp, events);
    engine.agent.close();
}

fn serve<A: AgentPort>(
    engine: &mut Engine<A>,
    requests: &Receiver<Request>,
    smp: Receiver<A::Smp>,
    events: &Sender<Event>,
) {
    let mut smp = smp;
    let jobs = engine.job_results();
    loop {
        let mut sel = Select::new();
        // A writer that falls behind stops the requests and the network until it catches up.
        let room = engine.out_len() < OUT_CAP;
        let req = room.then(|| sel.recv(requests));
        let net = room.then(|| sel.recv(&smp));
        let job = room.then(|| sel.recv(&jobs));
        let out = engine.has_out().then(|| sel.send(events));
        let Ok(op) = sel.select_timeout(engine.sleep_for()) else {
            engine.tick();
            continue;
        };
        let i = op.index();
        if Some(i) == req {
            match op.recv(requests) {
                Ok(r) => engine.on_request(r),
                Err(_) => break,
            }
        } else if Some(i) == net {
            match op.recv(&smp) {
                Ok(e) => engine.on_smp(e),
                // Nobody sends any more: wait on the others alone.
                Err(_) => smp = never(),
            }
        } else if Some(i) == job {
            // The engine holds a sender, so the channel never closes while it runs.
            if let Ok(d) = op.recv(&jobs) {
                engine.on_job(d);
            }
        } else if Some(i) == out {
            // `has_out` above means there is one; the selected send must be completed.
            let Some(e) = engine.pop_out() else { continue };
            if op.send(events, e).is_err() {
                return;
            }
        }
    }
    while let Some(e) = engine.pop_out() {
        if events.send(e).is_err() {
            return;
        }
    }
}

//! The helper's main: Hello, the store, the engine until stdin ends; the exit code core reads
//! (0 = stdin closed, 3 = the store did not open or the key is wrong, 4 = fatal; `status.rs`).

use std::io::{Read, Write};

use crossbeam_channel::Receiver;
use simplex_client::engine::log::Log;
use simplex_client::engine::{AgentPort, Clock, Engine, NativeAgent, SystemClock};
use simplex_client::store::Store;
use simplex_client::types::Config;
use zaun_chat_wire::VERSION;

use super::{lines, reader, writer};

pub const EXIT_OK: i32 = 0;
pub const EXIT_DATABASE: i32 = 3;
pub const EXIT_FATAL: i32 = 4;

/// The store's file in the data folder Hello names.
pub const STORE_FILE: &str = "chat.db";

/// The helper with the native agent, on the relays Hello names.
pub fn run(
    stdin: impl Read + Send + 'static,
    stdout: impl Write + Send + 'static,
    stderr: impl Write + Send + 'static,
) -> i32 {
    run_with(
        stdin,
        stdout,
        stderr,
        Box::new(SystemClock),
        |config, log| NativeAgent::new(&config.relays, log.clone()),
    )
}

/// `agent` makes the agent for the Hello's settings, with the channel its SMP threads send on.
pub fn run_with<A: AgentPort>(
    stdin: impl Read + Send + 'static,
    stdout: impl Write + Send + 'static,
    stderr: impl Write + Send + 'static,
    clock: Box<dyn Clock>,
    agent: impl FnOnce(&Config, &Log) -> (A, Receiver<A::Smp>),
) -> i32 {
    let log = Log::new(stderr);
    let Ok((first, requests)) = reader::spawn(stdin, log.clone()) else {
        log.line(format_args!("the reader thread did not start"));
        return EXIT_FATAL;
    };
    let config = match first.recv() {
        Ok(reader::First::Hello(h)) if h.v == VERSION => lines::config(&h),
        Ok(reader::First::Closed) => return EXIT_OK,
        Ok(reader::First::Hello(_) | reader::First::Malformed) | Err(_) => {
            log.line(format_args!(
                "the first line is not a hello of version {VERSION}"
            ));
            return EXIT_FATAL;
        }
    };
    let store = match Store::open(&config.dir.join(STORE_FILE), &config.key) {
        Ok(s) => s,
        Err(e) => {
            log.line(format_args!("{e}"));
            return EXIT_DATABASE;
        }
    };
    let Ok((events, written)) = writer::spawn(stdout, log.clone()) else {
        log.line(format_args!("the writer thread did not start"));
        return EXIT_FATAL;
    };
    let (agent, smp) = agent(&config, &log);
    let mut engine = Engine::new(store, agent, clock, log.clone());
    if let Err(e) = engine.start(&config) {
        log.line(format_args!("start: {e}"));
        return EXIT_DATABASE;
    }
    simplex_client::engine::run::run(engine, &requests, smp, &events);
    drop(events);
    if written.join().is_err() {
        log.line(format_args!("the writer thread panicked"));
    }
    EXIT_OK
}

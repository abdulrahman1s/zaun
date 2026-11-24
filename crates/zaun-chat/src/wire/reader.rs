//! The stdin reader thread: the Hello first, then each request into a bounded channel.

use std::io::{BufRead, BufReader, Read};

use crossbeam_channel::{Receiver, Sender, bounded};
use simplex_client::engine::log::Log;
use simplex_client::types::Request;
use zaun_chat_wire::{Hello, decode_line};

use super::lines::ReqLine;

/// Requests read ahead of the engine; a full channel stops reading, which holds up core's writer.
pub const REQUESTS: usize = 256;

/// What the first line said: the Hello, a line that is not one, or nothing before the end.
#[derive(Debug)]
pub enum First {
    Hello(Box<Hello>),
    Malformed,
    Closed,
}

/// The thread is never joined: it may sit in a read until the process exits.
pub fn spawn(
    stdin: impl Read + Send + 'static,
    log: Log,
) -> std::io::Result<(Receiver<First>, Receiver<Request>)> {
    let (first_tx, first_rx) = bounded(1);
    let (req_tx, req_rx) = bounded(REQUESTS);
    std::thread::Builder::new()
        .name("chat-stdin".into())
        .spawn(move || read(stdin, &first_tx, &req_tx, &log))?;
    Ok((first_rx, req_rx))
}

fn read(stdin: impl Read, first: &Sender<First>, requests: &Sender<Request>, log: &Log) {
    let mut lines = BufReader::new(stdin).split(b'\n');
    let hello = match lines.next() {
        Some(Ok(line)) => match decode_line::<Hello>(&String::from_utf8_lossy(&line)) {
            Ok(h) => First::Hello(Box::new(h)),
            Err(_) => First::Malformed,
        },
        Some(Err(_)) | None => First::Closed,
    };
    let go_on = matches!(hello, First::Hello(_));
    if first.send(hello).is_err() || !go_on {
        return;
    }
    for line in lines {
        let Ok(line) = line else { return };
        match decode_line::<ReqLine>(&String::from_utf8_lossy(&line)) {
            Ok(r) => {
                if requests.send(r.req).is_err() {
                    return;
                }
            }
            // Never the line: it may hold a message.
            Err(e) => log.line(format_args!(
                "a request line was skipped ({} bytes): {e}",
                line.len()
            )),
        }
    }
}

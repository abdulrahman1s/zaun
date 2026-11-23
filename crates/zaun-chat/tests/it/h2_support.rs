//! An in-memory duplex pipe and a scripted HTTP/2 peer for the connection tests.
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use zaun_chat::h2::frames::{CLIENT_PREFACE, FrameHeader, HEADER_LEN, encode_frame, parse_header};

#[derive(Default)]
struct State {
    buf: VecDeque<u8>,
    closed: bool,
}

#[derive(Clone, Default)]
struct Pipe(Arc<(Mutex<State>, Condvar)>);

impl Pipe {
    fn push(&self, data: &[u8]) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap().buf.extend(data);
        cv.notify_all();
    }

    fn close(&self) {
        let (lock, cv) = &*self.0;
        lock.lock().unwrap().closed = true;
        cv.notify_all();
    }

    fn read(&self, out: &mut [u8], timeout: Option<Duration>) -> io::Result<usize> {
        let (lock, cv) = &*self.0;
        let mut st = lock.lock().unwrap();
        while st.buf.is_empty() {
            if st.closed {
                return Ok(0);
            }
            match timeout {
                None => st = cv.wait(st).unwrap(),
                Some(t) => {
                    let (s, r) = cv.wait_timeout(st, t).unwrap();
                    st = s;
                    if r.timed_out() && st.buf.is_empty() {
                        return Err(io::ErrorKind::TimedOut.into());
                    }
                }
            }
        }
        let n = out.len().min(st.buf.len());
        for slot in &mut out[..n] {
            *slot = st.buf.pop_front().unwrap_or_default();
        }
        Ok(n)
    }
}

/// One end of the pipe: reads what the other end wrote.
pub struct End {
    rx: Pipe,
    tx: Pipe,
    pub read_timeout: Option<Duration>,
}

pub fn duplex() -> (End, End) {
    let (a, b) = (Pipe::default(), Pipe::default());
    (
        End {
            rx: a.clone(),
            tx: b.clone(),
            read_timeout: None,
        },
        End {
            rx: b,
            tx: a,
            read_timeout: None,
        },
    )
}

impl Read for End {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.rx.read(buf, self.read_timeout)
    }
}

impl Write for End {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tx.push(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for End {
    fn drop(&mut self) {
        self.tx.close();
    }
}

/// The scripted peer: reads and sends whole frames.
impl End {
    pub fn read_exact_n(&mut self, n: usize) -> Vec<u8> {
        let mut out = vec![0u8; n];
        Read::read_exact(self, &mut out).unwrap();
        out
    }

    pub fn frame(&mut self) -> (FrameHeader, Vec<u8>) {
        let head: [u8; HEADER_LEN] = self.read_exact_n(HEADER_LEN).try_into().unwrap();
        let h = parse_header(&head);
        let payload = self.read_exact_n(h.length);
        (h, payload)
    }

    pub fn expect_preface(&mut self) {
        assert_eq!(self.read_exact_n(CLIENT_PREFACE.len()), CLIENT_PREFACE);
    }

    pub fn send(&mut self, bytes: &[u8]) {
        self.write_all(bytes).unwrap();
    }

    pub fn send_frame(&mut self, kind: u8, flags: u8, stream: u32, payload: &[u8]) {
        self.send(&encode_frame(kind, flags, stream, payload));
    }
}

/// Runs `server` on its own thread against `client`'s pipe end and returns the client's result.
pub fn run<R>(
    server: impl FnOnce(&mut End) + Send,
    client: impl FnOnce(End) -> R,
    client_timeout: Option<Duration>,
) -> R {
    let (mut c, mut s) = duplex();
    c.read_timeout = client_timeout;
    std::thread::scope(|scope| {
        let peer = scope.spawn(move || server(&mut s));
        let r = client(c);
        peer.join().unwrap();
        r
    })
}

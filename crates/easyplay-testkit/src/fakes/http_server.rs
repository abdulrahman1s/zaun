//! A tiny HTTP/1.1 server on `127.0.0.1` for client tests (std `TcpListener`, one thread per
//! connection, `Connection: close`). Paired with `UreqTransport::plaintext_loopback`, which
//! sends `GET /<host><path>`: routes are matched on that full string, e.g.
//! `"/cdn.cloudflare.steamstatic.com/steam/apps/70/header.jpg"`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use parking_lot::Mutex;

/// What the server answers for a route.
#[derive(Clone, Debug)]
pub enum Reply {
    /// Status, extra headers and a body (sent with `Content-Length`).
    Full {
        /// Status code.
        status: u16,
        /// Extra headers.
        headers: Vec<(String, String)>,
        /// Body.
        body: Vec<u8>,
    },
    /// 200 with a chunked body of `len` bytes and no `Content-Length`.
    Chunked {
        /// Total body length.
        len: usize,
    },
    /// Accept the request and send nothing for `hold`.
    Stall {
        /// How long to hold the connection open.
        hold: Duration,
    },
}

impl Reply {
    /// `200` with `body`.
    pub fn ok(body: impl Into<Vec<u8>>) -> Self {
        Reply::Full {
            status: 200,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// A bare status.
    pub fn status(status: u16) -> Self {
        Reply::Full {
            status,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    /// `301` to `location`.
    pub fn redirect(location: &str) -> Self {
        Reply::Full {
            status: 301,
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
        }
    }
}

struct State {
    routes: HashMap<String, Reply>,
    log: Vec<(String, Option<String>)>,
}

/// The server. Stops accepting when dropped.
pub struct FakeHttpServer {
    addr: SocketAddr,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
}

impl FakeHttpServer {
    /// Start on an ephemeral loopback port. Unknown routes answer 404.
    pub fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let state = Arc::new(Mutex::new(State {
            routes: HashMap::new(),
            log: Vec::new(),
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        thread::spawn(move || {
            while !sp.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let st = st.clone();
                        thread::spawn(move || serve(stream, &st));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(5)),
                }
            }
        });
        Ok(Self { addr, state, stop })
    }

    /// The address to point the transport at.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Set the reply for `path` (`/<host><path>`).
    pub fn route(&self, path: &str, reply: Reply) -> &Self {
        self.state.lock().routes.insert(path.to_owned(), reply);
        self
    }

    /// Requests seen so far: path and `User-Agent`.
    pub fn requests(&self) -> Vec<(String, Option<String>)> {
        self.state.lock().log.clone()
    }
}

impl Drop for FakeHttpServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn serve(stream: TcpStream, state: &Mutex<State>) {
    let _ = stream.set_nonblocking(false);
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let mut agent = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':')
            && k.eq_ignore_ascii_case("user-agent")
        {
            agent = Some(v.trim().to_owned());
        }
    }
    let reply = {
        let mut s = state.lock();
        s.log.push((path.clone(), agent));
        s.routes.get(&path).cloned().unwrap_or(Reply::status(404))
    };
    let mut w = stream;
    match reply {
        Reply::Full {
            status,
            headers,
            body,
        } => {
            let mut head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                body.len()
            );
            for (k, v) in headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            let _ = w.write_all(head.as_bytes());
            let _ = w.write_all(&body);
        }
        Reply::Chunked { len } => {
            let _ = w.write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
            );
            let chunk = vec![b'x'; 64 * 1024];
            let mut left = len;
            while left > 0 {
                let n = left.min(chunk.len());
                if w.write_all(format!("{n:x}\r\n").as_bytes()).is_err()
                    || w.write_all(&chunk[..n]).is_err()
                    || w.write_all(b"\r\n").is_err()
                {
                    return;
                }
                left -= n;
            }
            let _ = w.write_all(b"0\r\n\r\n");
        }
        Reply::Stall { hold } => thread::sleep(hold),
    }
    let _ = w.flush();
}

//! Just enough of a WebSocket client (RFC 6455) to drive the CLI's chat server: text frames, masked, no extensions.
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, unbounded};

use crate::crypto::random_vec;
use crate::encoding::b64_encode;

pub struct Ws {
    out: Arc<Mutex<TcpStream>>,
    pub rx: Receiver<String>,
}

impl Ws {
    pub fn connect(port: u16) -> Self {
        let mut s = TcpStream::connect(("127.0.0.1", port)).expect("CLI chat server");
        let key = b64_encode(&random_vec(16).unwrap(), true);
        write!(
            s,
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
        )
        .unwrap();
        let mut rd = BufReader::new(s.try_clone().unwrap());
        let mut status = String::new();
        rd.read_line(&mut status).unwrap();
        assert!(status.contains(" 101 "), "upgrade refused: {status}");
        loop {
            let mut line = String::new();
            rd.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        let out = Arc::new(Mutex::new(s));
        let (tx, rx) = unbounded();
        let pong = Arc::clone(&out);
        thread::spawn(move || {
            while let Some((op, data)) = read_frame(&mut rd) {
                match op {
                    1 | 2 => {
                        if tx
                            .send(String::from_utf8_lossy(&data).into_owned())
                            .is_err()
                        {
                            break;
                        }
                    }
                    9 => frame(&mut pong.lock().unwrap(), 0xA, &data),
                    8 => break,
                    _ => {}
                }
            }
        });
        Self { out, rx }
    }

    pub fn send(&self, text: &str) {
        frame(&mut self.out.lock().unwrap(), 1, text.as_bytes());
    }

    pub fn recv(&self, wait: Duration) -> Option<String> {
        self.rx.recv_timeout(wait).ok()
    }
}

/// One whole message: continuation frames are joined.
fn read_frame(rd: &mut impl Read) -> Option<(u8, Vec<u8>)> {
    let mut data = Vec::new();
    let mut first_op = None;
    loop {
        let mut h = [0u8; 2];
        rd.read_exact(&mut h).ok()?;
        let len = match h[1] & 0x7F {
            126 => {
                let mut b = [0u8; 2];
                rd.read_exact(&mut b).ok()?;
                u64::from(u16::from_be_bytes(b))
            }
            127 => {
                let mut b = [0u8; 8];
                rd.read_exact(&mut b).ok()?;
                u64::from_be_bytes(b)
            }
            n => u64::from(n),
        };
        let mut part = vec![0u8; usize::try_from(len).ok()?];
        rd.read_exact(&mut part).ok()?;
        data.extend(part);
        first_op.get_or_insert(h[0] & 0x0F);
        if h[0] & 0x80 != 0 {
            return Some((first_op?, data));
        }
    }
}

fn frame(s: &mut TcpStream, op: u8, data: &[u8]) {
    let mut f = vec![0x80 | op];
    match data.len() {
        n if n < 126 => f.push(0x80 | u8::try_from(n).unwrap()),
        n if n < 65536 => {
            f.push(0x80 | 126);
            f.extend(u16::try_from(n).unwrap().to_be_bytes());
        }
        n => {
            f.push(0x80 | 127);
            f.extend(u64::try_from(n).unwrap().to_be_bytes());
        }
    }
    let mask = random_vec(4).unwrap();
    f.extend(&mask);
    f.extend(data.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    let _ = s.write_all(&f); // a closed CLI ends the test with its own timeout
}

//! A call between Zaun's media and the official apps' own WebRTC code (`call.js` from the
//! desktop app's `assets/www`, in a headless Chromium), both ways, with and without the
//! apps' frame encryption (ADR-0035 §3–4). The page is driven as the apps drive it: commands
//! in, responses and `ice` messages out, through a tiny HTTP server in this test.
//!
//! ```sh
//! ZAUN_APP_WWW=<dir with call.js, lz-string.min.js, desktop/ui.js> ZAUN_BROWSER=brave \
//!   cargo nextest run -p zaun-media app_interop --run-ignored only
//! ```

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};
use zaun_core::chat::media::{
    AudioSettings, CallKey, MediaConfig, MediaEvent, MediaFactory, MediaSink, Negotiation,
    TrackKind,
};
use zaun_media::{RtcMedia, ToneAudio};

use super::common::{Side, connected, description, wait};

const PAGE: &str = r#"<!DOCTYPE html><html><head><script src="/lz-string.min.js"></script></head><body>
<video id="remote-screen-video-stream"></video><video id="remote-video-stream" autoplay></video>
<video id="local-video-stream" muted></video><video id="local-screen-video-stream" muted></video>
<div id="progress"></div><div id="info-block"><p id="state"></p><p id="description"></p><b id="media-sources"></b></div>
<div id="audio-call-icon"></div><div id="permission-denied"><p id="permission-denied-title"></p><p id="permission-denied-desc"></p></div>
<p id="manage-call"><button id="toggle-screen"></button><button id="toggle-mic"></button><button id="end-call"></button><button id="toggle-speaker"></button><button id="toggle-camera"></button></p>
<script>
// The desktop app's ui.js talks over a WebSocket; this one is the test's HTTP server.
const post = (path, v) => fetch(path, {method: "POST", body: JSON.stringify(v)});
const log0 = console.log;
console.log = (...a) => { post("/out", {log: a.map((x) => (typeof x == "object" ? JSON.stringify(x) : String(x))).join(" ")}); log0(...a); };
window.onerror = (m, src, line) => post("/out", {log: "ERROR " + m + " " + src + ":" + line});
window.onunhandledrejection = (e) => post("/out", {log: "REJECTED " + (e.reason && (e.reason.stack || e.reason))});
class FakeSocket {
  constructor() { this.l = {}; setTimeout(() => (this.l.open || []).forEach((f) => f({})), 0); window.__socket = this; }
  addEventListener(t, f) { (this.l[t] = this.l[t] || []).push(f); }
  send(d) { post("/out", JSON.parse(d)); }
}
window.WebSocket = FakeSocket;
async function stats() {
  const pc = activeCall && activeCall.connection;
  if (!pc) return {none: true};
  let r = {received: 0, energy: 0, sent: 0, state: pc.connectionState, peerMic: activeCall.peerMediaSources.mic};
  (await pc.getStats()).forEach((s) => {
    if (s.type == "inbound-rtp" && s.kind == "audio") { r.received += s.bytesReceived || 0; r.energy += s.totalAudioEnergy || 0; }
    if (s.type == "outbound-rtp" && s.kind == "audio") r.sent += s.bytesSent || 0;
  });
  return r;
}
async function loop() {
  for (;;) {
    let r;
    try { r = await fetch("/next"); } catch (e) { return; }
    if (r.status != 200) continue;
    const cmd = await r.json();
    if (cmd.stats) { post("/out", {stats: await stats()}); continue; }
    (window.__socket.l.message || []).forEach((f) => f({data: JSON.stringify({corrId: 1, command: cmd})}));
  }
}
window.addEventListener("load", () => setTimeout(loop, 100));
</script>
<script src="/call.js"></script><script src="/desktop/ui.js"></script></body></html>"#;

/// The page's server: files from the apps' `www`, commands to the page, messages back.
struct Page {
    commands: Arc<(Mutex<VecDeque<String>>, Condvar)>,
    out: Receiver<Value>,
    browser: Child,
    _profile: PathBuf,
}

impl Page {
    fn open(www: PathBuf, browser: &str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let commands = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let (tx, out) = channel();
        let (c2, www2) = (Arc::clone(&commands), www.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (c, w, t) = (Arc::clone(&c2), www2.clone(), tx.clone());
                std::thread::spawn(move || serve(stream, &w, &c, &t));
            }
        });
        let profile = std::env::temp_dir().join(format!("zn-interop-{}", std::process::id()));
        let browser = Command::new(browser)
            .args([
                "--headless=new",
                "--no-first-run",
                "--use-fake-device-for-media-stream",
                "--use-fake-ui-for-media-stream",
                "--autoplay-policy=no-user-gesture-required",
                "--force-webrtc-ip-handling-policy=default",
            ])
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("http://127.0.0.1:{port}/"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("the browser starts (ZAUN_BROWSER)");
        Self {
            commands,
            out,
            browser,
            _profile: profile,
        }
    }

    fn send(&self, command: Value) {
        let (q, cv) = &*self.commands;
        q.lock().unwrap().push_back(command.to_string());
        cv.notify_all();
    }

    /// The next message from the page that `f` picks; `ice` messages go to `media` on the way.
    fn wait<T>(&self, side: &Side, what: &str, mut f: impl FnMut(&Value) -> Option<T>) -> T {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let left = end.saturating_duration_since(Instant::now());
            let m = self
                .out
                .recv_timeout(left)
                .unwrap_or_else(|_| panic!("timed out waiting for the page's {what}"));
            if let Some(l) = m.get("log").and_then(Value::as_str) {
                eprintln!("page: {}", l.chars().take(300).collect::<String>());
                continue;
            }
            if let Some(t) = f(&m) {
                return t;
            }
            match m["resp"]["type"].as_str() {
                Some("ice") => side
                    .media
                    .candidates(candidates(&m["resp"]["iceCandidates"])),
                Some("error") => panic!("the page refused a command: {m}"),
                _ => {}
            }
        }
    }

    fn stats(&self, side: &Side) -> Value {
        self.send(json!({ "stats": true }));
        self.wait(side, "stats", |m| m.get("stats").cloned())
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        let _ = self.browser.kill();
        let _ = self.browser.wait();
    }
}

fn serve(
    mut s: TcpStream,
    www: &std::path::Path,
    commands: &(Mutex<VecDeque<String>>, Condvar),
    out: &Sender<Value>,
) {
    let mut r = BufReader::new(s.try_clone().unwrap());
    let mut first = String::new();
    if r.read_line(&mut first).is_err() {
        return;
    }
    let mut length = 0;
    loop {
        let mut h = String::new();
        if r.read_line(&mut h).is_err() || h.trim().is_empty() {
            break;
        }
        if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
            length = v.trim().parse().unwrap_or(0);
        }
    }
    let path = first.split_whitespace().nth(1).unwrap_or("/").to_owned();
    let (status, ty, body): (&str, &str, Vec<u8>) = match path.as_str() {
        "/" => ("200 OK", "text/html", PAGE.as_bytes().to_vec()),
        "/out" => {
            let mut b = vec![0; length];
            let _ = r.read_exact(&mut b);
            if let Ok(v) = serde_json::from_slice(&b) {
                let _ = out.send(v);
            }
            ("200 OK", "text/plain", Vec::new())
        }
        "/next" => {
            let (q, cv) = commands;
            let g = q.lock().unwrap();
            let (mut g, _) = cv
                .wait_timeout_while(g, Duration::from_millis(500), |q| q.is_empty())
                .unwrap();
            match g.pop_front() {
                Some(c) => ("200 OK", "application/json", c.into_bytes()),
                None => ("204 No Content", "text/plain", Vec::new()),
            }
        }
        p => match std::fs::read(www.join(p.trim_start_matches('/'))) {
            Ok(b) => ("200 OK", "application/javascript", b),
            Err(_) => ("404 Not Found", "text/plain", Vec::new()),
        },
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ty}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = s.write_all(head.as_bytes());
    let _ = s.write_all(&body);
}

fn pack(v: &Value) -> String {
    lz_str::compress_to_base64(&v.to_string())
}

fn unpack(text: &Value) -> Value {
    let units = lz_str::decompress_from_base64(text.as_str().unwrap()).unwrap();
    serde_json::from_str(&String::from_utf16(&units).unwrap()).unwrap()
}

/// `candidate:…` strings from the page's compressed `RTCIceCandidate` list.
fn candidates(text: &Value) -> Vec<String> {
    unpack(text)
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["candidate"].as_str())
        .filter(|c| c.starts_with("candidate:"))
        .map(str::to_owned)
        .collect()
}

/// Our description as the codec sends it: `{type, sdp}`, candidates also in the list.
fn ours(sdp: &str, kind: &str) -> (String, String) {
    let mid = sdp
        .lines()
        .find_map(|l| l.strip_prefix("a=mid:"))
        .unwrap_or("0");
    let list: Vec<Value> = sdp
        .lines()
        .filter_map(|l| l.trim().strip_prefix("a="))
        .filter(|c| c.starts_with("candidate:"))
        .map(|c| json!({ "candidate": c, "sdpMid": mid, "sdpMLineIndex": 0 }))
        .collect();
    (
        pack(&json!({ "type": kind, "sdp": sdp })),
        pack(&Value::Array(list)),
    )
}

fn start(f: &RtcMedia, negotiation: Negotiation, key: Option<CallKey>) -> Side {
    let (tx, rx) = channel();
    let tx = Mutex::new(tx);
    let sink: MediaSink = Arc::new(move |e| {
        let _ = tx.lock().unwrap().send(e);
    });
    let config = MediaConfig {
        negotiation,
        tracks: vec![TrackKind::Audio],
        ice: Vec::new(),
        relay_only: false,
        audio: AudioSettings::default(),
        key,
    };
    Side {
        media: f.start(config, sink).unwrap(),
        events: rx,
    }
}

fn setup() -> Option<(PathBuf, String)> {
    let www = std::env::var_os("ZAUN_APP_WWW")?;
    let browser = std::env::var("ZAUN_BROWSER").unwrap_or_else(|_| "brave".into());
    Some((www.into(), browser))
}

fn key() -> ([u8; 32], String) {
    let bytes: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let text = base64::engine::general_purpose::URL_SAFE.encode(bytes);
    (bytes, text)
}

/// Sound got through both ways: the tone reached our speakers, and the page decoded our
/// frames into sound (energy) and plays them as the friend's microphone.
fn heard_both_ways(page: &Page, side: &Side, audio: &ToneAudio, what: &str) {
    connected(side);
    std::thread::sleep(Duration::from_secs(4));
    let samples = audio.heard.samples();
    let loud = samples.iter().filter(|s| s.unsigned_abs() > 300).count();
    let s = page.stats(side);
    eprintln!(
        "{what}: we heard {} samples ({loud} loud); page {s}",
        samples.len()
    );
    assert!(loud > 2_000, "{what}: Zaun heard nothing from the app");
    assert!(
        s["received"].as_f64().unwrap_or(0.0) > 5_000.0,
        "{what}: the app received no audio: {s}"
    );
    assert!(
        s["energy"].as_f64().unwrap_or(0.0) > 0.0,
        "{what}: the app decoded no sound: {s}"
    );
    assert_eq!(s["peerMic"], true, "{what}: the app plays our sound: {s}");
    wait(side, "levels", |e| {
        matches!(e, MediaEvent::Levels { .. }).then_some(())
    });
}

/// Zaun calls; the app (callee) writes the offer with its four lines; Zaun answers.
fn zaun_calls_the_app(encrypted: bool) {
    let Some((www, browser)) = setup() else {
        return;
    };
    let page = Page::open(www, &browser);
    let (bytes, text) = key();
    let mut start_cmd =
        json!({ "type": "start", "media": "audio", "iceServers": [], "relay": false });
    if encrypted {
        start_cmd["aesKey"] = text.into();
    }
    std::thread::sleep(Duration::from_secs(2));
    let audio = ToneAudio::new(440.0);
    let f = RtcMedia::new(Arc::new(audio.clone()));
    // A placeholder side for `ice` messages before ours exists: none come before the offer.
    let (_t, r) = channel();
    let none = Side {
        media: Box::new(NoMedia),
        events: r,
    };
    page.send(start_cmd);
    let offer = page.wait(&none, "offer", |m| {
        (m["resp"]["type"] == "offer").then(|| m["resp"].clone())
    });
    assert_eq!(offer["capabilities"]["encryption"], true);
    let sdp = unpack(&offer["offer"])["sdp"].as_str().unwrap().to_owned();
    assert_eq!(
        sdp.matches("\nm=").count(),
        4,
        "mic, camera, screen sound, screen"
    );
    let side = start(
        &f,
        Negotiation::Answer(sdp),
        encrypted.then_some(CallKey(bytes)),
    );
    side.media.candidates(candidates(&offer["iceCandidates"]));
    let answer = description(&side);
    let (a, list) = ours(&answer, "answer");
    page.send(json!({ "type": "answer", "answer": a, "iceCandidates": list }));
    page.wait(&side, "ok", |m| (m["resp"]["type"] == "ok").then_some(()));
    heard_both_ways(&page, &side, &audio, "Zaun calls");
}

/// The app calls; Zaun (callee) writes the offer, with mid "0"; the app answers.
fn the_app_calls_zaun(encrypted: bool) {
    let Some((www, browser)) = setup() else {
        return;
    };
    let page = Page::open(www, &browser);
    let (bytes, text) = key();
    std::thread::sleep(Duration::from_secs(2));
    let audio = ToneAudio::new(440.0);
    let f = RtcMedia::new(Arc::new(audio.clone()));
    let side = start(&f, Negotiation::Offer, encrypted.then_some(CallKey(bytes)));
    page.send(json!({ "type": "capabilities", "media": "audio" }));
    page.wait(&side, "capabilities", |m| {
        (m["resp"]["type"] == "capabilities").then_some(())
    });
    let offer = description(&side);
    assert!(offer.contains("a=mid:0\r\n"), "{offer}");
    let (o, list) = ours(&offer, "offer");
    let mut cmd = json!({ "type": "offer", "offer": o, "iceCandidates": list, "media": "audio", "iceServers": [], "relay": false });
    if encrypted {
        cmd["aesKey"] = text.into();
    }
    page.send(cmd);
    let answer = page.wait(&side, "answer", |m| {
        (m["resp"]["type"] == "answer").then(|| m["resp"].clone())
    });
    side.media.answer(
        unpack(&answer["answer"])["sdp"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    side.media.candidates(candidates(&answer["iceCandidates"]));
    heard_both_ways(&page, &side, &audio, "the app calls");
}

struct NoMedia;
impl zaun_core::chat::media::CallMedia for NoMedia {
    fn answer(&self, _: String) {}
    fn candidates(&self, _: Vec<String>) {}
    fn set_muted(&self, _: bool) {}
    fn set_deafened(&self, _: bool) {}
}

#[test]
#[ignore = "needs the apps' call.js (ZAUN_APP_WWW) and a Chromium browser"]
fn with_different_keys_neither_side_hears_the_other() {
    let Some((www, browser)) = setup() else {
        return;
    };
    let page = Page::open(www, &browser);
    let (_, text) = key();
    std::thread::sleep(Duration::from_secs(2));
    let audio = ToneAudio::new(440.0);
    let f = RtcMedia::new(Arc::new(audio.clone()));
    let side = start(&f, Negotiation::Offer, Some(CallKey([1; 32])));
    page.send(json!({ "type": "capabilities", "media": "audio" }));
    page.wait(&side, "capabilities", |m| {
        (m["resp"]["type"] == "capabilities").then_some(())
    });
    let (o, list) = ours(&description(&side), "offer");
    page.send(json!({ "type": "offer", "offer": o, "iceCandidates": list, "media": "audio", "aesKey": text, "iceServers": [], "relay": false }));
    let answer = page.wait(&side, "answer", |m| {
        (m["resp"]["type"] == "answer").then(|| m["resp"].clone())
    });
    side.media.answer(
        unpack(&answer["answer"])["sdp"]
            .as_str()
            .unwrap()
            .to_owned(),
    );
    connected(&side);
    std::thread::sleep(Duration::from_secs(4));
    let loud = audio
        .heard
        .samples()
        .iter()
        .filter(|s| s.unsigned_abs() > 300)
        .count();
    let s = page.stats(&side);
    eprintln!("different keys: we heard {loud} loud; page {s}");
    assert_eq!(loud, 0, "Zaun must not play frames it cannot open");
    assert!(
        s["energy"].as_f64().unwrap_or(1.0) < 0.001,
        "the app decoded our frames: {s}"
    );
}

#[test]
#[ignore = "needs the apps' call.js (ZAUN_APP_WWW) and a Chromium browser"]
fn zaun_calls_the_official_app_and_both_hear() {
    zaun_calls_the_app(false);
}

#[test]
#[ignore = "needs the apps' call.js (ZAUN_APP_WWW) and a Chromium browser"]
fn zaun_calls_the_official_app_with_its_frame_encryption_and_both_hear() {
    zaun_calls_the_app(true);
}

#[test]
#[ignore = "needs the apps' call.js (ZAUN_APP_WWW) and a Chromium browser"]
fn the_official_app_calls_zaun_and_both_hear() {
    the_app_calls_zaun(false);
}

#[test]
#[ignore = "needs the apps' call.js (ZAUN_APP_WWW) and a Chromium browser"]
fn the_official_app_calls_zaun_with_its_frame_encryption_and_both_hear() {
    the_app_calls_zaun(true);
}

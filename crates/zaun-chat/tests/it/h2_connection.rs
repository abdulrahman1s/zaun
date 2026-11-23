//! The HTTP/2 client against a scripted peer, ported from nook src/sx/h2/connection.test.ts.
use std::time::Duration;

use zaun_chat::h2::frames::*;
use zaun_chat::h2::{
    Connection, H2Error, HpackDecoder, Options, Progress, Request, Stop, encode_headers,
};

use super::h2_support::{End, run};

fn go(_: Progress) -> Result<(), Stop> {
    Ok(())
}

fn get(path: &str) -> Request<'_> {
    Request {
        method: "GET",
        path,
        authority: "a",
        headers: &[],
        body: &[],
    }
}

fn response(stream: u32, status: &str, end: bool, extra: &[(&str, &str)]) -> Vec<u8> {
    let mut fields = vec![(":status", status)];
    fields.extend_from_slice(extra);
    encode_frame(
        HEADERS,
        END_HEADERS | if end { END_STREAM } else { 0 },
        stream,
        &encode_headers(&fields),
    )
}

/// The server side of the connection's opening: preface, SETTINGS, the connection WINDOW_UPDATE.
fn accept(s: &mut End) {
    s.expect_preface();
    let (h, p) = s.frame();
    assert_eq!(h.kind, SETTINGS);
    let settings = decode_settings(&p).unwrap();
    assert!(settings.contains(&(SETTINGS_ENABLE_PUSH, 0)), "push is off");
    assert_eq!(s.frame().0.kind, WINDOW_UPDATE);
}

#[test]
fn the_preface_settings_and_request_headers_are_what_a_server_expects() {
    run(
        |s| {
            accept(s);
            let (h, p) = s.frame();
            assert_eq!(
                (
                    h.kind,
                    h.stream,
                    h.flags & END_STREAM,
                    h.flags & END_HEADERS
                ),
                (HEADERS, 1, 0, END_HEADERS)
            );
            let fields = HpackDecoder::new(4096, 1 << 20).decode(&p).unwrap();
            let want = [
                (":method", "POST"),
                (":scheme", "https"),
                (":path", "/"),
                (":authority", "xftp.test"),
                ("x-test", "1"),
                ("content-length", "3"),
            ];
            assert_eq!(
                fields,
                want.iter()
                    .map(|(n, v)| (n.to_string(), v.to_string()))
                    .collect::<Vec<_>>()
            );
            let (h, p) = s.frame();
            assert_eq!(
                (h.kind, h.flags & END_STREAM, p),
                (DATA, END_STREAM, vec![1, 2, 3])
            );
            s.send(&encode_settings(&[(SETTINGS_MAX_FRAME_SIZE, 16384)]));
            s.send(&response(
                1,
                "200",
                false,
                &[("content-type", "application/octet-stream")],
            ));
            s.send_frame(DATA, END_STREAM, 1, &[9, 8, 7]);
            let (h, _) = s.frame();
            assert_eq!(
                (h.kind, h.flags),
                (SETTINGS, ACK),
                "the server's SETTINGS are acknowledged"
            );
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            let req = Request {
                method: "POST",
                path: "/",
                authority: "xftp.test",
                headers: &[("x-test", "1")],
                body: &[1, 2, 3],
            };
            let r = c.request(&req, 100, &mut go).unwrap();
            assert_eq!((r.status, r.body), (200, vec![9, 8, 7]));
            assert!(
                r.headers
                    .contains(&("content-type".into(), "application/octet-stream".into()))
            );
        },
        None,
    );
}

#[test]
fn ping_padded_data_continuation_informational_and_trailers_are_handled() {
    run(
        |s| {
            accept(s);
            assert_eq!(s.frame().0.flags & END_STREAM, END_STREAM);
            s.send(&encode_ping([1, 2, 3, 4, 5, 6, 7, 8], false));
            let (h, p) = s.frame();
            assert_eq!(
                (h.kind, h.flags, p),
                (PING, ACK, vec![1, 2, 3, 4, 5, 6, 7, 8])
            );
            s.send(&response(1, "100", false, &[]));
            let block = encode_headers(&[(":status", "200"), ("x-long", &"y".repeat(100))]);
            s.send_frame(HEADERS, 0, 1, &block[..10]);
            s.send_frame(CONTINUATION, END_HEADERS, 1, &block[10..]);
            s.send_frame(DATA, PADDED, 1, &[2, 0xaa, 0xbb, 0xcc, 0, 0]);
            s.send_frame(
                HEADERS,
                END_HEADERS | END_STREAM,
                1,
                &encode_headers(&[("x-trailer", "t")]),
            );
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            let r = c.request(&get("/"), 100, &mut go).unwrap();
            assert_eq!(r.status, 200);
            assert!(r.headers.contains(&("x-long".into(), "y".repeat(100))));
            assert_eq!(r.body, [0xaa, 0xbb, 0xcc]);
            assert_eq!(r.trailers, [("x-trailer".to_owned(), "t".to_owned())]);
        },
        None,
    );
}

#[test]
fn an_upload_obeys_the_peer_window_and_resumes_on_window_update() {
    let body: Vec<u8> = (0..5000u32).map(|i| (i * 31) as u8).collect();
    let expect = body.clone();
    run(
        move |s| {
            accept(s);
            s.frame(); // HEADERS of the priming request
            s.send(&encode_settings(&[(SETTINGS_INITIAL_WINDOW_SIZE, 1000)]));
            s.send(&response(1, "200", true, &[]));
            assert_eq!(s.frame().0.kind, SETTINGS);
            s.frame(); // HEADERS of the upload
            let mut got = Vec::new();
            for round in 0..5 {
                let (h, p) = s.frame();
                assert_eq!(h.kind, DATA);
                assert_eq!(p.len(), 1000, "the window allows 1000 bytes at a time");
                got.extend(p);
                assert_eq!(
                    h.flags & END_STREAM,
                    if round == 4 { END_STREAM } else { 0 }
                );
                if round < 4 {
                    s.send(&encode_window_update(3, 1000));
                }
            }
            assert_eq!(got, expect);
            s.send(&response(3, "200", true, &[]));
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            c.request(&get("/prime"), 0, &mut go).unwrap();
            let req = Request {
                method: "POST",
                path: "/",
                authority: "a",
                headers: &[],
                body: &body,
            };
            assert_eq!(c.request(&req, 0, &mut go).unwrap().status, 200);
        },
        None,
    );
}

#[test]
fn a_big_download_is_read_in_frames_and_the_client_sends_window_updates() {
    const TOTAL: usize = 1_000_000;
    let payload: Vec<u8> = (0..TOTAL).map(|i| (i * 31 + (i >> 8)) as u8).collect();
    let expect = payload.clone();
    run(
        move |s| {
            accept(s);
            s.frame();
            s.send(&response(1, "200", false, &[]));
            let (mut sent, mut stream_room, mut conn_room, mut updates) =
                (0usize, 100_000usize, 200_000usize, 0);
            while sent < TOTAL {
                let n = 16384.min(TOTAL - sent).min(stream_room).min(conn_room);
                if n == 0 {
                    let (h, p) = s.frame();
                    assert_eq!(h.kind, WINDOW_UPDATE);
                    let inc = u32::from_be_bytes(p.try_into().unwrap()) as usize;
                    if h.stream == 0 {
                        conn_room += inc
                    } else {
                        stream_room += inc
                    }
                    updates += 1;
                    continue;
                }
                let end = if sent + n == TOTAL { END_STREAM } else { 0 };
                s.send_frame(DATA, end, 1, &payload[sent..sent + n]);
                (sent, stream_room, conn_room) = (sent + n, stream_room - n, conn_room - n);
            }
            assert!(updates > 0, "the client must replenish both windows");
        },
        |io| {
            let opts = Options {
                stream_window: 100_000,
                connection_window: 200_000,
                ..Options::default()
            };
            let mut c = Connection::open(io, opts).unwrap();
            let mut last = Progress::default();
            let mut watch = |p: Progress| -> Result<(), Stop> {
                last = p;
                Ok(())
            };
            let r = c.request(&get("/"), TOTAL, &mut watch).unwrap();
            assert_eq!(r.body, expect);
            assert_eq!(last.received, TOTAL);
        },
        None,
    );
}

#[test]
fn rst_stream_goaway_and_a_dead_transport_fail_the_request_and_the_connection() {
    run(
        |s| {
            accept(s);
            s.frame();
            s.send_frame(RST_STREAM, 0, 1, &8u32.to_be_bytes());
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            assert!(matches!(
                c.request(&get("/"), 0, &mut go),
                Err(H2Error::Reset(8))
            ));
            assert!(!c.is_usable());
            assert!(matches!(
                c.request(&get("/"), 0, &mut go),
                Err(H2Error::Closed)
            ));
        },
        None,
    );
    run(
        |s| {
            accept(s);
            s.frame();
            s.send(&encode_goaway(0, 0));
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            assert!(matches!(
                c.request(&get("/"), 0, &mut go),
                Err(H2Error::GoAway(0))
            ));
        },
        None,
    );
    run(
        |s| {
            accept(s);
            s.frame();
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            assert!(matches!(
                c.request(&get("/"), 0, &mut go),
                Err(H2Error::Closed)
            ));
        },
        None,
    );
}

#[test]
fn a_response_larger_than_the_cap_is_refused() {
    run(
        |s| {
            accept(s);
            s.frame();
            s.send(&response(1, "200", false, &[]));
            s.send_frame(DATA, END_STREAM, 1, &[0u8; 100]);
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            assert!(matches!(
                c.request(&get("/"), 10, &mut go),
                Err(H2Error::TooLarge(10))
            ));
        },
        None,
    );
}

#[test]
fn a_read_timeout_lets_the_watcher_stop_the_request() {
    run(
        |s| {
            accept(s);
            s.frame();
            std::thread::sleep(Duration::from_millis(300)); // the answer never comes in time
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            let mut calls = 0;
            let mut watch = |_: Progress| -> Result<(), Stop> {
                calls += 1;
                if calls >= 3 {
                    Err(Stop::Cancelled)
                } else {
                    Ok(())
                }
            };
            assert!(matches!(
                c.request(&get("/"), 0, &mut watch),
                Err(H2Error::Stopped(Stop::Cancelled))
            ));
            assert!(!c.is_usable());
        },
        Some(Duration::from_millis(10)),
    );
}

#[test]
fn an_early_answer_cancels_the_rest_of_the_body() {
    run(
        |s| {
            accept(s);
            s.frame(); // HEADERS
            s.send(&response(1, "200", true, &[("x-early", "1")]));
            loop {
                let (h, _) = s.frame();
                if h.kind == RST_STREAM {
                    assert_eq!(h.stream, 1);
                    break;
                }
            }
        },
        |io| {
            let mut c = Connection::open(io, Options::default()).unwrap();
            let body = vec![0u8; 200_000];
            let req = Request {
                method: "POST",
                path: "/",
                authority: "a",
                headers: &[],
                body: &body,
            };
            let r = c.request(&req, 0, &mut go).unwrap();
            assert_eq!(r.status, 200);
        },
        None,
    );
}

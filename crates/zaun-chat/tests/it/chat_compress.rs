//! The `X` container: thresholds, bounds, and interop with the zstd command line tool.
use super::chat_support::*;
use zaun_chat::chat::compress::{compress_body, compress_to_limit, decompress_container};
use zaun_chat::chat::container::MsgContainer;
use zaun_chat::chat::limits::{MAX_ENCODED_MSG_LENGTH, MAX_ENCODED_MSG_LENGTH_PQ};
use zaun_chat::chat::zstd_frame::{compress, content_size, decompress_limited};
use zaun_chat::chat::{
    ChatError, ChatEvent as E, ChatMessage, encode_chat_message, parse_chat_messages,
};

/// Text that compresses well and makes a body of about `n` bytes.
fn big_message(n: usize) -> ChatMessage {
    msg_with_id(
        &[1, 2, 3, 4],
        E::MsgNew(MsgContainer::text(&"hello ".repeat(n / 6))),
    )
}

fn body_len(m: &ChatMessage) -> usize {
    m.to_json().unwrap().len()
}

#[test]
fn a_body_at_the_limit_is_sent_as_it_is_and_one_over_is_compressed() {
    let m = msg(E::MsgNew(MsgContainer::text("x".repeat(100).as_str())));
    let plain = m.to_json().unwrap();
    assert_eq!(encode_chat_message(&m, false).unwrap(), plain);
    assert_eq!(compress_to_limit(&plain, plain.len()).unwrap(), plain);
    let compressed = compress_to_limit(&plain, plain.len() - 1).unwrap_err();
    assert_eq!(
        compressed,
        ChatError::TooLarge,
        "a body this short does not shrink below itself"
    );
    let big = big_message(MAX_ENCODED_MSG_LENGTH + 100);
    assert!(body_len(&big) > MAX_ENCODED_MSG_LENGTH);
    let out = encode_chat_message(&big, false).unwrap();
    assert_eq!(out[0], b'X');
    assert!(out.len() <= MAX_ENCODED_MSG_LENGTH);
    assert_eq!(parse_chat_messages(&out).unwrap(), vec![big]);
}

#[test]
fn the_pq_limit_is_13380() {
    let m = big_message(MAX_ENCODED_MSG_LENGTH_PQ + 50);
    assert!(body_len(&m) <= MAX_ENCODED_MSG_LENGTH && body_len(&m) > MAX_ENCODED_MSG_LENGTH_PQ);
    assert_eq!(encode_chat_message(&m, false).unwrap()[0], b'{');
    assert_eq!(encode_chat_message(&m, true).unwrap()[0], b'X');
}

#[test]
fn a_body_that_does_not_fit_even_compressed_fails() {
    let mut x = 0x1234_5678_9abc_def0u64;
    let noise: String = (0..40_000)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            char::from(b'a' + (x % 26) as u8)
        })
        .collect();
    let m = msg(E::MsgNew(MsgContainer::text(&noise)));
    assert_eq!(
        encode_chat_message(&m, false).unwrap_err(),
        ChatError::TooLarge
    );
}

#[test]
fn small_bodies_pass_through_up_to_180_bytes_and_use_a_zstd_part_above() {
    let at = vec![b'{'; 180];
    assert_eq!(&compress_body(&at).unwrap()[..3], b"X\x010");
    assert_eq!(compress_body(&at).unwrap()[3], 180);
    let over = vec![b'{'; 181];
    let c = compress_body(&over).unwrap();
    assert_eq!(&c[..3], b"X\x011");
    let len = usize::from(u16::from_be_bytes([c[3], c[4]]));
    assert_eq!(c.len(), 5 + len);
    assert_eq!(decompress_container(&c[1..]).unwrap(), vec![over]);
}

#[test]
fn our_frames_state_their_content_size() {
    for n in [1usize, 181, 5000, 65536] {
        let data = vec![b'a'; n];
        let f = compress(&data).unwrap();
        assert_eq!(content_size(&f), Some(n as u64));
        assert_eq!(decompress_limited(&f, 65536).unwrap(), data);
    }
}

fn container_of(parts: &[(&[u8], bool)]) -> Vec<u8> {
    let mut out = vec![parts.len() as u8];
    for (body, zstd) in parts {
        if *zstd {
            out.push(b'1');
            out.extend_from_slice(&(body.len() as u16).to_be_bytes());
        } else {
            out.push(b'0');
            out.push(body.len() as u8);
        }
        out.extend_from_slice(body);
    }
    out
}

/// Rewrites the Frame_Content_Size field of a frame whose descriptor gives it 2 or 4 bytes.
fn with_content_size(mut frame: Vec<u8>, size: u32) -> Vec<u8> {
    let d = frame[4];
    let at = 5 + usize::from(d >> 5 & 1 == 0);
    match d >> 6 {
        1 => frame[at..at + 2].copy_from_slice(&u16::try_from(size - 256).unwrap().to_le_bytes()),
        2 => frame[at..at + 4].copy_from_slice(&size.to_le_bytes()),
        flag => panic!("unexpected size field flag {flag}"),
    }
    frame
}

#[test]
fn a_part_claiming_more_than_65536_is_refused_before_decoding() {
    let frame = with_content_size(compress(&vec![0u8; 65536]).unwrap(), 65537);
    let e = decompress_container(&container_of(&[(&frame, true)])).unwrap_err();
    assert!(e.to_string().contains("exceeds limit"), "{e}");
}

#[test]
fn parts_whose_sizes_add_up_over_65536_are_refused() {
    let f = compress(&vec![b'a'; 40_000]).unwrap();
    let e = decompress_container(&container_of(&[(&f, true), (&f, true)])).unwrap_err();
    assert!(e.to_string().contains("exceeds limit"), "{e}");
}

#[test]
fn exactly_65536_is_accepted() {
    let f = compress(&vec![b'a'; 65536]).unwrap();
    assert_eq!(
        decompress_container(&container_of(&[(&f, true)])).unwrap()[0].len(),
        65536
    );
}

#[test]
fn a_frame_without_a_content_size_is_refused() {
    // magic, descriptor 0 (no size, not single segment), window byte, an empty last raw block
    let frame = [0x28, 0xB5, 0x2F, 0xFD, 0x00, 0x00, 0x01, 0x00, 0x00];
    assert_eq!(content_size(&frame), None);
    let e = decompress_container(&container_of(&[(&frame, true)])).unwrap_err();
    assert!(
        e.to_string().contains("compressed size not specified"),
        "{e}"
    );
}

#[test]
fn a_frame_that_lies_about_its_size_is_refused() {
    let f = with_content_size(compress(&vec![b'a'; 5000]).unwrap(), 300);
    assert_eq!(content_size(&f), Some(300));
    assert!(decompress_limited(&f, 65536).is_err());
}

#[test]
fn malformed_containers_are_refused() {
    assert!(decompress_container(&[]).is_err());
    assert!(decompress_container(&[0]).is_err(), "an empty list");
    assert!(
        decompress_container(&[1, b'2', 0]).is_err(),
        "an unknown tag"
    );
    assert!(
        decompress_container(&[1, b'0', 5, 1, 2]).is_err(),
        "a short part"
    );
    assert!(
        decompress_container(&[1, b'0', 1, 9, 9]).is_err(),
        "trailing bytes"
    );
    let f = compress(&vec![b'a'; 300]).unwrap();
    assert!(
        decompress_container(&container_of(&[(&f[..f.len() - 3], true)])).is_err(),
        "a truncated frame"
    );
}

#[test]
fn a_container_inside_a_container_is_refused() {
    let inner = compress_body(&[b'{'; 200]).unwrap();
    let outer = container_of(&[(&inner, true)]);
    let mut wire = vec![b'X'];
    wire.extend_from_slice(&outer);
    assert!(parse_chat_messages(&wire).is_err());
}

// The tests below drive the `zstd` command line tool (`nix shell nixpkgs#zstd`).
#[allow(clippy::disallowed_methods, reason = "interop test with the zstd CLI")]
fn zstd_cli(args: &[&str], stdin: Option<&[u8]>) -> Option<Vec<u8>> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut child = Command::new("zstd")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut input = child.stdin.take()?;
    let data = stdin.map(<[u8]>::to_vec);
    let writer = std::thread::spawn(move || {
        if let Some(d) = data {
            let _ = input.write_all(&d);
        }
    });
    let out = child.wait_with_output().ok()?;
    let _ = writer.join();
    out.status.success().then_some(out.stdout)
}

fn tmp(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("zaun-chat-{}-{name}", std::process::id()))
}

#[test]
fn cli_level_3_output_with_content_size_decodes_here() {
    if zstd_cli(&["-V"], None).is_none() {
        eprintln!("zstd CLI not found, skipping");
        return;
    }
    let body = big_message(15_602).to_json().unwrap();
    for data in [body, vec![b'z'; 181], b"{\"v\":\"9\"}".repeat(2000)] {
        // The size is only written when the input is a file, not stdin.
        let (src, dst) = (tmp("in"), tmp("out.zst"));
        std::fs::write(&src, &data).unwrap();
        zstd_cli(
            &[
                "-3",
                "-q",
                "-f",
                "--content-size",
                "-o",
                dst.to_str().unwrap(),
                src.to_str().unwrap(),
            ],
            None,
        )
        .unwrap();
        let frame = std::fs::read(&dst).unwrap();
        assert_eq!(content_size(&frame), Some(data.len() as u64));
        assert_eq!(decompress_limited(&frame, 65536).unwrap(), data);
        let _ = (std::fs::remove_file(&src), std::fs::remove_file(&dst));
    }
}

#[test]
fn cli_output_from_stdin_has_no_content_size_and_is_refused() {
    let Some(frame) = zstd_cli(&["-3", "-c", "-q"], Some(&vec![b'a'; 1000])) else {
        eprintln!("zstd CLI not found, skipping");
        return;
    };
    assert_eq!(content_size(&frame), None);
    assert!(decompress_limited(&frame, 65536).is_err());
}

#[test]
fn the_zstd_cli_decodes_our_frames_and_whole_x_containers() {
    if zstd_cli(&["-V"], None).is_none() {
        eprintln!("zstd CLI not found, skipping");
        return;
    }
    for n in [181usize, 1000, 15602, 65536] {
        let data: Vec<u8> = (0..n).map(|i| b"hello json "[i % 11]).collect();
        let frame = compress(&data).unwrap();
        assert_eq!(zstd_cli(&["-d", "-c", "-q"], Some(&frame)).unwrap(), data);
    }
    let big = big_message(20_000);
    let wire = encode_chat_message(&big, false).unwrap();
    let len = usize::from(u16::from_be_bytes([wire[3], wire[4]]));
    let frame = &wire[5..5 + len];
    assert_eq!(
        zstd_cli(&["-d", "-c", "-q"], Some(frame)).unwrap(),
        big.to_json().unwrap()
    );
}

//! File encryption, chunking and download hygiene: known answers from nook, Haskell's testDecryptChunks sizes.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use zaun_chat::crypto::{CbNonce, SbKey, sha256};
use zaun_chat::xftp::{
    CHUNK_SIZE_0, CHUNK_SIZE_1, CHUNK_SIZE_2, CHUNK_SIZE_3, CancelFlag, EncryptParams,
    FILE_SIZE_LEN, FileHeader, KB, MB, XftpError, claim_unique, decrypt_chunks, encrypt_file,
    encrypted_size, prepare_chunk_sizes, prepare_chunk_specs, safe_file_name, single_chunk_size,
};

use super::hex::hex;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zaun-xftp-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn seq(n: usize, f: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + f) & 0xff) as u8).collect()
}

fn key() -> SbKey {
    SbKey::from_slice(&seq(32, 1)).unwrap()
}

fn nonce() -> CbNonce {
    CbNonce::from_slice(&seq(24, 2)).unwrap()
}

fn header() -> Vec<u8> {
    FileHeader {
        file_name: "a.bin".into(),
        file_extra: None,
    }
    .encode()
    .unwrap()
}

static NO_CANCEL: AtomicBool = AtomicBool::new(false);

fn encrypt(src: &[u8], work: &Path, hdr: &[u8], enc_size: u64) -> Vec<u8> {
    let (s, d) = (work.join("src"), work.join("enc"));
    fs::write(&s, src).unwrap();
    let (k, n) = (key(), nonce());
    let params = EncryptParams {
        src: &s,
        header: hdr,
        key: &k,
        nonce: &n,
        content_len: src.len() as u64,
        enc_size,
        dst: &d,
    };
    encrypt_file(&params, CancelFlag(&NO_CANCEL), &mut |_| {}).unwrap();
    fs::read(d).unwrap()
}

#[test]
fn encrypting_matches_nooks_known_answers() {
    let work = dir("kat");
    let hdr = header();
    for (len, size, sha, head, tag) in [
        (
            1000,
            65536,
            "4ff3d63a47beead472bf84bfee7161b36f15ef2b128f9162b8a4422b5a5ac1d5",
            "db92f29e6eeaadeda6aebbd7aff6007a64de77103fe9e37acb0d9ac84747577d65365fb287742c45",
            "307975513588a73e024cc7e926a9231d",
        ),
        (
            100_000,
            131_072,
            "911ed894025c0de590f7be35754f42569a11a90d4a2b5443667dd78187235db7",
            "db92f29e6eeb28a5a6aebbd7aff6007a64de77103fe9e37acb0d9ac84747577d65365fb287742c45",
            "350510d2e68502cbfaff6e6adc633feb",
        ),
    ] {
        let sizes = prepare_chunk_sizes((hdr.len() + len) as u64 + FILE_SIZE_LEN + 16);
        assert_eq!(sizes.iter().map(|s| u64::from(*s)).sum::<u64>(), size);
        let enc = encrypt(&seq(len, 3), &work, &hdr, size);
        assert_eq!(enc.len() as u64, size);
        assert_eq!(hex(&sha256(&enc)), sha);
        assert_eq!(hex(&enc[..40]), head);
        assert_eq!(hex(&enc[enc.len() - 16..]), tag);
    }
}

fn split_to_chunks(enc: &[u8], sizes: &[u32], work: &Path) -> Vec<PathBuf> {
    prepare_chunk_specs(sizes)
        .iter()
        .zip(1..)
        .map(|(spec, n)| {
            let p = work.join(format!("chunk{n}"));
            fs::write(
                &p,
                &enc[spec.offset as usize..(spec.offset + u64::from(spec.size)) as usize],
            )
            .unwrap();
            p
        })
        .collect()
}

#[test]
fn decrypting_chunk_files_restores_the_file_for_haskells_payload_sizes() {
    let work = dir("roundtrip");
    let hdr = header();
    let sizes_under_test = (128 * KB..=128 * KB + 16).chain([2 * MB + 1, 2 * MB + 512 * KB + 15]);
    for payload in sizes_under_test {
        let content_len = payload - FILE_SIZE_LEN - 16 - hdr.len() as u64;
        let content: Vec<u8> = vec![b'a'; content_len as usize];
        let sizes = prepare_chunk_sizes(payload);
        let enc_size = sizes.iter().map(|s| u64::from(*s)).sum();
        assert_eq!(encrypted_size(payload - FILE_SIZE_LEN - 16), enc_size);
        let enc = encrypt(&content, &work, &hdr, enc_size);
        let chunks = split_to_chunks(&enc, &sizes, &work);
        let dst = work.join("dst");
        let h = decrypt_chunks(
            &chunks,
            &key(),
            &nonce(),
            &dst,
            CancelFlag(&NO_CANCEL),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(h.file_name, "a.bin");
        assert_eq!(fs::read(&dst).unwrap(), content, "payload {payload}");
    }
}

#[test]
fn an_empty_file_and_a_multi_chunk_file_round_trip() {
    let work = dir("shapes");
    let hdr = header();
    for len in [0usize, 5 * MB as usize + 3] {
        let payload = (hdr.len() + len) as u64 + FILE_SIZE_LEN + 16;
        let sizes = prepare_chunk_sizes(payload);
        let enc = encrypt(
            &seq(len, 3),
            &work,
            &hdr,
            sizes.iter().map(|s| u64::from(*s)).sum(),
        );
        let chunks = split_to_chunks(&enc, &sizes, &work);
        decrypt_chunks(
            &chunks,
            &key(),
            &nonce(),
            &work.join("dst"),
            CancelFlag(&NO_CANCEL),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(fs::read(work.join("dst")).unwrap(), seq(len, 3));
    }
}

#[test]
fn a_flipped_bit_or_a_wrong_key_fails_and_leaves_no_output() {
    let work = dir("tamper");
    let hdr = header();
    let mut enc = encrypt(&seq(5000, 3), &work, &hdr, 65536);
    let dst = work.join("dst");
    let run = |enc: &[u8], key: &SbKey| {
        let chunk = work.join("c");
        fs::write(&chunk, enc).unwrap();
        decrypt_chunks(
            &[chunk],
            key,
            &nonce(),
            &dst,
            CancelFlag(&NO_CANCEL),
            &mut |_| {},
        )
    };
    assert!(run(&enc, &key()).is_ok());
    assert!(run(&enc, &SbKey::from_slice(&seq(32, 9)).unwrap()).is_err());
    assert!(!dst.exists());
    enc[100] ^= 1;
    assert!(matches!(run(&enc, &key()), Err(XftpError::Mismatch(_))));
    assert!(!dst.exists(), "an unauthenticated file must not stay");
    enc[100] ^= 1;
    let last = enc.len() - 1;
    enc[last] ^= 1;
    assert!(run(&enc, &key()).is_err());
}

#[test]
fn encryption_stops_when_cancelled_and_refuses_a_too_small_size() {
    let work = dir("cancel");
    let s = work.join("src");
    fs::write(&s, seq(100_000, 3)).unwrap();
    let flag = AtomicBool::new(true);
    let (hdr, k, n, dst) = (header(), key(), nonce(), work.join("enc"));
    let mut params = EncryptParams {
        src: &s,
        header: &hdr,
        key: &k,
        nonce: &n,
        content_len: 100_000,
        enc_size: 131_072,
        dst: &dst,
    };
    let r = encrypt_file(&params, CancelFlag(&flag), &mut |_| {});
    assert!(matches!(r, Err(XftpError::Cancelled)));
    params.enc_size = 1000;
    let r = encrypt_file(&params, CancelFlag(&NO_CANCEL), &mut |_| {});
    assert!(r.is_err());
}

#[test]
fn the_file_header_is_a_name_and_an_optional_extra() {
    let h = FileHeader {
        file_name: "photo.png".into(),
        file_extra: Some("x".into()),
    };
    let bytes = h.encode().unwrap();
    assert_eq!(hex(&bytes), "0970686f746f2e706e67310178");
    let mut with_content = bytes.clone();
    with_content.extend_from_slice(b"CONTENT");
    assert_eq!(FileHeader::parse(&with_content).unwrap(), (h, bytes.len()));
    let long = FileHeader {
        file_name: "é".repeat(200),
        file_extra: None,
    };
    let (back, _) = FileHeader::parse(&long.encode().unwrap()).unwrap();
    assert_eq!(
        back.file_name.len(),
        254,
        "cut to 255 bytes at a character boundary"
    );
}

#[test]
fn chunk_sizes_follow_the_split_rules() {
    assert_eq!(prepare_chunk_sizes(0), Vec::<u32>::new());
    assert_eq!(prepare_chunk_sizes(1), [CHUNK_SIZE_0]);
    assert_eq!(
        prepare_chunk_sizes(u64::from(CHUNK_SIZE_1) * 3 / 4 + 1),
        [CHUNK_SIZE_1]
    );
    assert_eq!(
        prepare_chunk_sizes(u64::from(CHUNK_SIZE_0) + 1),
        [CHUNK_SIZE_0, CHUNK_SIZE_0]
    );
    assert_eq!(
        prepare_chunk_sizes(3 * MB),
        [CHUNK_SIZE_2, CHUNK_SIZE_2, CHUNK_SIZE_2]
    );
    assert_eq!(prepare_chunk_sizes(3 * MB + 1), [CHUNK_SIZE_3]);
    assert_eq!(prepare_chunk_sizes(5 * MB), [CHUNK_SIZE_3, CHUNK_SIZE_2]);
    assert_eq!(
        prepare_chunk_sizes(4 * MB + 100),
        [CHUNK_SIZE_3, CHUNK_SIZE_2]
    );
    assert_eq!(single_chunk_size(1), Some(CHUNK_SIZE_0));
    assert_eq!(
        single_chunk_size(u64::from(CHUNK_SIZE_0) + 1),
        Some(CHUNK_SIZE_1)
    );
    assert_eq!(single_chunk_size(u64::from(CHUNK_SIZE_3) + 1), None);
    let specs = prepare_chunk_specs(&[CHUNK_SIZE_3, CHUNK_SIZE_2]);
    assert_eq!((specs[1].offset, specs[1].size), (4 * MB, CHUNK_SIZE_2));
}

#[test]
fn a_name_from_a_stranger_cannot_leave_the_folder_or_hide() {
    for (given, safe) in [
        ("../../etc/passwd", "passwd"),
        ("..", "file"),
        (".bashrc", "bashrc"),
        ("a\\b\\c.txt", "c.txt"),
        ("  ", "file"),
        ("", "file"),
        ("ok name.txt", "ok name.txt"),
        ("tab\there.txt", "tabhere.txt"),
    ] {
        assert_eq!(safe_file_name(given), safe, "{given:?}");
    }
    assert!(safe_file_name(&"n".repeat(500)).len() <= 200);
}

#[test]
fn claiming_a_name_never_overwrites_a_file() {
    let d = dir("claim");
    let names: Vec<String> = (0..3)
        .map(|_| {
            claim_unique(&d, "pic.png")
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names, ["pic.png", "pic (1).png", "pic (2).png"]);
}

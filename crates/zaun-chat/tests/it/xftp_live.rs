//! Against a real xftp-server. Start a local xftp-server and run
//! `ZAUN_TEST_XFTP=xftp://<fingerprint>@127.0.0.1:<port> cargo test -p zaun-chat --test it xftp_live -- --ignored`.
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use zaun_chat::crypto::random_vec;
use zaun_chat::xftp::description::{
    FileDescription, encode_description, join_description, split_description,
};
use zaun_chat::xftp::{
    ClientConfig, Ctl, DownloadParams, UploadParams, XftpError, XftpServer, connect, delete_file,
    download, upload,
};

fn server() -> XftpServer {
    let Ok(addr) = std::env::var("ZAUN_TEST_XFTP") else {
        panic!("ZAUN_TEST_XFTP is not set")
    };
    addr.parse().unwrap()
}

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("zaun-xftp-live-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);

fn round_trip(name: &str, size: usize) {
    let (work, out, src) = (
        dir(&format!("{name}-work")),
        dir(&format!("{name}-out")),
        dir(&format!("{name}-src")),
    );
    let data = random_vec(size).unwrap();
    let source = src.join("picture.bin");
    fs::write(&source, &data).unwrap();
    let servers = [server()];
    let mut ticks = Vec::new();
    let up = upload(
        &UploadParams {
            source: &source,
            file_name: "picture.bin",
            work_dir: &work,
            servers: &servers,
            recipients: 4,
            storage_hours: None,
        },
        &NOT_CANCELLED,
        &mut |done, total| ticks.push((done, total)),
    )
    .unwrap();
    assert_eq!(up.recipients.len(), 4);
    assert!(
        ticks.windows(2).all(|w| w[0].0 <= w[1].0),
        "progress never goes back"
    );
    assert_eq!(ticks.last().map(|t| t.0), Some(up.encrypted_size));
    for fd in &up.recipients {
        fd.validate().unwrap();
        assert_eq!(fd.size, up.encrypted_size);
    }
    up.sender.validate().unwrap();

    // The description travels as chat message parts, then is read back.
    let text = encode_description(&up.recipients[2]);
    let parts = split_description(&text, 300, 300);
    let received = FileDescription::parse_valid(&join_description(&parts).unwrap()).unwrap();
    assert_eq!(received, up.recipients[2]);

    for (i, fd) in [&received, &up.recipients[0], &up.recipients[3]]
        .into_iter()
        .enumerate()
    {
        let dest = dir(&format!("{name}-out{i}"));
        let got = download(
            &DownloadParams {
                description: fd,
                work_dir: &work,
                dest_dir: &dest,
            },
            &NOT_CANCELLED,
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(
            (got.file_name.as_str(), got.size),
            ("picture.bin", size as u64)
        );
        assert_eq!(fs::read(&got.path).unwrap(), data, "recipient {i}");
    }
    assert_eq!(
        fs::read_dir(&work).unwrap().count(),
        0,
        "no temp file stays"
    );

    delete_file(&up.sender, &NOT_CANCELLED).unwrap();
    let gone = download(
        &DownloadParams {
            description: &received,
            work_dir: &work,
            dest_dir: &out,
        },
        &NOT_CANCELLED,
        &mut |_, _| {},
    );
    assert!(gone.is_err(), "a deleted file cannot be downloaded");
    assert_eq!(fs::read_dir(&out).unwrap().count(), 0);
    assert!(
        delete_file(&up.sender, &NOT_CANCELLED).is_ok(),
        "deleting twice is not an error"
    );
}

#[test]
#[ignore = "needs a local xftp-server; set ZAUN_TEST_XFTP"]
fn a_3_mib_file_goes_up_to_four_recipients_comes_down_and_is_deleted() {
    round_trip("three", 3 << 20);
}

#[test]
#[ignore = "needs a local xftp-server; set ZAUN_TEST_XFTP"]
fn a_5_mib_file_in_two_chunks_round_trips() {
    round_trip("five", 5 << 20 | 12345);
}

#[test]
#[ignore = "needs a local xftp-server; set ZAUN_TEST_XFTP"]
fn a_tiny_file_uses_one_small_chunk() {
    round_trip("tiny", 10);
}

#[test]
#[ignore = "needs a local xftp-server; set ZAUN_TEST_XFTP"]
fn ping_works_and_a_wrong_fingerprint_is_refused() {
    let s = server();
    let mut c = connect(&s, &ClientConfig::default()).unwrap();
    println!("negotiated XFTP v{}", c.client.version());
    assert!(
        c.client.version() >= 3,
        "negotiated v{}",
        c.client.version()
    );
    let clock = zaun_chat::smp::SystemClock::new();
    let mut ignore = |_: zaun_chat::h2::Progress| {};
    let mut ctl = Ctl {
        cancel: &NOT_CANCELLED,
        clock: &clock,
        on_progress: &mut ignore,
    };
    c.client.ping(&mut ctl).unwrap();
    c.close();
    let mut wrong = s.clone();
    wrong.key_hash[0] ^= 1;
    assert!(matches!(
        connect(&wrong, &ClientConfig::default()),
        Err(XftpError::Handshake(_))
    ));
}

#[test]
#[ignore = "needs a local xftp-server; set ZAUN_TEST_XFTP"]
fn a_cancelled_upload_stops_and_leaves_no_temp_file() {
    let (work, src) = (dir("cancel-work"), dir("cancel-src"));
    let source = src.join("f.bin");
    fs::write(&source, random_vec(1 << 20).unwrap()).unwrap();
    let cancelled = AtomicBool::new(true);
    let servers = [server()];
    let r = upload(
        &UploadParams {
            source: &source,
            file_name: "f.bin",
            work_dir: &work,
            servers: &servers,
            recipients: 1,
            storage_hours: None,
        },
        &cancelled,
        &mut |_, _| {},
    );
    assert!(matches!(r, Err(XftpError::Cancelled)));
    assert_eq!(fs::read_dir(&work).unwrap().count(), 0);
}

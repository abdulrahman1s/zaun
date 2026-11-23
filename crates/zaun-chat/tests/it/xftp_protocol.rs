//! XFTP wire formats: commands, blocks, responses, handshake, addresses; nook is the known-answer source for v3.
use zaun_chat::crypto::{Alg, CbNonce, KeyPair, PrivateKey, PublicKey, sha256};
use zaun_chat::encoding::{Version, VersionRange, b64_encode};
use zaun_chat::xftp::{
    BLOCK_SIZE, Command, CommandError, FILE_STORAGE_TIME, FileInfo, HandshakeError, Response,
    ServerHandshake, XftpError, XftpErrorType, XftpServer, decode_transmission,
    encode_client_handshake, encode_transmission, preset_servers,
};

use super::hex::{hex, unhex};

fn seq(n: usize, f: usize) -> Vec<u8> {
    (0..n).map(|i| ((i * 7 + f) & 0xff) as u8).collect()
}

fn pubkey(h: &str) -> PublicKey {
    PublicKey::from_der(&unhex(h)).unwrap()
}

const SND: &str =
    "302a300506032b65700321005d130de8527df4df802812f4726d29236feb8e22e6a8ae43849fc46a3537045a";
const RCV: &str =
    "302a300506032b6570032100d785b49583ee736c66cf55dc87474f4f337c971bfc3d69cba1e21e67763653af";
const FNEW_V3: &str = "464e4557202c302a300506032b65700321005d130de8527df4df802812f4726d29236feb8e22e6a8ae43849fc46a3537045a000100002061672f6a97b118d2488616dc4c89d49307936a371d5e50a52006b29af85f87a3012c302a300506032b6570032100d785b49583ee736c66cf55dc87474f4f337c971bfc3d69cba1e21e67763653af30";

fn fnew(storage_hours: Option<u32>, auth: Option<&str>) -> Command {
    let info = FileInfo {
        snd_key: pubkey(SND),
        size: 65536,
        digest: sha256(&seq(100, 4)).to_vec(),
    };
    Command::New {
        info,
        rcv_keys: vec![pubkey(RCV)],
        basic_auth: auth.map(str::to_owned),
        storage_hours,
    }
}

#[test]
fn fnew_at_v3_is_nooks_encoding() {
    assert_eq!(hex(&fnew(None, None).encode(3).unwrap()), FNEW_V3);
    assert_eq!(
        hex(&fnew(Some(24), None).encode(3).unwrap()),
        FNEW_V3,
        "no storage time before v4"
    );
}

#[test]
fn fnew_at_v4_ends_with_the_requested_storage_time() {
    assert_eq!(
        hex(&fnew(None, None).encode(4).unwrap()),
        format!("{FNEW_V3}30")
    );
    assert_eq!(
        hex(&fnew(Some(24), None).encode(4).unwrap()),
        format!("{FNEW_V3}3100000018")
    );
    let with_auth = hex(&fnew(None, Some("pw")).encode(4).unwrap());
    assert!(
        with_auth.ends_with("3102707730"),
        "Maybe BasicAuth, then no storage time: {with_auth}"
    );
}

#[test]
fn the_other_commands_are_encoded_like_nook() {
    let two = Command::AddRecipients {
        rcv_keys: vec![pubkey(RCV), pubkey(RCV)],
    };
    assert_eq!(
        hex(&two.encode(4).unwrap()),
        format!("464144442002 2c{RCV}2c{RCV}").replace(' ', "")
    );
    let get = Command::Get {
        rcv_dh_key: pubkey(RCV),
    };
    assert_eq!(hex(&get.encode(4).unwrap()), format!("46474554202c{RCV}"));
    for (cmd, text) in [
        (Command::Put, "FPUT"),
        (Command::Delete, "FDEL"),
        (Command::Ack, "FACK"),
        (Command::Ping, "PING"),
    ] {
        assert_eq!(cmd.encode(4).unwrap(), text.as_bytes());
    }
    assert!(
        Command::AddRecipients { rcv_keys: vec![] }
            .encode(4)
            .is_err(),
        "recipients are a non-empty list"
    );
}

fn sender_key() -> PrivateKey {
    PrivateKey::from_raw(Alg::Ed25519, &seq(32, 9)).unwrap()
}

#[test]
fn the_sender_key_is_the_one_the_vectors_were_made_with() {
    assert_eq!(sender_key().public().unwrap(), pubkey(SND));
}

#[test]
fn a_signed_transmission_block_is_nooks_block() {
    let cmd = unhex(FNEW_V3);
    let block = encode_transmission(&seq(32, 5), &[], &cmd, Some(&sender_key())).unwrap();
    assert_eq!(block.len(), BLOCK_SIZE);
    assert_eq!(
        hex(&sha256(&block)),
        "823465dd709533f57ce524b1e0f7bbbd373ee4574cb54e041dc5bd80d2cb6b46"
    );
    assert_eq!(&hex(&block[..16]), "00ed0100ea40ffb89366c9cbab0b5a71");
}

#[test]
fn an_unsigned_ping_block_is_nooks_block() {
    let block = encode_transmission(&seq(32, 5), &[], b"PING", None).unwrap();
    assert_eq!(
        hex(&sha256(&block)),
        "a0f7608854aa4906a01ef4d85f96c6b910684bfda23538a51d5f8c6627a5bac7"
    );
    assert_eq!(&hex(&block[..8]), "002b010028002005");
}

#[test]
fn a_response_block_yields_its_entity_and_command_and_checks_the_session() {
    let session = seq(32, 5);
    let block = encode_transmission(&session, b"entity-id", b"OK", None).unwrap();
    assert_eq!(
        decode_transmission(&session, &block).unwrap(),
        (b"entity-id".to_vec(), b"OK".to_vec())
    );
    assert!(matches!(
        decode_transmission(&seq(32, 6), &block),
        Err(XftpError::Unexpected(_))
    ));
}

#[test]
fn a_bare_error_word_in_a_block_is_the_servers_error() {
    let block = zaun_chat::crypto::pad(b"SESSION", BLOCK_SIZE).unwrap();
    assert!(matches!(
        decode_transmission(&[0; 32], &block),
        Err(XftpError::Server(XftpErrorType::Session))
    ));
}

fn sids(extra: &[u8]) -> Vec<u8> {
    [
        b"SIDS ".as_slice(),
        &[3],
        b"snd",
        &[2, 3],
        b"r_1",
        &[3],
        b"r_2",
        extra,
    ]
    .concat()
}

#[test]
fn sids_carries_the_granted_storage_time_from_v4() {
    let want = |expires| Response::SndIds {
        sender_id: b"snd".to_vec(),
        recipient_ids: vec![b"r_1".to_vec(), b"r_2".to_vec()],
        expires,
    };
    assert_eq!(Response::decode(3, &sids(&[])).unwrap(), want(None));
    let granted = [b"1T".as_slice(), &1_800_000_000i64.to_be_bytes()].concat();
    assert_eq!(
        Response::decode(4, &sids(&granted)).unwrap(),
        want(Some(1_800_000_000))
    );
    assert_eq!(Response::decode(4, &sids(b"0")).unwrap(), want(None));
    assert!(
        Response::decode(4, &sids(&[])).is_err(),
        "v4 always has the Maybe"
    );
}

#[test]
fn the_other_responses_decode() {
    let rids = [b"RIDS ".as_slice(), &[1, 2], b"ab"].concat();
    assert_eq!(
        Response::decode(4, &rids).unwrap(),
        Response::RcvIds {
            recipient_ids: vec![b"ab".to_vec()]
        }
    );
    let kp = KeyPair::generate(Alg::X25519).unwrap();
    let nonce = seq(24, 7);
    let file = [b"FILE ".as_slice(), &[44], &kp.public.to_der(), &nonce].concat();
    assert_eq!(
        Response::decode(4, &file).unwrap(),
        Response::File {
            dh_key: kp.public,
            nonce: CbNonce::from_slice(&nonce).unwrap()
        }
    );
    assert_eq!(Response::decode(4, b"OK").unwrap(), Response::Ok);
    assert_eq!(Response::decode(4, b"PONG").unwrap(), Response::Pong);
    assert!(Response::decode(4, b"WAT").is_err());
}

#[test]
fn error_responses_name_the_servers_error() {
    for (text, want) in [
        ("AUTH", XftpErrorType::Auth),
        ("NO_FILE", XftpErrorType::NoFile),
        ("CMD SYNTAX", XftpErrorType::Cmd(CommandError::Syntax)),
        ("CMD NO_AUTH", XftpErrorType::Cmd(CommandError::NoAuth)),
        ("CMD NO_QUEUE", XftpErrorType::Cmd(CommandError::NoEntity)),
        ("QUOTA", XftpErrorType::Quota),
        (
            "BLOCKED {\"reason\":\"spam\"}",
            XftpErrorType::Blocked("{\"reason\":\"spam\"}".into()),
        ),
    ] {
        let r = Response::decode(4, format!("ERR {text}").as_bytes()).unwrap();
        assert_eq!(r, Response::Err(want.clone()));
        assert_eq!(want.to_string(), text.replace("NO_QUEUE", "NO_ENTITY"));
    }
    assert!(Response::decode(4, b"ERR NOPE").is_err());
    assert!(XftpErrorType::Timeout.is_temporary() && !XftpErrorType::Auth.is_temporary());
}

fn range(min: u16, max: u16) -> VersionRange {
    VersionRange::new(Version(min), Version(max)).unwrap()
}

fn server_hello(max: u16) -> ServerHandshake {
    ServerHandshake {
        range: range(1, max),
        session_id: seq(32, 5),
        chain: vec![seq(300, 1), seq(200, 2)],
        signed_key: seq(120, 3),
        web_proof: None,
        info: (max >= FILE_STORAGE_TIME).then(|| b"{\"x\":1}".to_vec()),
    }
}

#[test]
fn the_server_handshake_round_trips_with_server_info_from_v4() {
    for max in [3, 4] {
        let hello = server_hello(max);
        assert_eq!(
            ServerHandshake::decode_block(&hello.encode_block().unwrap()).unwrap(),
            hello
        );
    }
}

#[test]
fn a_web_proof_and_unknown_trailing_fields_are_read_past() {
    let hello = ServerHandshake {
        web_proof: Some(vec![9; 64]),
        ..server_hello(4)
    };
    assert_eq!(
        ServerHandshake::decode_block(&hello.encode_block().unwrap()).unwrap(),
        hello
    );
    let mut raw = zaun_chat::crypto::unpad(&server_hello(3).encode_block().unwrap())
        .unwrap()
        .to_vec();
    raw.extend_from_slice(b"future fields");
    let block = zaun_chat::crypto::pad(&raw, BLOCK_SIZE).unwrap();
    assert_eq!(
        ServerHandshake::decode_block(&block).unwrap().range,
        range(1, 3)
    );
}

#[test]
fn a_garbled_or_refusing_handshake_block_is_an_error() {
    let junk = zaun_chat::crypto::pad(&[1, 2, 3], BLOCK_SIZE).unwrap();
    assert!(matches!(
        ServerHandshake::decode_block(&junk),
        Err(XftpError::Handshake(HandshakeError::Parse))
    ));
    let refused = zaun_chat::crypto::pad(b"HANDSHAKE", BLOCK_SIZE).unwrap();
    assert!(matches!(
        ServerHandshake::decode_block(&refused),
        Err(XftpError::Server(XftpErrorType::Handshake))
    ));
}

#[test]
fn the_client_handshake_at_v3_is_nooks_and_at_v4_adds_no_entitlement() {
    let v3 = encode_client_handshake(3, &seq(32, 6)).unwrap();
    assert_eq!(
        hex(&sha256(&v3)),
        "b5c372bec5606a6d36e3ac98d8b09b54472c1bea2a324c41a48f0099501b1b49"
    );
    let v4 = encode_client_handshake(4, &seq(32, 6)).unwrap();
    assert_eq!(&hex(&v4[..2]), "0024");
    assert_eq!(&hex(&v4[2..4]), "0004");
    assert_eq!(v4[2 + 2 + 33], b'0');
    assert_eq!(v4.len(), BLOCK_SIZE);
}

const FP: &str = "da1aH3nOT-9G8lV7bWamhxpDYdJ1xmW7j3JpGaDR5Ug=";

#[test]
fn addresses_keep_hosts_port_and_key_hash_and_default_to_443() {
    let s: XftpServer = format!("xftp://{FP}@xftp1.simplex.im,abc.onion:5443")
        .parse()
        .unwrap();
    assert_eq!(
        (s.hosts.as_slice(), s.port),
        (
            ["xftp1.simplex.im".to_owned(), "abc.onion".to_owned()].as_slice(),
            Some(5443)
        )
    );
    assert_eq!(s.key_hash.len(), 32);
    assert_eq!(
        s.to_string(),
        format!("xftp://{FP}@xftp1.simplex.im,abc.onion:5443")
    );
    assert_eq!(s.dialable_hosts().collect::<Vec<_>>(), ["xftp1.simplex.im"]);
    let d: XftpServer = format!("xftp://{FP}@h").parse().unwrap();
    assert_eq!((d.port, d.port_or_default()), (None, 443));
    let v6: XftpServer = format!("xftp://{FP}@[2001:db8::1]:8443").parse().unwrap();
    assert_eq!((v6.hosts[0].as_str(), v6.port), ("2001:db8::1", Some(8443)));
    assert_eq!(v6.to_string(), format!("xftp://{FP}@[2001:db8::1]:8443"));
}

#[test]
fn malformed_addresses_are_rejected() {
    assert!("smp://abc@h".parse::<XftpServer>().is_err());
    assert!(format!("xftp://{FP}").parse::<XftpServer>().is_err());
    assert!(format!("xftp://{FP}@a,,b").parse::<XftpServer>().is_err());
    assert!("xftp://!!!@h".parse::<XftpServer>().is_err());
}

#[test]
fn servers_order_as_haskells_derived_ord_does() {
    let s = |a: &str| a.parse::<XftpServer>().unwrap();
    let mut list = [
        s(&format!("xftp://{FP}@zzz.com")),
        s(&format!("xftp://{FP}@10.0.0.2")),
        s(&format!("xftp://{FP}@abc.onion")),
        s(&format!("xftp://{FP}@aaa.com:5443")),
        s(&format!("xftp://{FP}@aaa.com")),
        s(&format!("xftp://{FP}@10.0.0.10")),
    ];
    list.sort_by(|a, b| a.cmp_haskell(b));
    let hosts: Vec<String> = list
        .iter()
        .map(|x| format!("{}:{}", x.hosts[0], x.port.unwrap_or(0)))
        .collect();
    assert_eq!(
        hosts,
        [
            "10.0.0.2:0",
            "10.0.0.10:0",
            "abc.onion:0",
            "aaa.com:0",
            "aaa.com:5443",
            "zzz.com:0"
        ]
    );
}

#[test]
fn the_twelve_preset_servers_parse_with_32_byte_key_hashes() {
    let presets = preset_servers();
    assert_eq!(presets.len(), 12);
    for p in &presets {
        assert_eq!(p.key_hash.len(), 32);
        assert_eq!(p.hosts.len(), 2);
        assert_eq!(p.dialable_hosts().count(), 1);
    }
    assert_eq!(b64_encode(&presets[0].key_hash, true), FP);
}

#[test]
fn a_server_that_never_answers_makes_connect_fail_instead_of_hang() {
    use std::sync::mpsc;
    use std::time::Duration;
    use zaun_chat::xftp::{ClientConfig, connect};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done, hold) = mpsc::channel::<()>();
    // The silent server keeps the connection open until connect has given up.
    let holder = std::thread::spawn(move || {
        let held = listener.accept();
        let _ = hold.recv_timeout(Duration::from_secs(30));
        drop(held);
    });
    let server: XftpServer = format!("xftp://{FP}@127.0.0.1:{port}").parse().unwrap();
    let cfg = ClientConfig {
        connect_timeout: Duration::from_millis(300),
        ..ClientConfig::default()
    };
    assert!(matches!(connect(&server, &cfg), Err(XftpError::Connect(_))));
    done.send(()).unwrap();
    holder.join().unwrap();
}

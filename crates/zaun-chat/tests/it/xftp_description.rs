//! File descriptions against Haskell's FileDescriptionTests.hs and its fixture tests/fixtures/file_description.yaml.
use zaun_chat::encoding::b64_decode;
use zaun_chat::xftp::description::*;
use zaun_chat::xftp::{
    CHUNK_SIZE_0, CHUNK_SIZE_3, GB, MAX_FILE_SIZE_HARD, MB, SERVER_CHUNK_SIZES, XftpServer,
    prepare_chunk_sizes,
};

const FIXTURE: &str = "chunkSize: 4mb
digest: YWJj
key: 00n8p1tJq5E-SGnHcYTOrS4A9I07gTA_WFD6MTFFFOY=
nonce: dPSF-wrQpDiK_K6sYv0BDBZ9S4dg-jmu
party: recipient
replicas:
- chunks:
  - 1:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Z2hp
  - 3:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Z2hp
  server: xftp://abc=@example1.com
- chunks:
  - 2:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Z2hp
  - 4:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Z2hp:1mb
  server: xftp://abc=@example2.com
- chunks:
  - 1:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe
  - 4:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe
  server: xftp://abc=@example3.com
- chunks:
  - 2:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe
  - 3:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe
  server: xftp://abc=@example4.com
size: 13mb
";

fn server(host: &str) -> XftpServer {
    format!("xftp://abc=@{host}").parse().unwrap()
}

fn replica(host: &str) -> ChunkReplica {
    ChunkReplica {
        server: server(host),
        replica_id: b"abc".to_vec(),
        replica_key: b64_decode("MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe")
            .unwrap(),
    }
}

fn chunk(no: u32, size: u32, hosts: [&str; 2]) -> FileChunk {
    FileChunk {
        chunk_no: no,
        chunk_size: size,
        digest: b"ghi".to_vec(),
        replicas: hosts.map(replica).to_vec(),
    }
}

fn fixture_description() -> FileDescription {
    let four = 4 * MB as u32;
    FileDescription {
        party: Party::Recipient,
        size: 13 * MB,
        digest: b"abc".to_vec(),
        key: b64_decode("00n8p1tJq5E-SGnHcYTOrS4A9I07gTA_WFD6MTFFFOY=").unwrap(),
        nonce: b64_decode("dPSF-wrQpDiK_K6sYv0BDBZ9S4dg-jmu").unwrap(),
        chunk_size: four,
        chunks: vec![
            chunk(1, four, ["example1.com", "example3.com"]),
            chunk(2, four, ["example2.com", "example4.com"]),
            chunk(3, four, ["example1.com", "example4.com"]),
            chunk(4, MB as u32, ["example2.com", "example3.com"]),
        ],
        redirect: None,
    }
}

#[test]
fn the_haskell_fixture_parses_to_the_expected_description() {
    assert_eq!(
        FileDescription::parse(FIXTURE).unwrap(),
        fixture_description()
    );
}

#[test]
fn the_description_prints_to_the_haskell_fixture_byte_for_byte() {
    assert_eq!(encode_description(&fixture_description()), FIXTURE);
}

#[test]
fn a_valid_description_passes_validation_and_round_trips() {
    let fd = FileDescription::parse_valid(FIXTURE).unwrap();
    assert_eq!(
        FileDescription::parse_valid(&encode_description(&fd)).unwrap(),
        fd
    );
}

#[test]
fn yaml_aliases_are_rejected() {
    let aliased = FIXTURE
        .replacen("- chunks:", "- &g chunks:", 1)
        .replace("size: 13mb", "- *g\nsize: 13mb");
    assert_eq!(
        FileDescription::parse(&aliased).unwrap_err(),
        "YAML aliases are not allowed"
    );
}

#[test]
fn a_run_of_65_hex_digits_is_rejected_and_64_is_accepted() {
    let with = |n: usize| format!("{FIXTURE}x: {}\n", "7".repeat(n));
    assert_eq!(
        FileDescription::parse(&with(65)).unwrap_err(),
        "number too long in file description"
    );
    assert!(FileDescription::parse(&with(64)).is_ok());
}

fn description_of(sizes: &[u32], replicas: usize) -> FileDescription {
    let fixture = fixture_description();
    let pool: Vec<ChunkReplica> = fixture
        .chunks
        .iter()
        .flat_map(|c| c.replicas.clone())
        .collect();
    FileDescription {
        size: sizes.iter().map(|s| u64::from(*s)).sum(),
        chunk_size: CHUNK_SIZE_3,
        chunks: sizes
            .iter()
            .zip(1u32..)
            .map(|(s, n)| FileChunk {
                chunk_no: n,
                chunk_size: *s,
                digest: b"ghi".to_vec(),
                replicas: pool.iter().cycle().take(replicas).cloned().collect(),
            })
            .collect(),
        ..fixture
    }
}

fn redirect_of(sizes: &[u32]) -> FileDescription {
    FileDescription {
        redirect: Some(Redirect {
            size: 1024,
            digest: b"abc".to_vec(),
        }),
        ..description_of(sizes, 1)
    }
}

#[test]
fn chunk_sizes_replicas_and_redirect_size_are_limited() {
    let largest = prepare_chunk_sizes(MAX_FILE_SIZE_HARD - MB);
    let split64 = prepare_chunk_sizes(64 * GB - MB);
    assert_eq!((largest.len(), split64.len()), (1282, 16386));
    assert!(description_of(&largest, 16).validate().is_ok());
    assert_eq!(
        description_of(&largest, 17).validate().unwrap_err(),
        "too many chunk replicas"
    );
    assert!(description_of(&split64, 4).validate().is_ok());
    assert!(description_of(&[CHUNK_SIZE_3; 8192], 16).validate().is_ok());
    assert_eq!(
        description_of(&[CHUNK_SIZE_3; 8193], 16)
            .validate()
            .unwrap_err(),
        "too many file replicas"
    );
    assert!(description_of(&SERVER_CHUNK_SIZES, 1).validate().is_ok());
    assert_eq!(
        description_of(&[CHUNK_SIZE_3, 2 * MB as u32], 1)
            .validate()
            .unwrap_err(),
        "incorrect chunk size"
    );
    assert_eq!(
        description_of(&[CHUNK_SIZE_3, 8 * MB as u32], 1)
            .validate()
            .unwrap_err(),
        "incorrect chunk size"
    );
    assert!(redirect_of(&[CHUNK_SIZE_3]).validate().is_ok());
    assert_eq!(
        redirect_of(&[CHUNK_SIZE_3, CHUNK_SIZE_0])
            .validate()
            .unwrap_err(),
        "redirect file description is too large"
    );
}

#[test]
fn chunk_numbers_and_total_size_are_checked() {
    let mut fd = fixture_description();
    fd.chunks[1].chunk_no = 5;
    assert_eq!(
        fd.validate().unwrap_err(),
        "chunk numbers are not sequential"
    );
    let mut fd = fixture_description();
    fd.size += 1;
    assert_eq!(
        fd.validate().unwrap_err(),
        "chunks total size is different than file size"
    );
}

#[test]
fn file_sizes_print_in_the_largest_exact_unit() {
    for (n, s) in [
        (13 * MB, "13mb"),
        (1, "1"),
        (1536, "1536"),
        (2048, "2kb"),
        (GB, "1gb"),
        (1025, "1025"),
        (0, "0gb"),
    ] {
        assert_eq!(encode_file_size(n), s);
    }
    for (s, n) in [("4mb", 4 * MB), ("7", 7), ("3kb", 3072), ("2gb", 2 * GB)] {
        assert_eq!(decode_file_size(s).unwrap(), n);
    }
    assert!(
        decode_file_size("mb").is_err()
            && decode_file_size("1.5mb").is_err()
            && decode_file_size("12x").is_err()
    );
}

#[test]
fn a_redirect_description_keeps_its_numeric_size() {
    let fd = redirect_of(&[CHUNK_SIZE_3]);
    let text = encode_description(&fd);
    assert!(
        text.contains("redirect:\n  digest: YWJj\n  size: 1024\nreplicas:"),
        "{text}"
    );
    assert_eq!(FileDescription::parse(&text).unwrap(), fd);
}

#[test]
fn a_description_without_chunks_prints_an_empty_replica_list() {
    let fd = FileDescription {
        chunks: vec![],
        size: 0,
        ..fixture_description()
    };
    let text = encode_description(&fd);
    assert!(text.contains("replicas: []\n"));
    assert_eq!(FileDescription::parse(&text).unwrap(), fd);
}

#[test]
fn a_digest_that_reads_as_a_number_is_quoted_and_survives() {
    let fd = FileDescription {
        digest: b64_decode("1234").unwrap(),
        ..fixture_description()
    };
    let text = encode_description(&fd);
    assert!(
        text.contains("digest: '1234'\n") || text.contains("digest: \"1234\"\n"),
        "quoted: {text}"
    );
    assert_eq!(FileDescription::parse(&text).unwrap().digest, fd.digest);
}

#[test]
fn servers_group_by_haskell_order_and_keep_each_hosts_list() {
    let mut fd = fixture_description();
    for c in &mut fd.chunks {
        for r in &mut c.replicas {
            r.server = format!("xftp://abc=@{},b4gxbdgx.onion", r.server.hosts[0])
                .parse()
                .unwrap();
        }
    }
    let text = encode_description(&fd);
    assert!(text.contains("server: xftp://abc=@example1.com,b4gxbdgx.onion\n"));
    assert_eq!(FileDescription::parse(&text).unwrap(), fd);
}

#[test]
fn indented_block_lists_and_comments_are_read() {
    let text = "# a comment\nchunkSize: 64kb\ndigest: YWJj\nkey: 00n8p1tJq5E-SGnHcYTOrS4A9I07gTA_WFD6MTFFFOY=\nnonce: dPSF-wrQpDiK_K6sYv0BDBZ9S4dg-jmu\nparty: sender\nreplicas:\n  - chunks:\n      - 1:YWJj:MC4C:Z2hp\n    server: \"xftp://abc=@example1.com\"\nsize: 64kb\n";
    let fd = FileDescription::parse(text).unwrap();
    assert_eq!(
        (fd.party, fd.chunks.len(), fd.chunks[0].replicas.len()),
        (Party::Sender, 1, 1)
    );
    assert_eq!(fd.chunks[0].digest, b"ghi");
}

#[test]
fn a_chunk_with_conflicting_digests_or_no_digest_is_rejected() {
    let conflict = FIXTURE.replacen(
        "3:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Z2hp",
        "1:YWJj:MC4CAQAwBQYDK2VwBCIEIDfEfevydXXfKajz3sRkcQ7RPvfWUPoq6pu1TYHV1DEe:Zm9v",
        1,
    );
    assert_eq!(
        FileDescription::parse(&conflict).unwrap_err(),
        "different size or digest in chunk replicas"
    );
    let none = FIXTURE.replace(":Z2hp", "");
    assert_eq!(
        FileDescription::parse(&none).unwrap_err(),
        "no digest for chunk"
    );
}

#[test]
fn text_splits_into_numbered_parts_and_joins_back() {
    let text = "x".repeat(30000);
    let parts = split_description(&text, DESCR_PART_SIZE, DESCR_PART_SIZE);
    assert_eq!(
        parts.iter().map(|p| p.part_no).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(
        parts.iter().map(|p| p.complete).collect::<Vec<_>>(),
        [false, false, true]
    );
    assert_eq!(
        parts.iter().map(|p| p.text.len()).collect::<Vec<_>>(),
        [14000, 14000, 2000]
    );
    assert_eq!(join_description(&parts).unwrap(), text);
}

#[test]
fn parts_are_cut_by_characters_and_the_last_part_keeps_room_for_a_badge() {
    let text = "é".repeat(13700);
    let parts = split_description(&text, DESCR_PART_SIZE, BADGE_DESCR_PART_SIZE);
    assert_eq!(
        parts
            .iter()
            .map(|p| p.text.chars().count())
            .collect::<Vec<_>>(),
        [13500, 200]
    );
    assert_eq!(
        split_description("short", DESCR_PART_SIZE, BADGE_DESCR_PART_SIZE).len(),
        1
    );
    assert_eq!(join_description(&parts).unwrap(), text);
}

#[test]
fn received_parts_must_arrive_in_order_and_stay_within_limits() {
    let text = "y".repeat(500);
    let parts = split_description(&text, 200, 200);
    let mut a = DescrAssembler::default();
    assert!(a.add(&parts[1]).is_err(), "part 2 before part 1");
    let mut a = DescrAssembler::default();
    assert_eq!(a.add(&parts[0]).unwrap(), None);
    assert!(a.add(&parts[0]).is_err(), "part 1 twice");
    assert_eq!(a.add(&parts[1]).unwrap(), None);
    assert_eq!(a.add(&parts[2]).unwrap(), Some(text));
    assert!(a.add(&parts[2]).is_err(), "after the complete part");
    let huge = DescrPart {
        text: "z".repeat(1024 * 1024 + 1),
        part_no: 1,
        complete: true,
    };
    assert!(DescrAssembler::default().add(&huge).is_err());
}

//! HPACK against RFC 7541 Appendix C, ported from nook src/sx/h2/hpack.test.ts.
use zaun_chat::h2::{
    Headers, HpackDecoder, Huffman, decode_int, encode_headers, encode_int, huffman_encode,
};

use super::hex::{hex, unhex};

const BIG: usize = 1 << 20;

fn decoder(limit: usize) -> HpackDecoder {
    HpackDecoder::new(limit, BIG)
}

fn pairs(list: &[(&str, &str)]) -> Headers {
    list.iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect()
}

fn int_hex(value: usize, prefix: u8) -> String {
    let mut out = Vec::new();
    encode_int(value, prefix, 0, &mut out);
    hex(&out)
}

#[test]
fn prefix_integers_encode_the_rfc_examples() {
    assert_eq!(int_hex(10, 5), "0a");
    assert_eq!(int_hex(1337, 5), "1f9a0a");
    assert_eq!(int_hex(42, 8), "2a");
}

#[test]
fn prefix_integers_decode_back_and_reject_truncation() {
    for (v, bits) in [(10, 5), (1337, 5), (42, 8), (0, 7), (127, 7), (1 << 20, 4)] {
        let mut out = Vec::new();
        encode_int(v, bits, 0, &mut out);
        let mut pos = 0;
        assert_eq!(decode_int(&out, &mut pos, bits).unwrap(), v);
        assert_eq!(pos, out.len());
    }
    assert!(decode_int(&unhex("1f9a"), &mut 0, 5).is_err());
}

#[test]
fn a_literal_with_a_long_value_decodes() {
    let value = "x".repeat(1337);
    let mut block = vec![0x00, 1, b'a', 0x7f];
    block.extend(
        [1337 - 127]
            .iter()
            .flat_map(|v| [(v & 0x7f) as u8 | 0x80, (v >> 7) as u8]),
    );
    block.extend(value.bytes());
    assert_eq!(
        decoder(4096).decode(&block).unwrap(),
        pairs(&[("a", &value)])
    );
}

#[test]
fn huffman_matches_the_rfc_examples() {
    assert_eq!(
        hex(&huffman_encode(b"www.example.com")),
        "f1e3c2e5f23a6ba0ab90f4ff"
    );
    let h = Huffman::new();
    assert_eq!(
        h.decode(&unhex("f1e3c2e5f23a6ba0ab90f4ff")).unwrap(),
        b"www.example.com"
    );
    assert_eq!(h.decode(&unhex("a8eb10649cbf")).unwrap(), b"no-cache");
}

#[test]
fn huffman_round_trips_every_byte_value() {
    let h = Huffman::new();
    let all: Vec<u8> = (0..=255).collect();
    assert_eq!(h.decode(&huffman_encode(&all)).unwrap(), all);
    let noise: Vec<u8> = (0..500u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    assert_eq!(h.decode(&huffman_encode(&noise)).unwrap(), noise);
}

#[test]
fn huffman_rejects_bad_padding_and_eos() {
    let h = Huffman::new();
    assert!(h.decode(&unhex("f1e3c2e5f23a6ba0ab90f4fe")).is_err());
    assert!(h.decode(&unhex("ffffffff")).is_err());
}

#[test]
fn single_representations_of_rfc_c2_decode() {
    assert_eq!(
        decoder(4096)
            .decode(&unhex(
                "400a637573746f6d2d6b65790d637573746f6d2d686561646572"
            ))
            .unwrap(),
        pairs(&[("custom-key", "custom-header")])
    );
    assert_eq!(
        decoder(4096)
            .decode(&unhex("040c2f73616d706c652f70617468"))
            .unwrap(),
        pairs(&[(":path", "/sample/path")])
    );
    assert_eq!(
        decoder(4096)
            .decode(&unhex("100870617373776f726406736563726574"))
            .unwrap(),
        pairs(&[("password", "secret")])
    );
    assert_eq!(
        decoder(4096).decode(&unhex("82")).unwrap(),
        pairs(&[(":method", "GET")])
    );
    let mut d = decoder(4096);
    d.decode(&unhex(
        "400a637573746f6d2d6b65790d637573746f6d2d686561646572",
    ))
    .unwrap();
    assert_eq!(d.dynamic_table_size(), 55);
}

fn requests(blocks: &[&str]) -> (HpackDecoder, Vec<Headers>) {
    let mut d = decoder(4096);
    let out = blocks
        .iter()
        .map(|b| d.decode(&unhex(b)).unwrap())
        .collect();
    (d, out)
}

fn expected_requests() -> Vec<Headers> {
    let r1 = [
        (":method", "GET"),
        (":scheme", "http"),
        (":path", "/"),
        (":authority", "www.example.com"),
    ];
    let r2 = [r1.as_slice(), &[("cache-control", "no-cache")]].concat();
    let r3 = [
        (":method", "GET"),
        (":scheme", "https"),
        (":path", "/index.html"),
        (":authority", "www.example.com"),
        ("custom-key", "custom-value"),
    ];
    vec![pairs(&r1), pairs(&r2), pairs(&r3)]
}

#[test]
fn requests_without_huffman_use_the_dynamic_table_rfc_c3() {
    let (d, out) = requests(&[
        "828684410f7777772e6578616d706c652e636f6d",
        "828684be58086e6f2d6361636865",
        "828785bf400a637573746f6d2d6b65790c637573746f6d2d76616c7565",
    ]);
    assert_eq!(out, expected_requests());
    assert_eq!(d.dynamic_table_size(), 164);
    assert_eq!(
        d.dynamic_table()[0],
        ("custom-key".to_owned(), "custom-value".to_owned())
    );
}

#[test]
fn requests_with_huffman_decode_the_same_rfc_c4() {
    let (d, out) = requests(&[
        "828684418cf1e3c2e5f23a6ba0ab90f4ff",
        "828684be5886a8eb10649cbf",
        "828785bf408825a849e95ba97d7f8925a849e95bb8e8b4bf",
    ]);
    assert_eq!(out, expected_requests());
    assert_eq!(d.dynamic_table_size(), 164);
}

fn expected_responses() -> Vec<Headers> {
    let s1 = [
        (":status", "302"),
        ("cache-control", "private"),
        ("date", "Mon, 21 Oct 2013 20:13:21 GMT"),
        ("location", "https://www.example.com"),
    ];
    let s2 = [
        (":status", "307"),
        ("cache-control", "private"),
        ("date", "Mon, 21 Oct 2013 20:13:21 GMT"),
        ("location", "https://www.example.com"),
    ];
    let s3 = [
        (":status", "200"),
        ("cache-control", "private"),
        ("date", "Mon, 21 Oct 2013 20:13:22 GMT"),
        ("location", "https://www.example.com"),
        ("content-encoding", "gzip"),
        (
            "set-cookie",
            "foo=ASDJKHQKBZXOQWEOPIUAXQWEOIU; max-age=3600; version=1",
        ),
    ];
    vec![pairs(&s1), pairs(&s2), pairs(&s3)]
}

fn responses(blocks: [&str; 3]) {
    let mut d = decoder(256);
    let want = expected_responses();
    assert_eq!(d.decode(&unhex(blocks[0])).unwrap(), want[0]);
    assert_eq!(d.dynamic_table_size(), 222);
    assert_eq!(d.decode(&unhex(blocks[1])).unwrap(), want[1]);
    assert_eq!(d.dynamic_table_size(), 222);
    assert_eq!(d.decode(&unhex(blocks[2])).unwrap(), want[2]);
    assert_eq!(d.dynamic_table_size(), 215);
}

#[test]
fn responses_without_huffman_evict_from_a_256_byte_table_rfc_c5() {
    responses([
        "4803333032580770726976617465611d4d6f6e2c203231204f637420323031332032303a31333a323120474d546e1768747470733a2f2f7777772e6578616d706c652e636f6d",
        "4803333037c1c0bf",
        "88c1611d4d6f6e2c203231204f637420323031332032303a31333a323220474d54c05a04677a69707738666f6f3d4153444a4b48514b425a584f5157454f50495541585157454f49553b206d61782d6167653d333630303b2076657273696f6e3d31",
    ]);
}

#[test]
fn responses_with_huffman_evict_from_a_256_byte_table_rfc_c6() {
    responses([
        "488264025885aec3771a4b6196d07abe941054d444a8200595040b8166e082a62d1bff6e919d29ad171863c78f0b97c8e9ae82ae43d3",
        "4883640effc1c0bf",
        "88c16196d07abe941054d444a8200595040b8166e084a62d1bffc05a839bd9ab77ad94e7821dd7f2e6c7b335dfdfcd5b3960d5af27087f3672c1ab270fb5291f9587316065c003ed4ee5b1063d5007",
    ]);
}

#[test]
fn a_table_size_update_evicts_and_abuse_is_rejected() {
    let mut d = decoder(4096);
    d.decode(&unhex(
        "400a637573746f6d2d6b65790d637573746f6d2d686561646572",
    ))
    .unwrap();
    assert!(d.decode(&unhex("20")).unwrap().is_empty());
    assert_eq!(d.dynamic_table_size(), 0);
    assert!(
        d.decode(&unhex("3fe21f")).is_err(),
        "above the announced limit"
    );
    assert!(
        decoder(4096).decode(&unhex("8220")).is_err(),
        "update after a field"
    );
    assert!(
        decoder(4096).decode(&unhex("be")).is_err(),
        "empty dynamic table index"
    );
    assert!(decoder(4096).decode(&unhex("80")).is_err(), "index 0");
}

#[test]
fn a_header_list_over_the_cap_is_rejected() {
    let mut d = HpackDecoder::new(4096, 10);
    assert!(
        d.decode(&unhex(
            "400a637573746f6d2d6b65790d637573746f6d2d686561646572"
        ))
        .is_err()
    );
}

#[test]
fn request_encoding_uses_indexed_fields_and_literals_our_decoder_reads() {
    let headers = [
        (":method", "POST"),
        (":scheme", "https"),
        (":path", "/"),
        (":authority", "xftp1.example.com"),
        ("xftp-web-hello", "1"),
        ("content-length", "16384"),
    ];
    let block = encode_headers(&headers);
    assert_eq!(hex(&block[..3]), "838784");
    assert_eq!(decoder(4096).decode(&block).unwrap(), pairs(&headers));
}

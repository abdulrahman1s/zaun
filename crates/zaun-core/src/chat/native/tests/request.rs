use zaun_chat_wire::{ReqLine, decode_line, encode_line};

use super::samples::{VARIANTS, index, requests};
use crate::chat::native::to_wire::request;

#[test]
fn every_request_variant_has_a_sample() {
    let seen: Vec<usize> = requests().iter().map(index).collect();
    assert_eq!(seen, (0..VARIANTS).collect::<Vec<_>>());
}

#[test]
fn every_request_survives_the_wire_and_means_what_it_meant() {
    for r in requests() {
        let want = format!("{r:?}");
        let wire = request(r);
        let line = encode_line(&ReqLine { req: wire.clone() }).unwrap();
        assert!(!line.contains('\n'));
        let back: ReqLine = decode_line(&line).unwrap();
        assert_eq!(back.req, wire, "{want}");
        // The wire's variants and fields are named as core's, so the two print alike.
        assert_eq!(format!("{:?}", back.req), want);
    }
}

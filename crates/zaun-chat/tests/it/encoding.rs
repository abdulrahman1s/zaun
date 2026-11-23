//! Encoding tests: the byte layouts of Encoding.hs.
use zaun_chat::encoding::{
    Decode, DecodeError, Decoder, Encode, EncodeError, Large, List, NonEmpty, SysTime, Tail,
    Version, VersionRange, b64_decode, b64_encode,
};

use crate::hex::{hex, unhex};

fn enc<T: Encode>(v: &T) -> String {
    hex(&v.to_bytes().unwrap())
}

#[test]
fn integers_are_big_endian_and_int64_is_two_word32s() {
    assert_eq!(enc(&0x0102u16), "0102");
    assert_eq!(enc(&0x01020304u32), "01020304");
    assert_eq!(enc(&0x0102030405060708i64), "0102030405060708");
    assert_eq!(enc(&-1i64), "ffffffffffffffff");
    assert_eq!(i64::from_bytes(&unhex("ffffffffffffffff")).unwrap(), -1);
    assert_eq!(
        i64::from_bytes(&unhex("8000000000000000")).unwrap(),
        i64::MIN
    );
}

#[test]
fn bool_is_t_or_f_and_rejects_other_bytes() {
    assert_eq!(enc(&true), "54");
    assert_eq!(enc(&false), "46");
    assert_eq!(
        bool::from_bytes(b"X"),
        Err(DecodeError::BadTag("Bool", b'X'))
    );
}

#[test]
fn short_string_has_a_one_byte_length() {
    assert_eq!(enc(&vec![1u8, 2, 3]), "03010203");
    assert_eq!(
        Vec::<u8>::from_bytes(&unhex("03010203")).unwrap(),
        [1, 2, 3]
    );
    assert_eq!(enc(&"hi".to_owned()), "026869");
    assert_eq!(String::from_bytes(&unhex("02ffff")), Err(DecodeError::Utf8));
}

#[test]
fn short_string_over_255_bytes_is_an_error_not_a_wrapped_length() {
    assert_eq!(
        vec![0u8; 256].to_bytes(),
        Err(EncodeError::TooLong {
            what: "ByteString",
            len: 256,
            max: 255
        })
    );
    assert_eq!(vec![0u8; 255].to_bytes().unwrap().len(), 256);
}

#[test]
fn large_has_a_two_byte_length_and_tail_has_none() {
    let big = Large(vec![7; 300]);
    let bytes = big.to_bytes().unwrap();
    assert_eq!(&bytes[..2], [0x01, 0x2c]);
    assert_eq!(Large::from_bytes(&bytes).unwrap(), big);
    assert_eq!(enc(&Tail(vec![9, 8])), "0908");
    assert_eq!(Tail::from_bytes(&[9, 8, 7]).unwrap(), Tail(vec![9, 8, 7]));
}

#[test]
fn maybe_is_0_or_1_then_the_value() {
    assert_eq!(enc(&None::<u16>), "30");
    assert_eq!(enc(&Some(5u16)), "310005");
    assert_eq!(
        Option::<u16>::from_bytes(&unhex("310005")).unwrap(),
        Some(5)
    );
    assert_eq!(
        Option::<u16>::from_bytes(b"2"),
        Err(DecodeError::BadTag("Maybe", b'2'))
    );
}

#[test]
fn list_has_a_one_byte_count_and_non_empty_rejects_zero() {
    assert_eq!(enc(&List(vec![1u16, 2])), "020001".to_owned() + "0002");
    assert_eq!(
        List::<u16>::from_bytes(&unhex("020001")).unwrap_err(),
        DecodeError::Eof
    );
    assert_eq!(List::<u8>::from_bytes(&[0]).unwrap(), List(vec![]));
    assert_eq!(
        NonEmpty::<u8>::from_bytes(&[0]),
        Err(DecodeError::EmptyList)
    );
    assert_eq!(
        NonEmpty::<u8>::from_bytes(&[2, 5, 6]).unwrap().as_slice(),
        [5, 6]
    );
    assert_eq!(NonEmpty::<u8>::new(vec![]), None);
    assert_eq!(
        List(vec![0u8; 256]).to_bytes(),
        Err(EncodeError::TooLong {
            what: "list",
            len: 256,
            max: 255
        })
    );
}

#[test]
fn tuples_concatenate_their_fields() {
    let v = (1u16, true, vec![9u8], Some(2u8));
    assert_eq!(enc(&v), "0001".to_owned() + "54" + "0109" + "3102");
    let back = <(u16, bool, Vec<u8>, Option<u8>)>::from_bytes(&v.to_bytes().unwrap()).unwrap();
    assert_eq!(back, v);
}

#[test]
fn decode_of_whole_input_rejects_trailing_bytes() {
    assert_eq!(u16::from_bytes(&[0, 1, 2]), Err(DecodeError::Trailing(1)));
    assert_eq!(u16::from_bytes(&[0]), Err(DecodeError::Eof));
}

#[test]
fn decoder_tracks_position_and_take_all() {
    let mut d = Decoder::new(&[1, 2, 3, 4]);
    assert_eq!(d.byte().unwrap(), 1);
    assert_eq!(d.offset(), 1);
    assert_eq!(d.take_all(), [2, 3, 4]);
    assert_eq!(d.remaining(), 0);
    assert_eq!(d.take(1), Err(DecodeError::Eof));
}

#[test]
fn system_time_is_its_seconds_as_int64() {
    assert_eq!(enc(&SysTime(0x0102030405)), "0000000102030405");
    assert_eq!(
        SysTime::from_bytes(&unhex("0000000102030405")).unwrap(),
        SysTime(0x0102030405)
    );
}

#[test]
fn version_is_word16_and_range_must_be_ordered() {
    assert_eq!(enc(&Version(7)), "0007");
    let r = VersionRange::new(Version(1), Version(5)).unwrap();
    assert_eq!(enc(&r), "00010005");
    assert_eq!(VersionRange::from_bytes(&unhex("00010005")).unwrap(), r);
    assert_eq!(
        VersionRange::from_bytes(&unhex("00050001")),
        Err(DecodeError::Invalid("version range"))
    );
    assert_eq!(VersionRange::new(Version(2), Version(1)), None);
}

#[test]
fn version_ranges_intersect_as_compatible_vrange() {
    let r = |a, b| VersionRange::new(Version(a), Version(b)).unwrap();
    assert_eq!(r(1, 5).intersect(&r(3, 9)), Some(r(3, 5)));
    assert_eq!(r(1, 2).intersect(&r(3, 9)), None);
    assert_eq!(r(1, 5).highest_common(&r(3, 9)), Some(Version(5)));
    assert_eq!(r(1, 5).capped(Version(3)), Some(r(1, 3)));
    assert_eq!(r(4, 5).capped(Version(3)), None);
    assert!(r(1, 5).contains(Version(5)) && !r(1, 5).contains(Version(6)));
}

#[test]
fn base64url_pads_like_haskell_and_decodes_both_forms() {
    assert_eq!(b64_encode(b"f", true), "Zg==");
    assert_eq!(b64_encode(b"fo", true), "Zm8=");
    assert_eq!(b64_encode(b"foo", true), "Zm9v");
    assert_eq!(b64_encode(b"f", false), "Zg");
    assert_eq!(b64_encode(&[0xfb, 0xff, 0xfe], true), "-__-");
    assert_eq!(b64_decode("Zg==").unwrap(), b"f");
    assert_eq!(b64_decode("Zg").unwrap(), b"f");
    assert_eq!(b64_decode("+//+").unwrap(), [0xfb, 0xff, 0xfe]);
    assert!(b64_decode("Z").is_err());
    assert!(b64_decode("Zg!=").is_err());
}

#[test]
fn base64url_round_trips_every_length() {
    for n in 0..70u8 {
        let data: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37)).collect();
        for padded in [true, false] {
            assert_eq!(
                b64_decode(&b64_encode(&data, padded)).unwrap(),
                data,
                "len {n}"
            );
        }
    }
}

#[test]
fn base64url_matches_nook_for_a_long_value() {
    let ca = b64_decode("hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg").unwrap();
    assert_eq!(ca.len(), 32);
    assert_eq!(
        b64_encode(&ca, false),
        "hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg"
    );
    assert_eq!(
        b64_encode(&ca, true),
        "hpq7_4gGJiilmz5Rf-CswuU5kZGkm_zOIooSw6yALRg="
    );
}

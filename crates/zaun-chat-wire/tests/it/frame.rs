use std::path::PathBuf;

use zaun_chat_wire::{Hello, ReqLine, Request, WireError, decode_line, encode_line, features_with};

#[test]
fn hello_line_has_the_golden_shape() {
    let hello = Hello::new(
        PathBuf::from("/data/chat"),
        "k3y".into(),
        "Ada".into(),
        vec!["smp://relay".into()],
    );
    assert_eq!(
        encode_line(&hello).unwrap(),
        r#"{"v":2,"dir":"/data/chat","key":"k3y","name":"Ada","relays":["smp://relay"]}"#
    );
}

#[test]
fn request_line_is_wrapped_in_req() {
    let line = encode_line(&ReqLine {
        req: Request::Accept(5),
    })
    .unwrap();
    assert_eq!(line, r#"{"req":{"t":"accept","c":5}}"#);
}

#[test]
fn unit_request_has_no_content() {
    let line = encode_line(&ReqLine {
        req: Request::Refresh,
    })
    .unwrap();
    assert_eq!(line, r#"{"req":{"t":"refresh"}}"#);
}

#[test]
fn unknown_fields_are_ignored() {
    let hello: Hello =
        decode_line(r#"{"v":2,"dir":"/d","key":"k","name":"n","relays":[],"later":true}"#).unwrap();
    assert_eq!(hello.v, 2);
    let line = r#"{"req":{"t":"send","c":{"chat":{"t":"direct","c":1},"text":"x","reply_to":null,"mentions":[],"new":1}},"extra":0}"#;
    let back: ReqLine = decode_line(line).unwrap();
    assert!(matches!(back.req, Request::Send { .. }));
}

#[test]
fn unknown_variant_is_an_error_not_a_panic() {
    let r: Result<ReqLine, _> = decode_line(r#"{"req":{"t":"teleport","c":1}}"#);
    assert!(matches!(r, Err(WireError::Malformed(_))));
}

#[test]
fn garbage_and_empty_lines_are_errors() {
    assert!(decode_line::<ReqLine>("not json").is_err());
    assert!(decode_line::<ReqLine>("").is_err());
}

#[test]
fn a_trailing_newline_is_accepted() {
    let back: ReqLine = decode_line("{\"req\":{\"t\":\"refresh\"}}\n").unwrap();
    assert_eq!(back.req, Request::Refresh);
}

#[test]
fn native_features_match_what_the_swift_engine_offers_today() {
    let f = features_with(&[]);
    assert!(f.files && f.gifs && f.reactions && f.replies && f.edit && f.mute);
    assert!(f.block && f.verify && f.mentions && f.calls && f.share);
    assert!(!f.presence && !f.voice && !f.group_voice && !f.link_previews);
}

#[test]
fn own_relays_without_an_own_file_server_carry_no_files() {
    let relay = "smp://k@relay.test".to_owned();
    let files = "xftp://k@files.test".to_owned();
    assert!(!features_with(std::slice::from_ref(&relay)).files);
    assert!(!features_with(std::slice::from_ref(&relay)).gifs);
    assert!(features_with(&[relay, files.clone()]).files);
    assert!(
        features_with(&[files]).files,
        "an own file server with the preset relays"
    );
}

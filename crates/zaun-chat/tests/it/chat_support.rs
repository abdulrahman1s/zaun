//! Shared fixtures for the chat message tests (tests/ProtocolTests.hs test data).
use serde_json::Value;
use zaun_chat::chat::ChatEvent;
use zaun_chat::chat::ChatMessage;
use zaun_chat::chat::ids::SharedMsgId;
use zaun_chat::chat::parse_chat_messages;
use zaun_chat::chat::preferences::{AllowPref, FeatureAllowed, Preferences};
use zaun_chat::chat::profile::Profile;
use zaun_chat::chat::version::CHAT_INITIAL_VRANGE;

pub const IMG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgAAAAIAQMAAAD+wSzIAAAABlBMVEX///+/v7+jQ3Y5AAAADklEQVQI12P4AIX8EAgALgAD/aNpbtEAAAAASUVORK5CYII=";

pub const CONN_REQ: &str = "simplex:/invitation#/?v=1&smp=smp%3A%2F%2F1234-w%3D%3D%40smp.simplex.im%3A5223%2F3456-w%3D%3D%23%2F%3Fv%3D1-4%26dh%3DMCowBQYDK2VuAyEAjiswwI3O_NlS8Fk3HJUW870EY2bAwmttMBsvRB9eV3o%253D&e2e=v%3D3%26x3dh%3DMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D%2CMEIwBQYDK2VvAzkAmKuSYeQ_m0SixPDS8Wq8VBaTS1cW-Lp0n0h4Diu-kUpR-qXx4SDJ32YGEFoGFGSbGPry5Ychr6U%3D";

pub fn id(bytes: &[u8]) -> SharedMsgId {
    SharedMsgId(bytes.to_vec())
}

pub fn allow(a: FeatureAllowed) -> Option<AllowPref> {
    Some(AllowPref { allow: a })
}

pub fn test_prefs() -> Preferences {
    Preferences {
        voice: allow(FeatureAllowed::Yes),
        reactions: allow(FeatureAllowed::Yes),
        ..Preferences::default()
    }
}

/// `testProfile`: alice with an image and voice and reactions allowed.
pub fn test_profile() -> Profile {
    Profile {
        display_name: "alice".into(),
        full_name: "Alice".into(),
        short_descr: None,
        description: None,
        image: Some(zaun_chat::chat::image::ImageData(IMG.into())),
        contact_link: None,
        preferences: Some(test_prefs()),
        peer_type: None,
        badge: None,
        contact_domain: None,
    }
}

pub fn test_profile_json() -> String {
    format!(
        r#"{{"fullName":"Alice","displayName":"alice","image":"{IMG}","preferences":{{"reactions":{{"allow":"yes"}},"voice":{{"allow":"yes"}}}}}}"#
    )
}

/// `ChatMessage chatInitialVRange Nothing event`.
pub fn msg(event: ChatEvent) -> ChatMessage {
    ChatMessage {
        v: Some(CHAT_INITIAL_VRANGE),
        msg_id: None,
        event,
    }
}

pub fn msg_with_id(msg_id: &[u8], event: ChatEvent) -> ChatMessage {
    ChatMessage {
        v: Some(CHAT_INITIAL_VRANGE),
        msg_id: Some(id(msg_id)),
        event,
    }
}

fn json(s: &str) -> Value {
    serde_json::from_str(s).unwrap()
}

/// `==##`: parsing the string gives exactly this message.
pub fn parses_to(s: &str, expected: &ChatMessage) {
    let got = parse_chat_messages(s.as_bytes()).unwrap();
    assert_eq!(got, vec![expected.clone()], "parsing {s}");
}

/// `##==`: encoding the message gives the same JSON value as the string.
pub fn encodes_to(s: &str, m: &ChatMessage) {
    let encoded = m.to_json().unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&encoded).unwrap(),
        json(s),
        "encoding for {s}"
    );
}

/// `##==##`.
pub fn both(s: &str, m: &ChatMessage) {
    encodes_to(s, m);
    parses_to(s, m);
}

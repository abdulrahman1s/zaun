//! The session strings of a call (call.ts, Call.hs `WebRTCSession`): `{type, sdp}` and a candidate list, as JSON text.
//!
//! The apps compress that text with LZ-String (base64); plain JSON is still read (streams, and Zaun's earlier form).

use serde_json::{Value, json};

use crate::chat::call::WebRtcSession;

fn pack(value: &Value) -> String {
    lz_str::compress_to_base64(&value.to_string())
}

fn unpack(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }
    let units = lz_str::decompress_from_base64(text)?;
    serde_json::from_str(&String::from_utf16(&units).ok()?).ok()
}

/// A session for an offer or answer: the candidates are in the SDP and, for the apps, listed too.
#[must_use]
pub(super) fn session(sdp: &str, kind: &str) -> WebRtcSession {
    let mid = sdp
        .lines()
        .find_map(|l| l.trim().strip_prefix("a=mid:"))
        .unwrap_or("0");
    let list: Vec<Value> = sdp
        .lines()
        .filter_map(|l| l.trim().strip_prefix("a="))
        .filter(|c| c.starts_with("candidate:"))
        .map(|c| json!({ "candidate": c, "sdpMid": mid, "sdpMLineIndex": 0 }))
        .collect();
    WebRtcSession {
        rtc_session: pack(&json!({ "type": kind, "sdp": sdp })),
        rtc_ice_candidates: pack(&Value::Array(list)),
    }
}

/// A stream's session: plain SDP, nothing listed.
#[must_use]
pub(super) fn plain_session(sdp: &str) -> WebRtcSession {
    WebRtcSession {
        rtc_session: sdp.to_owned(),
        rtc_ice_candidates: "[]".to_owned(),
    }
}

/// `rtcIceCandidates` of an `x.call.extra`.
#[must_use]
pub(super) fn extra_candidates(candidates: &[String]) -> String {
    let list = candidates
        .iter()
        .map(|c| json!({ "candidate": c, "sdpMLineIndex": 0 }))
        .collect();
    pack(&Value::Array(list))
}

/// The SDP of a session: plain, or `{type, sdp}`.
#[must_use]
pub(super) fn description(s: &WebRtcSession) -> Option<String> {
    let text = s.rtc_session.as_str();
    if text.starts_with("v=0") {
        return Some(text.to_owned());
    }
    let sdp = unpack(text)?.get("sdp")?.as_str()?.to_owned();
    sdp.starts_with("v=0").then_some(sdp)
}

/// Only plain SDP: a stream's.
#[must_use]
pub(super) fn plain_description(s: &WebRtcSession) -> Option<String> {
    s.rtc_session
        .starts_with("v=0")
        .then(|| s.rtc_session.clone())
}

/// `candidate:…` strings of a list of strings or of objects with a `candidate` field; empty
/// ones (the end of gathering) and anything else are left out.
#[must_use]
pub(super) fn candidates(text: &str) -> Vec<String> {
    let Some(Value::Array(list)) = unpack(text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|c| c.as_str().or_else(|| c.get("candidate")?.as_str()))
        .map(|c| c.strip_prefix("a=").unwrap_or(c))
        .filter(|c| c.starts_with("candidate:"))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SDP: &str = "v=0\r\no=- 1 2 IN IP4 0.0.0.0\r\ns=-\r\nt=0 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\na=candidate:1 1 udp 2130706431 10.0.0.2 4000 typ relay raddr 0.0.0.0 rport 0\r\n";

    #[test]
    fn what_is_written_reads_back_as_the_same_description_and_candidates() {
        let s = session(SDP, "offer");
        assert_eq!(description(&s).as_deref(), Some(SDP));
        assert_eq!(
            candidates(&s.rtc_ice_candidates),
            ["candidate:1 1 udp 2130706431 10.0.0.2 4000 typ relay raddr 0.0.0.0 rport 0"]
        );
        assert!(
            !s.rtc_session.starts_with("v=0"),
            "compressed, as the apps write it"
        );
    }

    #[test]
    fn the_apps_compressed_json_reads_with_the_type_and_mid_they_carry() {
        let offer = lz_str::compress_to_base64(&json!({ "type": "offer", "sdp": SDP }).to_string());
        let list = lz_str::compress_to_base64(
            &json!([
                { "candidate": "candidate:9 1 udp 41885439 203.0.113.5 3478 typ relay", "sdpMid": "0", "sdpMLineIndex": 0 },
                { "candidate": "", "sdpMid": "0", "sdpMLineIndex": 0 }
            ])
            .to_string(),
        );
        let s = WebRtcSession {
            rtc_session: offer,
            rtc_ice_candidates: list,
        };
        assert_eq!(description(&s).as_deref(), Some(SDP));
        assert_eq!(
            candidates(&s.rtc_ice_candidates),
            ["candidate:9 1 udp 41885439 203.0.113.5 3478 typ relay"]
        );
    }

    #[test]
    fn plain_sdp_and_a_plain_list_read_and_nonsense_does_not() {
        let s = WebRtcSession {
            rtc_session: SDP.into(),
            rtc_ice_candidates: "[\"candidate:1 1 udp 1 10.0.0.1 9 typ host\"]".into(),
        };
        assert_eq!(description(&s).as_deref(), Some(SDP));
        assert_eq!(candidates(&s.rtc_ice_candidates).len(), 1);
        let bad = WebRtcSession {
            rtc_session: "nonsense".into(),
            rtc_ice_candidates: "nonsense".into(),
        };
        assert_eq!(description(&bad), None);
        assert!(candidates(&bad.rtc_ice_candidates).is_empty());
        assert_eq!(plain_description(&plain_session(SDP)).as_deref(), Some(SDP));
        assert_eq!(plain_description(&session(SDP, "offer")), None);
    }

    #[test]
    fn empty_candidates_end_of_gathering_are_left_out() {
        let list = r#"[{"candidate":""},{"candidate":"candidate:9 1 udp 1 1.2.3.4 5 typ relay"}]"#;
        assert_eq!(
            candidates(list),
            ["candidate:9 1 udp 1 1.2.3.4 5 typ relay"]
        );
    }
}

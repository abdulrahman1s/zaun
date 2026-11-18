//! The session strings of the apps' calls (`packages/simplex-chat-webrtc/src/call.ts`,
//! ADR-0035 §3): a description is LZ-String-compressed (base64) JSON `{type, sdp}`, and the
//! candidates are the same of a JSON list of `RTCIceCandidateInit`. Plain SDP and a plain
//! JSON list (EasyPlay's earlier form, and game streams) are still read.
use serde_json::Value;

/// The apps' text for `value`: its JSON, compressed.
fn pack(value: &Value) -> String {
    lz_str::compress_to_base64(&value.to_string())
}

/// JSON from the apps' compressed text, or from plain JSON text.
fn unpack(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }
    let units = lz_str::decompress_from_base64(text)?;
    serde_json::from_str(&String::from_utf16(&units).ok()?).ok()
}

/// A `WebRTCSession` for the engine: `kind` is `"offer"` or `"answer"`. The candidates are
/// in the SDP and, for the apps, in the list as well (on the description's first media).
pub(super) fn session(sdp: &str, kind: &str) -> Value {
    let mid = sdp
        .lines()
        .find_map(|l| l.trim().strip_prefix("a=mid:"))
        .unwrap_or("0");
    let list: Vec<Value> = sdp
        .lines()
        .filter_map(|l| l.trim().strip_prefix("a="))
        .filter(|c| c.starts_with("candidate:"))
        .map(|c| serde_json::json!({ "candidate": c, "sdpMid": mid, "sdpMLineIndex": 0 }))
        .collect();
    serde_json::json!({
        "rtcSession": pack(&serde_json::json!({ "type": kind, "sdp": sdp })),
        "rtcIceCandidates": pack(&Value::Array(list)),
    })
}

/// `rtcIceCandidates` for `/_call extra`: candidates found after the description.
pub(super) fn extra_candidates(candidates: &[String]) -> String {
    let list = candidates
        .iter()
        .map(|c| serde_json::json!({ "candidate": c, "sdpMLineIndex": 0 }))
        .collect();
    pack(&Value::Array(list))
}

/// The SDP of a `WebRTCSession`'s `rtcSession`: plain, or the apps' compressed `{type, sdp}`.
pub(super) fn description(s: Option<&Value>) -> Option<String> {
    let text = s?.get("rtcSession")?.as_str()?;
    if text.starts_with("v=0") {
        return Some(text.to_owned());
    }
    let sdp = unpack(text)?.get("sdp")?.as_str()?.to_owned();
    sdp.starts_with("v=0").then_some(sdp)
}

/// `rtcIceCandidates`: a list (plain or compressed JSON text) of `candidate:…` strings or of
/// objects with a `candidate` field. Empty ones (the end of gathering) and anything else are
/// left out.
pub(super) fn candidates(s: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(list)) = s
        .and_then(|s| s.get("rtcIceCandidates"))
        .and_then(Value::as_str)
        .and_then(unpack)
    else {
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
        assert_eq!(description(Some(&s)).as_deref(), Some(SDP));
        assert_eq!(
            candidates(Some(&s)),
            ["candidate:1 1 udp 2130706431 10.0.0.2 4000 typ relay raddr 0.0.0.0 rport 0"]
        );
        let text = s["rtcSession"].as_str().unwrap_or_default();
        assert!(!text.starts_with("v=0"), "compressed, as the apps write it");
    }

    #[test]
    fn the_apps_compressed_json_reads_with_the_type_and_mid_they_carry() {
        let offer = lz_str::compress_to_base64(
            &serde_json::json!({ "type": "offer", "sdp": SDP }).to_string(),
        );
        let list = lz_str::compress_to_base64(
            &serde_json::json!([
                { "candidate": "candidate:9 1 udp 41885439 203.0.113.5 3478 typ relay", "sdpMid": "0", "sdpMLineIndex": 0 },
                { "candidate": "", "sdpMid": "0", "sdpMLineIndex": 0 }
            ])
            .to_string(),
        );
        let s = serde_json::json!({ "rtcSession": offer, "rtcIceCandidates": list });
        assert_eq!(description(Some(&s)).as_deref(), Some(SDP));
        assert_eq!(
            candidates(Some(&s)),
            ["candidate:9 1 udp 41885439 203.0.113.5 3478 typ relay"]
        );
    }

    #[test]
    fn plain_sdp_and_a_plain_list_still_read_and_nonsense_does_not() {
        let s = serde_json::json!({ "rtcSession": SDP, "rtcIceCandidates": "[\"candidate:1 1 udp 1 10.0.0.1 9 typ host\"]" });
        assert_eq!(description(Some(&s)).as_deref(), Some(SDP));
        assert_eq!(candidates(Some(&s)).len(), 1);
        let bad = serde_json::json!({ "rtcSession": "nonsense", "rtcIceCandidates": "nonsense" });
        assert_eq!(description(Some(&bad)), None);
        assert!(candidates(Some(&bad)).is_empty());
    }
}

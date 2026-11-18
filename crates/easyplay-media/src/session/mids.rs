//! The apps name a call's media lines by number (`mediaSourceFromTransceiverMid` in `call.ts`:
//! "0" is the microphone) and play only what arrives on those, but str0m picks random mids.
//! So the offer a call writes is renamed "0", "1", … on the way out and the answer renamed
//! back on the way in (ADR-0035 §3). The RTP `sdes:mid` extension is left out of the offer:
//! its values would still be str0m's names, so both sides find streams by their SSRC lines.

const MID_EXTENSION: &str = "urn:ietf:params:rtp-hdrext:sdes:mid";

/// Our mids and the numbers they go out as, in the offer's order.
#[derive(Debug, Default)]
pub(super) struct Renames(Vec<(String, String)>);

impl Renames {
    /// `sdp` (our offer) with numbered mids and without the mid extension, and the renames.
    pub fn number(sdp: &str) -> (String, Self) {
        let mut names = Vec::new();
        for line in sdp.lines() {
            if let Some(mid) = line.trim().strip_prefix("a=mid:") {
                let number = names.len().to_string();
                names.push((mid.to_owned(), number));
            }
        }
        let renames = Self(names);
        let out = rewrite(sdp, |l| !l.contains(MID_EXTENSION), |m| renames.out(m));
        (out, renames)
    }

    /// The friend's answer with our own mids again.
    pub fn restore(&self, sdp: &str) -> String {
        rewrite(sdp, |_| true, |m| self.back(m))
    }

    fn out<'a>(&'a self, mid: &'a str) -> &'a str {
        self.0
            .iter()
            .find(|(ours, _)| ours == mid)
            .map_or(mid, |(_, n)| n)
    }

    fn back<'a>(&'a self, mid: &'a str) -> &'a str {
        self.0
            .iter()
            .find(|(_, n)| n == mid)
            .map_or(mid, |(ours, _)| ours)
    }
}

/// `sdp` with the lines `keep` refuses left out and every mid in `a=mid:` and
/// `a=group:BUNDLE` passed through `name`. Lines end in CRLF, as SDP's do.
fn rewrite<'a>(
    sdp: &'a str,
    keep: impl Fn(&str) -> bool,
    name: impl Fn(&'a str) -> &'a str,
) -> String {
    let mut out = String::with_capacity(sdp.len());
    for line in sdp.lines().filter(|l| keep(l)) {
        let line = line.trim_end();
        if let Some(mid) = line.strip_prefix("a=mid:") {
            out.push_str("a=mid:");
            out.push_str(name(mid));
        } else if let Some(group) = line.strip_prefix("a=group:BUNDLE") {
            out.push_str("a=group:BUNDLE");
            for mid in group.split_whitespace() {
                out.push(' ');
                out.push_str(name(mid));
            }
        } else {
            out.push_str(line);
        }
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const OFFER: &str = "v=0\r\na=group:BUNDLE xQz\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:xQz\r\na=extmap:3 urn:ietf:params:rtp-hdrext:sdes:mid\r\na=extmap:1 urn:ietf:params:rtp-hdrext:ssrc-audio-level\r\na=ssrc:42 cname:a\r\n";

    #[test]
    fn our_offer_goes_out_with_numbered_mids_and_without_the_mid_extension() {
        let (out, _) = Renames::number(OFFER);
        assert!(out.contains("a=group:BUNDLE 0\r\n"));
        assert!(out.contains("a=mid:0\r\n"));
        assert!(!out.contains("sdes:mid"));
        assert!(out.contains("ssrc-audio-level"), "other extensions stay");
        assert!(
            out.contains("a=ssrc:42 cname:a"),
            "streams are found by their SSRC"
        );
    }

    #[test]
    fn the_answer_comes_back_with_our_mids() {
        let (_, renames) = Renames::number(OFFER);
        let answer = "v=0\r\na=group:BUNDLE 0\r\nm=audio 9 UDP/TLS/RTP/SAVPF 111\r\na=mid:0\r\n";
        let back = renames.restore(answer);
        assert!(back.contains("a=group:BUNDLE xQz\r\n"));
        assert!(back.contains("a=mid:xQz\r\n"));
    }
}

//! `FileSize`: `13mb`, `1kb`, or plain bytes when not a multiple of 1024 (Description.hs `StrEncoding (FileSize a)`).

#[must_use]
pub fn encode_file_size(bytes: u64) -> String {
    let (ks, b) = (bytes / 1024, bytes % 1024);
    let (ms, k) = (ks / 1024, ks % 1024);
    let (gs, m) = (ms / 1024, ms % 1024);
    if b != 0 {
        bytes.to_string()
    } else if k != 0 {
        format!("{ks}kb")
    } else if m != 0 {
        format!("{ms}mb")
    } else {
        format!("{gs}gb")
    }
}

pub fn decode_file_size(s: &str) -> Result<u64, String> {
    let bad = || format!("bad file size: {s}");
    let (digits, unit) = [("gb", 1u64 << 30), ("mb", 1 << 20), ("kb", 1 << 10)]
        .into_iter()
        .find_map(|(suffix, unit)| s.strip_suffix(suffix).map(|d| (d, unit)))
        .unwrap_or((s, 1));
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    digits
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .ok_or_else(bad)
}

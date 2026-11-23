//! Printing a description the way Haskell's `Data.Yaml.encode` does: keys sorted, plain scalars, lists flush with their key.
use crate::encoding::b64_encode;

use super::file_size::encode_file_size;
use super::model::FileDescription;
use super::replicas::{encode_replica, group_by_server, unfold};

const SPECIAL: [&str; 22] = [
    "y", "Y", "yes", "Yes", "YES", "n", "N", "no", "No", "NO", "true", "True", "TRUE", "false",
    "False", "FALSE", "on", "On", "ON", "off", "Off", "OFF",
];

/// `Data.Yaml` single-quotes a string that would read back as a number, a boolean or null (from memory of its
/// `isSpecialString`; no real field is ever such a string).
fn scalar(s: &str) -> String {
    let numeric =
        s.parse::<f64>().is_ok() || s.starts_with(['+', '-']) && s[1..].parse::<f64>().is_ok();
    let special = SPECIAL.contains(&s) || matches!(s, "null" | "Null" | "NULL" | "~" | "");
    if numeric || special {
        format!("'{}'", s.replace('\'', "''"))
    } else {
        s.to_owned()
    }
}

#[must_use]
pub fn encode_description(fd: &FileDescription) -> String {
    let mut out = String::new();
    let mut line = |s: String| {
        out.push_str(&s);
        out.push('\n');
    };
    let b64 = |b: &[u8]| scalar(&b64_encode(b, true));
    line(format!(
        "chunkSize: {}",
        scalar(&encode_file_size(u64::from(fd.chunk_size)))
    ));
    line(format!("digest: {}", b64(&fd.digest)));
    line(format!("key: {}", b64(&fd.key)));
    line(format!("nonce: {}", b64(&fd.nonce)));
    line(format!("party: {}", fd.party.as_str()));
    if let Some(r) = &fd.redirect {
        line("redirect:".into());
        line(format!("  digest: {}", b64(&r.digest)));
        line(format!("  size: {}", r.size));
    }
    let groups = group_by_server(unfold(fd.chunk_size, &fd.chunks));
    if groups.is_empty() {
        line("replicas: []".into());
    } else {
        line("replicas:".into());
        for (server, replicas) in &groups {
            line("- chunks:".into());
            for r in replicas {
                line(format!("  - {}", scalar(&encode_replica(r))));
            }
            line(format!("  server: {}", scalar(&server.to_string())));
        }
    }
    line(format!("size: {}", scalar(&encode_file_size(fd.size))));
    out
}

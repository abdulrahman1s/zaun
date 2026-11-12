//! Lossless storage of paths in `TEXT` columns (DATA_MODEL §1).
//!
//! A path whose bytes are valid UTF-8 is stored as is. Any other path is stored as `b:`
//! followed by the bytes, valid UTF-8 runs kept as text, every other byte and every `\`
//! written as `\xNN`. Stored paths are absolute, so they start with `/` and the `b:` prefix is
//! unambiguous. The encoding does not preserve ordering.

use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

/// Why a path cannot be encoded or a column value decoded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PathCodecError {
    /// Only absolute paths are stored.
    #[error("path is not absolute")]
    NotAbsolute,
    /// A `b:` value with a malformed escape.
    #[error("malformed escaped path at byte {0}")]
    BadEscape(usize),
    /// A relative path that is empty or has `.`/`..` components (it would not round-trip and
    /// could name something outside the game folder).
    #[error("relative path is empty or not normal")]
    NotNormal,
}

const PREFIX: &str = "b:";

/// Encode an absolute path for a `TEXT` column.
pub fn encode(path: &Path) -> Result<String, PathCodecError> {
    if !path.is_absolute() {
        return Err(PathCodecError::NotAbsolute);
    }
    let bytes = path.as_os_str().as_bytes();
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Ok(s.to_owned());
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    out.push_str(PREFIX);
    for chunk in bytes.utf8_chunks() {
        for c in chunk.valid().chars() {
            if c == '\\' {
                out.push_str("\\x5c");
            } else {
                out.push(c);
            }
        }
        for b in chunk.invalid() {
            out.push_str(&format!("\\x{b:02x}"));
        }
    }
    Ok(out)
}

/// Decode a column value written by [`encode`].
pub fn decode(text: &str) -> Result<PathBuf, PathCodecError> {
    let path = match text.strip_prefix(PREFIX) {
        None => PathBuf::from(text),
        Some(rest) => {
            let src = rest.as_bytes();
            let mut out = Vec::with_capacity(src.len());
            let mut i = 0;
            while i < src.len() {
                if src[i] == b'\\' {
                    let hex = src
                        .get(i + 1..i + 4)
                        .filter(|h| h[0] == b'x')
                        .and_then(|h| std::str::from_utf8(&h[1..]).ok())
                        .and_then(|h| u8::from_str_radix(h, 16).ok())
                        .ok_or(PathCodecError::BadEscape(i + PREFIX.len()))?;
                    out.push(hex);
                    i += 4;
                } else {
                    out.push(src[i]);
                    i += 1;
                }
            }
            PathBuf::from(std::ffi::OsString::from_vec(out))
        }
    };
    if !path.is_absolute() {
        return Err(PathCodecError::NotAbsolute);
    }
    Ok(path)
}

/// Encode an optional path.
pub fn encode_opt(path: Option<&Path>) -> Result<Option<String>, PathCodecError> {
    path.map(encode).transpose()
}

/// Decode an optional column value.
pub fn decode_opt(text: Option<&str>) -> Result<Option<PathBuf>, PathCodecError> {
    text.map(decode).transpose()
}

/// Encode a relative path (`executables.rel_path`) with the same escaping; the stored form
/// is prefixed `./` (or `b:./`) so that [`decode`]'s absolute-path rule is replaced by a
/// relative-path rule in [`decode_relative`].
pub fn encode_relative(path: &Path) -> Result<String, PathCodecError> {
    if path.is_absolute() {
        return Err(PathCodecError::NotAbsolute);
    }
    // `components()` also drops a trailing `/.` and collapses `//`, so the components must
    // rebuild the exact original bytes, or the stored value would not round-trip.
    let normal = path
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    let exact = path.components().collect::<PathBuf>().as_os_str() == path.as_os_str();
    if path.as_os_str().is_empty() || !normal || !exact {
        return Err(PathCodecError::NotNormal);
    }
    let abs = Path::new("/").join(path);
    let enc = encode(&abs)?;
    Ok(match enc.strip_prefix(PREFIX) {
        Some(rest) => format!("{PREFIX}.{rest}"),
        None => format!(".{enc}"),
    })
}

/// Decode a value written by [`encode_relative`].
pub fn decode_relative(text: &str) -> Result<PathBuf, PathCodecError> {
    let (prefix, rest) = match text.strip_prefix(PREFIX) {
        Some(rest) => (PREFIX, rest),
        None => ("", text),
    };
    let rest = rest.strip_prefix("./").ok_or(PathCodecError::NotAbsolute)?;
    let abs = decode(&format!("{prefix}/{rest}"))?;
    let rel = abs
        .strip_prefix("/")
        .map_err(|_| PathCodecError::NotAbsolute)?;
    Ok(rel.to_path_buf())
}

/// Bytes of a path (helper for tests and callers comparing raw paths).
pub fn as_bytes(path: &Path) -> &[u8] {
    OsStr::as_bytes(path.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn utf8_paths_are_stored_verbatim() {
        assert_eq!(
            encode(Path::new("/home/sam/Games/x.exe")).unwrap(),
            "/home/sam/Games/x.exe"
        );
        assert_eq!(encode(Path::new("/a\\b")).unwrap(), "/a\\b");
        assert_eq!(decode("/a\\b").unwrap(), PathBuf::from("/a\\b"));
    }

    #[test]
    fn non_utf8_paths_are_escaped() {
        let p = PathBuf::from(std::ffi::OsString::from_vec(b"/g/\xffname\\x".to_vec()));
        let enc = encode(&p).unwrap();
        assert_eq!(enc, "b:/g/\\xffname\\x5cx");
        assert_eq!(decode(&enc).unwrap(), p);
    }

    #[test]
    fn relative_and_malformed() {
        assert_eq!(encode(Path::new("rel")), Err(PathCodecError::NotAbsolute));
        assert_eq!(decode("rel"), Err(PathCodecError::NotAbsolute));
        assert!(matches!(
            decode("b:/a\\x"),
            Err(PathCodecError::BadEscape(_))
        ));
        assert!(matches!(
            decode("b:/a\\zz1"),
            Err(PathCodecError::BadEscape(_))
        ));
        let rel = Path::new("bin/game.exe");
        let enc = encode_relative(rel).unwrap();
        assert_eq!(enc, "./bin/game.exe");
        assert_eq!(decode_relative(&enc).unwrap(), rel);
        assert_eq!(
            encode_relative(Path::new("/abs")),
            Err(PathCodecError::NotAbsolute)
        );
        for bad in [
            "",
            ".",
            "..",
            "bin/../game.exe",
            "./game.exe",
            "bin/.",
            "bin//game.exe",
            "bin/",
        ] {
            assert_eq!(
                encode_relative(Path::new(bad)),
                Err(PathCodecError::NotNormal),
                "{bad:?}"
            );
        }
    }

    proptest! {
        #[test]
        fn arbitrary_byte_paths_round_trip(tail in proptest::collection::vec(any::<u8>(), 0..64)) {
            let mut bytes = b"/".to_vec();
            bytes.extend(tail.into_iter().filter(|b| *b != 0));
            let p = PathBuf::from(std::ffi::OsString::from_vec(bytes));
            let enc = encode(&p).unwrap();
            let back = decode(&enc).unwrap();
            prop_assert_eq!(as_bytes(&back), as_bytes(&p));
        }

        #[test]
        fn arbitrary_relative_paths_round_trip(tail in proptest::collection::vec(1u8..=255, 1..32)) {
            let bytes: Vec<u8> = tail.into_iter().filter(|b| *b != b'/').collect();
            prop_assume!(!bytes.is_empty() && bytes != b"." && bytes != b"..");
            let p = PathBuf::from(std::ffi::OsString::from_vec(bytes));
            let back = decode_relative(&encode_relative(&p).unwrap()).unwrap();
            prop_assert_eq!(as_bytes(&back), as_bytes(&p));
        }
    }
}

//! Minimal reader for Valve's text KeyValues format (`toolmanifest.vdf`,
//! `compatibilitytool.vdf`): nested quoted keys and values, unquoted tokens, `//` comments,
//! backslash escapes, `#include`/`#base` lines ignored, `[$CONDITION]` suffixes skipped.
//! Inputs are capped at 64 KiB and nesting at depth 8 (RUNTIME_MANAGEMENT §5).

/// Largest accepted document.
pub const VDF_MAX_BYTES: usize = 64 * 1024;
/// Deepest accepted nesting (the root object is depth 0).
pub const VDF_MAX_DEPTH: usize = 8;

/// A value: a string or a nested object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VdfValue {
    /// A string.
    Str(String),
    /// A nested object.
    Obj(VdfObject),
}

/// An ordered list of key/value pairs (keys may repeat; lookups are case-insensitive, like
/// Steam's).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VdfObject(pub Vec<(String, VdfValue)>);

impl VdfObject {
    /// The first value under `key`.
    pub fn get(&self, key: &str) -> Option<&VdfValue> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// The first string under `key`.
    pub fn get_str(&self, key: &str) -> Option<&str> {
        match self.get(key)? {
            VdfValue::Str(s) => Some(s),
            VdfValue::Obj(_) => None,
        }
    }

    /// The first object under `key`.
    pub fn get_obj(&self, key: &str) -> Option<&VdfObject> {
        match self.get(key)? {
            VdfValue::Obj(o) => Some(o),
            VdfValue::Str(_) => None,
        }
    }

    /// Every nested object, in order.
    pub fn objects(&self) -> impl Iterator<Item = (&str, &VdfObject)> {
        self.0.iter().filter_map(|(k, v)| match v {
            VdfValue::Obj(o) => Some((k.as_str(), o)),
            VdfValue::Str(_) => None,
        })
    }
}

/// Why a document was rejected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum VdfError {
    /// Larger than [`VDF_MAX_BYTES`].
    #[error("document larger than {VDF_MAX_BYTES} bytes")]
    TooLarge,
    /// Not UTF-8.
    #[error("document is not valid UTF-8")]
    InvalidUtf8,
    /// Nested deeper than [`VDF_MAX_DEPTH`].
    #[error("nesting deeper than {VDF_MAX_DEPTH} at line {line}")]
    TooDeep {
        /// Line number (1-based).
        line: usize,
    },
    /// A quoted string without its closing quote.
    #[error("unterminated string at line {line}")]
    UnterminatedString {
        /// Line number.
        line: usize,
    },
    /// A token where it does not belong (a `}` without `{`, a key without a value, …).
    #[error("unexpected {found} at line {line}")]
    Unexpected {
        /// What was found.
        found: &'static str,
        /// Line number.
        line: usize,
    },
    /// The document ended inside an object.
    #[error("unexpected end of document")]
    UnexpectedEof,
}

#[derive(Debug, PartialEq, Eq)]
enum Tok {
    Str(String),
    Open,
    Close,
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    line: usize,
}

impl Lexer<'_> {
    fn next(&mut self) -> Result<Option<Tok>, VdfError> {
        loop {
            let Some(&c) = self.chars.peek() else {
                return Ok(None);
            };
            match c {
                '\n' => {
                    self.line += 1;
                    self.chars.next();
                }
                c if c.is_whitespace() => {
                    self.chars.next();
                }
                '/' => {
                    self.chars.next();
                    if self.chars.peek() == Some(&'/') {
                        self.skip_line();
                    } else {
                        return Err(VdfError::Unexpected {
                            found: "'/'",
                            line: self.line,
                        });
                    }
                }
                '#' => self.skip_line(),
                '[' => {
                    // Platform conditional such as [$WIN32]: ignored.
                    for c in self.chars.by_ref() {
                        if c == ']' {
                            break;
                        }
                        if c == '\n' {
                            return Err(VdfError::Unexpected {
                                found: "'['",
                                line: self.line,
                            });
                        }
                    }
                }
                '{' => {
                    self.chars.next();
                    return Ok(Some(Tok::Open));
                }
                '}' => {
                    self.chars.next();
                    return Ok(Some(Tok::Close));
                }
                '"' => {
                    self.chars.next();
                    return self.quoted().map(|s| Some(Tok::Str(s)));
                }
                _ => {
                    let mut s = String::new();
                    while let Some(&c) = self.chars.peek() {
                        if c.is_whitespace() || matches!(c, '{' | '}' | '"') {
                            break;
                        }
                        s.push(c);
                        self.chars.next();
                    }
                    return Ok(Some(Tok::Str(s)));
                }
            }
        }
    }

    fn skip_line(&mut self) {
        for c in self.chars.by_ref() {
            if c == '\n' {
                self.line += 1;
                break;
            }
        }
    }

    fn quoted(&mut self) -> Result<String, VdfError> {
        let start = self.line;
        let mut s = String::new();
        loop {
            match self.chars.next() {
                None => return Err(VdfError::UnterminatedString { line: start }),
                Some('"') => return Ok(s),
                Some('\\') => match self.chars.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('\\') => s.push('\\'),
                    Some('"') => s.push('"'),
                    Some(other) => {
                        s.push('\\');
                        s.push(other);
                    }
                    None => return Err(VdfError::UnterminatedString { line: start }),
                },
                Some('\n') => {
                    self.line += 1;
                    s.push('\n');
                }
                Some(c) => s.push(c),
            }
        }
    }
}

fn parse_object(lx: &mut Lexer<'_>, depth: usize, root: bool) -> Result<VdfObject, VdfError> {
    let mut obj = VdfObject::default();
    loop {
        let key = match lx.next()? {
            None if root => return Ok(obj),
            None => return Err(VdfError::UnexpectedEof),
            Some(Tok::Close) if !root => return Ok(obj),
            Some(Tok::Close) => {
                return Err(VdfError::Unexpected {
                    found: "'}'",
                    line: lx.line,
                });
            }
            Some(Tok::Open) => {
                return Err(VdfError::Unexpected {
                    found: "'{'",
                    line: lx.line,
                });
            }
            Some(Tok::Str(k)) => k,
        };
        let value = match lx.next()? {
            None => return Err(VdfError::UnexpectedEof),
            Some(Tok::Str(v)) => VdfValue::Str(v),
            Some(Tok::Open) => {
                if depth + 1 > VDF_MAX_DEPTH {
                    return Err(VdfError::TooDeep { line: lx.line });
                }
                VdfValue::Obj(parse_object(lx, depth + 1, false)?)
            }
            Some(Tok::Close) => {
                return Err(VdfError::Unexpected {
                    found: "'}' after a key",
                    line: lx.line,
                });
            }
        };
        obj.0.push((key, value));
    }
}

/// Parse a document from bytes.
pub fn parse_bytes(bytes: &[u8]) -> Result<VdfObject, VdfError> {
    if bytes.len() > VDF_MAX_BYTES {
        return Err(VdfError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| VdfError::InvalidUtf8)?;
    parse(text)
}

/// Parse a document.
pub fn parse(text: &str) -> Result<VdfObject, VdfError> {
    if text.len() > VDF_MAX_BYTES {
        return Err(VdfError::TooLarge);
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut lx = Lexer {
        chars: text.chars().peekable(),
        line: 1,
    };
    parse_object(&mut lx, 0, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOLMANIFEST_UMU: &str =
        include_str!("../../tests/data/vdf/UMU-Proton-10.0-4.toolmanifest.vdf");
    const COMPAT_UMU: &str =
        include_str!("../../tests/data/vdf/UMU-Proton-10.0-4.compatibilitytool.vdf");
    const SLR: &str = include_str!("../../tests/data/vdf/steamrt3.toolmanifest.vdf");
    const CACHYOS: &str = include_str!("../../tests/data/vdf/proton-cachyos.toolmanifest.vdf");

    #[test]
    fn corpus() {
        let m = parse(TOOLMANIFEST_UMU).unwrap();
        let m = m.get_obj("manifest").unwrap();
        assert_eq!(m.get_str("commandline"), Some("/proton %verb%"));
        assert_eq!(m.get_str("require_tool_appid"), Some("1628350"));
        assert_eq!(m.get_str("COMPATMANAGER_LAYER_NAME"), Some("proton"));

        let c = parse(COMPAT_UMU).unwrap();
        let tools = c
            .get_obj("compatibilitytools")
            .unwrap()
            .get_obj("compat_tools")
            .unwrap();
        let (name, tool) = tools.objects().next().unwrap();
        assert_eq!(name, "UMU-Proton-10.0-4-proton");
        assert_eq!(tool.get_str("display_name"), Some("UMU-Proton-10.0-4"));

        let s = parse(SLR).unwrap();
        assert_eq!(
            s.get_obj("manifest")
                .unwrap()
                .get_str("compatmanager_layer_name"),
            Some("container-runtime")
        );
        let c = parse(CACHYOS).unwrap();
        assert_eq!(
            c.get_obj("manifest").unwrap().get_str("require_tool_appid"),
            Some("4183110")
        );
    }

    #[test]
    fn escapes_unquoted_and_conditionals() {
        let o =
            parse("a \"x\\\"y\\\\z\\n\" b plain \"c\" \"v\" [$WIN32] \"d\" { } #include x.vdf\n")
                .unwrap();
        assert_eq!(o.get_str("a"), Some("x\"y\\z\n"));
        assert_eq!(o.get_str("b"), Some("plain"));
        assert_eq!(o.get_str("c"), Some("v"));
        assert_eq!(o.get_obj("d"), Some(&VdfObject::default()));
    }

    #[test]
    fn malformed_inputs_fail_cleanly() {
        let cases: &[(&str, VdfError)] = &[
            ("\"a\" {", VdfError::UnexpectedEof),
            ("\"a\"", VdfError::UnexpectedEof),
            ("\"a\" \"b", VdfError::UnterminatedString { line: 1 }),
            (
                "}",
                VdfError::Unexpected {
                    found: "'}'",
                    line: 1,
                },
            ),
            (
                "{",
                VdfError::Unexpected {
                    found: "'{'",
                    line: 1,
                },
            ),
            (
                "\"a\" }",
                VdfError::Unexpected {
                    found: "'}' after a key",
                    line: 1,
                },
            ),
            (
                "a / b",
                VdfError::Unexpected {
                    found: "'/'",
                    line: 1,
                },
            ),
        ];
        for (text, err) in cases {
            assert_eq!(parse(text).as_ref(), Err(err), "{text:?}");
        }
        let deep = "a {".repeat(9) + &"}".repeat(9);
        assert!(matches!(parse(&deep), Err(VdfError::TooDeep { .. })));
        let ok = "a {".repeat(8) + &"}".repeat(8);
        assert!(parse(&ok).is_ok());
        assert_eq!(
            parse_bytes(&vec![b' '; VDF_MAX_BYTES + 1]),
            Err(VdfError::TooLarge)
        );
        assert_eq!(parse_bytes(&[0xff, 0xfe]), Err(VdfError::InvalidUtf8));
    }

    #[test]
    fn never_panics_on_prefixes() {
        // Fuzz-target stub: every prefix of the corpus parses or fails without panicking.
        for doc in [TOOLMANIFEST_UMU, COMPAT_UMU, SLR] {
            for i in 0..doc.len() {
                if doc.is_char_boundary(i) {
                    let _ = parse(&doc[..i]);
                }
            }
        }
    }
}

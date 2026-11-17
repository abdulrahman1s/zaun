//! A tolerant reader for Valve's text KeyValues format (`.vdf`, `.acf`) as Steam writes it
//! (ADR-0031 §2, THREAT_MODEL T-22).
//!
//! These files are written by another program, so the reader never fails and never panics:
//! a document that is cut off, has a stray brace or an unterminated string yields what could
//! be read, and [`Parsed::recovered`] counts the repairs. It is bounded in every dimension:
//!
//! - input: the caller reads at most [`KvLimits::max_bytes`] (the reader ignores the rest);
//! - nesting: objects deeper than [`KvLimits::max_depth`] are skipped whole, without
//!   recursion (the parser keeps its own stack, so no input can exhaust the thread's stack);
//! - entries: at most [`KvLimits::max_entries`] key/value pairs are kept in the whole
//!   document; later ones are read and dropped;
//! - tokens: each string is capped at [`KvLimits::max_token_bytes`] (the rest is skipped).
//!
//! Every loop consumes at least one input byte per iteration, so the work is linear in the
//! input size.
//!
//! The grammar: `key value` pairs where a value is a string or `{ … }`; strings are quoted
//! (`"…"` with `\n`, `\t`, `\\`, `\"` escapes) or bare (up to white space, a brace or a
//! quote); `//` comments; `#include`/`#base` lines are ignored; `[$CONDITION]` suffixes are
//! skipped. Bytes that are not UTF-8 are replaced (U+FFFD). Keys compare case-insensitively,
//! like Steam's.
//!
//! [`crate::runtime::vdf`] is the strict reader for Proton's own small manifests and stays
//! as it is.

/// Bounds of one parse.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KvLimits {
    /// Bytes of input considered.
    pub max_bytes: usize,
    /// Deepest nesting kept (the root object is depth 0).
    pub max_depth: usize,
    /// Key/value pairs kept in the whole document.
    pub max_entries: usize,
    /// Bytes kept of one key or value.
    pub max_token_bytes: usize,
}

impl KvLimits {
    /// Manifests, `libraryfolders.vdf`, `loginusers.vdf`: small files.
    pub const SMALL: KvLimits = KvLimits {
        max_bytes: 1024 * 1024,
        max_depth: 16,
        max_entries: 100_000,
        max_token_bytes: 64 * 1024,
    };
    /// `config.vdf`, `localconfig.vdf`: hundreds of kilobytes on a real machine, megabytes
    /// for very large libraries.
    pub const LARGE: KvLimits = KvLimits {
        max_bytes: 32 * 1024 * 1024,
        max_depth: 24,
        max_entries: 2_000_000,
        max_token_bytes: 256 * 1024,
    };
}

/// A value: a string or a nested object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KvValue {
    /// A string.
    Str(String),
    /// A nested object.
    Obj(KvObject),
}

/// An ordered list of key/value pairs; keys may repeat (lookups return the first).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KvObject(pub Vec<(String, KvValue)>);

impl KvObject {
    /// The first value under `key` (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&KvValue> {
        self.0
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    /// The first string under `key`.
    pub fn str(&self, key: &str) -> Option<&str> {
        self.0.iter().find_map(|(k, v)| match v {
            KvValue::Str(s) if k.eq_ignore_ascii_case(key) => Some(s.as_str()),
            _ => None,
        })
    }

    /// The first object under `key`.
    pub fn obj(&self, key: &str) -> Option<&KvObject> {
        self.0.iter().find_map(|(k, v)| match v {
            KvValue::Obj(o) if k.eq_ignore_ascii_case(key) => Some(o),
            _ => None,
        })
    }

    /// The object reached by following `path` (each step case-insensitive).
    pub fn path(&self, path: &[&str]) -> Option<&KvObject> {
        path.iter().try_fold(self, |o, k| o.obj(k))
    }

    /// The first string under `key` parsed as an unsigned number (surrounding white space
    /// allowed).
    pub fn u64(&self, key: &str) -> Option<u64> {
        self.str(key).and_then(|s| s.trim().parse().ok())
    }

    /// Every nested object, in order.
    pub fn objects(&self) -> impl Iterator<Item = (&str, &KvObject)> {
        self.0.iter().filter_map(|(k, v)| match v {
            KvValue::Obj(o) => Some((k.as_str(), o)),
            KvValue::Str(_) => None,
        })
    }

    /// Every string entry, in order.
    pub fn strings(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().filter_map(|(k, v)| match v {
            KvValue::Str(s) => Some((k.as_str(), s.as_str())),
            KvValue::Obj(_) => None,
        })
    }
}

/// The result of a parse: the root object and how many repairs were needed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// Every top-level pair.
    pub root: KvObject,
    /// Repairs made: stray braces, an unterminated string or object, a key without a value,
    /// skipped deep objects, dropped entries, truncated tokens, input beyond the byte limit.
    pub recovered: u32,
}

/// One token of the text form.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    /// A key or a string value, unescaped.
    Str(Vec<u8>),
    /// `{`.
    Open,
    /// `}`.
    Close,
}

/// The tokenizer shared by [`parse`] and the editor (`steam::launch_options`), so the two
/// never disagree about where a token is.
pub(crate) struct Lexer<'a> {
    b: &'a [u8],
    i: usize,
    max_token: usize,
    recovered: u32,
}

impl<'a> Lexer<'a> {
    /// A tokenizer over all of `b` (no byte limit; a byte-order mark is skipped), keeping
    /// at most `max_token` bytes of a string.
    pub(crate) fn new(b: &'a [u8], max_token: usize) -> Self {
        let i = if b.starts_with(b"\xEF\xBB\xBF") { 3 } else { 0 };
        Lexer {
            b,
            i,
            max_token,
            recovered: 0,
        }
    }

    /// Repairs made so far (an unterminated string, a truncated token).
    pub(crate) fn recovered(&self) -> u32 {
        self.recovered
    }

    /// The next token with its byte range in the input: from its first byte (the opening
    /// quote of a quoted string) to just after its last (the closing quote).
    pub(crate) fn next_spanned(&mut self) -> Option<(Tok, usize, usize)> {
        loop {
            let start = self.i;
            let tok = self.next_here()?;
            if let Some(t) = tok {
                return Some((t, start, self.i));
            }
        }
    }
}

impl Lexer<'_> {
    fn skip_line(&mut self) {
        while let Some(&c) = self.b.get(self.i) {
            self.i += 1;
            if c == b'\n' {
                break;
            }
        }
    }

    fn push(&mut self, s: &mut Vec<u8>, c: u8, truncated: &mut bool) {
        if s.len() < self.max_token {
            s.push(c);
        } else if !*truncated {
            *truncated = true;
            self.recovered += 1;
        }
    }

    fn next(&mut self) -> Option<Tok> {
        loop {
            if let Some(t) = self.next_here()? {
                return Some(t);
            }
        }
    }

    /// At the current byte: `None` at the end, `Some(None)` after skipping white space, a
    /// comment or a conditional, `Some(Some(token))` after reading a token.
    fn next_here(&mut self) -> Option<Option<Tok>> {
        let c = *self.b.get(self.i)?;
        match c {
            b'{' => {
                self.i += 1;
                return Some(Some(Tok::Open));
            }
            b'}' => {
                self.i += 1;
                return Some(Some(Tok::Close));
            }
            b'/' if self.b.get(self.i + 1) == Some(&b'/') => self.skip_line(),
            b'#' => self.skip_line(),
            b'[' => {
                // A platform conditional such as [$WIN32]: skipped up to `]` or the line end.
                self.i += 1;
                while let Some(&c) = self.b.get(self.i) {
                    if c == b'\n' {
                        break;
                    }
                    self.i += 1;
                    if c == b']' {
                        break;
                    }
                }
            }
            b'"' => {
                self.i += 1;
                return Some(Some(Tok::Str(self.quoted())));
            }
            c if c.is_ascii_whitespace() => self.i += 1,
            _ => return Some(Some(Tok::Str(self.bare()))),
        }
        Some(None)
    }

    fn quoted(&mut self) -> Vec<u8> {
        let mut s = Vec::new();
        let mut truncated = false;
        loop {
            let Some(&c) = self.b.get(self.i) else {
                // Unterminated: keep what was read.
                self.recovered += 1;
                return s;
            };
            self.i += 1;
            match c {
                b'"' => return s,
                b'\\' => {
                    let Some(&e) = self.b.get(self.i) else {
                        self.recovered += 1;
                        return s;
                    };
                    self.i += 1;
                    match e {
                        b'n' => self.push(&mut s, b'\n', &mut truncated),
                        b't' => self.push(&mut s, b'\t', &mut truncated),
                        b'\\' => self.push(&mut s, b'\\', &mut truncated),
                        b'"' => self.push(&mut s, b'"', &mut truncated),
                        other => {
                            self.push(&mut s, b'\\', &mut truncated);
                            self.push(&mut s, other, &mut truncated);
                        }
                    }
                }
                c => self.push(&mut s, c, &mut truncated),
            }
        }
    }

    fn bare(&mut self) -> Vec<u8> {
        let mut s = Vec::new();
        let mut truncated = false;
        while let Some(&c) = self.b.get(self.i) {
            if c.is_ascii_whitespace() || matches!(c, b'{' | b'}' | b'"') {
                break;
            }
            self.i += 1;
            self.push(&mut s, c, &mut truncated);
        }
        s
    }
}

pub(crate) fn text(v: Vec<u8>) -> String {
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

/// One open object on the parser's stack: the key it will be stored under and its pairs.
struct Frame {
    key: String,
    obj: KvObject,
}

/// Parse `input` (at most `limits.max_bytes` of it). Never fails; see the module docs.
pub fn parse(input: &[u8], limits: &KvLimits) -> Parsed {
    let mut recovered = 0u32;
    let input = if input.len() > limits.max_bytes {
        recovered += 1;
        &input[..limits.max_bytes]
    } else {
        input
    };
    let input = input.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(input);
    let mut lx = Lexer {
        b: input,
        i: 0,
        max_token: limits.max_token_bytes,
        recovered: 0,
    };
    let mut stack: Vec<Frame> = vec![Frame {
        key: String::new(),
        obj: KvObject::default(),
    }];
    let mut entries = 0usize;
    // A key read, waiting for its value.
    let mut pending: Option<String> = None;
    // Depth of an object being skipped (too deep or keyless); 0 = not skipping.
    let mut skipping = 0usize;

    while let Some(tok) = lx.next() {
        if skipping > 0 {
            match tok {
                Tok::Open => skipping += 1,
                Tok::Close => skipping -= 1,
                Tok::Str(_) => {}
            }
            continue;
        }
        match (tok, pending.take()) {
            (Tok::Str(k), None) => pending = Some(text(k)),
            (Tok::Str(v), Some(k)) => {
                entries += 1;
                if entries <= limits.max_entries {
                    if let Some(top) = stack.last_mut() {
                        top.obj.0.push((k, KvValue::Str(text(v))));
                    }
                } else {
                    recovered = recovered.saturating_add(1);
                }
            }
            (Tok::Open, Some(k)) => {
                // `stack.len() - 1` is the depth of the current object; the new one is one
                // deeper.
                if stack.len() > limits.max_depth {
                    recovered = recovered.saturating_add(1);
                    skipping = 1;
                } else {
                    stack.push(Frame {
                        key: k,
                        obj: KvObject::default(),
                    });
                }
            }
            (Tok::Open, None) => {
                // An object without a key: read past it.
                recovered = recovered.saturating_add(1);
                skipping = 1;
            }
            (Tok::Close, pending_key) => {
                if pending_key.is_some() {
                    // A key without a value before `}`.
                    recovered = recovered.saturating_add(1);
                }
                if stack.len() > 1 {
                    close_top(&mut stack, &mut entries, limits, &mut recovered);
                } else {
                    // A stray `}` at the top level.
                    recovered = recovered.saturating_add(1);
                }
            }
        }
    }
    if pending.is_some() || skipping > 0 {
        recovered = recovered.saturating_add(1);
    }
    // Objects left open at the end of the input are closed.
    while stack.len() > 1 {
        recovered = recovered.saturating_add(1);
        close_top(&mut stack, &mut entries, limits, &mut recovered);
    }
    let root = stack.pop().map(|f| f.obj).unwrap_or_default();
    Parsed {
        root,
        recovered: recovered.saturating_add(lx.recovered),
    }
}

fn close_top(stack: &mut Vec<Frame>, entries: &mut usize, limits: &KvLimits, recovered: &mut u32) {
    let Some(done) = stack.pop() else { return };
    *entries += 1;
    if *entries > limits.max_entries {
        *recovered = recovered.saturating_add(1);
        return;
    }
    if let Some(parent) = stack.last_mut() {
        parent.obj.0.push((done.key, KvValue::Obj(done.obj)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Parsed {
        parse(s.as_bytes(), &KvLimits::SMALL)
    }

    const MANIFEST: &str = include_str!("../../tests/data/steam/appmanifest_1174180.acf");
    const LIBRARY: &str = include_str!("../../tests/data/steam/libraryfolders.vdf");

    #[test]
    fn reads_steam_files() {
        let m = p(MANIFEST);
        assert_eq!(m.recovered, 0);
        let s = m.root.obj("AppState").unwrap();
        assert_eq!(s.str("appid"), Some("1174180"));
        assert_eq!(
            s.str("NAME"),
            Some("Red Dead Redemption 2"),
            "case-insensitive"
        );
        assert_eq!(s.u64("StateFlags"), Some(6));
        assert!(s.obj("InstalledDepots").is_some());
        let l = p(LIBRARY);
        assert_eq!(l.recovered, 0);
        let lf = l.root.obj("libraryfolders").unwrap();
        assert_eq!(lf.objects().count(), 2);
        assert_eq!(
            l.root.path(&["libraryfolders", "1"]).unwrap().str("label"),
            Some("Games")
        );
    }

    #[test]
    fn escapes_bare_tokens_comments_and_conditionals() {
        let o = p(
            "a \"x\\\"y\\\\z\\n\\q\" b plain // note\n c \"v\" [$WIN32] \"d\" { } \
                   #include \"x.vdf\"\n \"e\" \"1\"",
        )
        .root;
        assert_eq!(o.str("a"), Some("x\"y\\z\n\\q"));
        assert_eq!(o.str("b"), Some("plain"));
        assert_eq!(o.str("c"), Some("v"));
        assert_eq!(o.obj("d"), Some(&KvObject::default()));
        assert_eq!(o.u64("e"), Some(1));
    }

    #[test]
    fn repairs_instead_of_failing() {
        // Cut off inside an object and inside a string.
        let r = p("\"a\" { \"b\" \"1\" \"c\" { \"d\" \"unterminated");
        let a = r.root.obj("a").unwrap();
        assert_eq!(a.str("b"), Some("1"));
        assert_eq!(a.obj("c").unwrap().str("d"), Some("unterminated"));
        assert!(r.recovered >= 2);
        // A stray closing brace and a key without a value.
        let r = p("} \"a\" \"1\" \"b\" }");
        assert_eq!(r.root.str("a"), Some("1"));
        assert!(r.recovered >= 2);
        // An object without a key is skipped, including its contents.
        let r = p("{ \"x\" \"y\" } \"a\" \"1\"");
        assert_eq!(r.root.0.len(), 1);
        assert_eq!(r.root.str("a"), Some("1"));
        // Not UTF-8.
        let r = parse(b"\"n\" \"caf\xe9\"", &KvLimits::SMALL);
        assert_eq!(r.root.str("n"), Some("caf\u{fffd}"));
        // A byte-order mark.
        let r = parse(b"\xEF\xBB\xBF\"a\" \"1\"", &KvLimits::SMALL);
        assert_eq!(r.root.str("a"), Some("1"));
        // Empty and blank input.
        assert_eq!(p("").root, KvObject::default());
        assert_eq!(p(" \n\t ").recovered, 0);
    }

    #[test]
    fn deep_objects_are_skipped_without_recursion() {
        let limits = KvLimits {
            max_depth: 3,
            ..KvLimits::SMALL
        };
        let s = "a { b { c { d { e \"1\" } } } x \"2\" }";
        let r = parse(s.as_bytes(), &limits);
        let c = r.root.path(&["a", "b", "c"]).unwrap();
        assert!(c.obj("d").is_none(), "depth 4 skipped");
        assert_eq!(r.root.obj("a").unwrap().str("x"), Some("2"));
        assert!(r.recovered >= 1);
        // A million opening braces: no stack overflow, linear time.
        let deep = "k {".repeat(1_000_000);
        let r = parse(deep.as_bytes(), &KvLimits::LARGE);
        assert!(r.recovered >= 1);
    }

    #[test]
    fn limits_bound_entries_tokens_and_input() {
        let limits = KvLimits {
            max_entries: 3,
            max_token_bytes: 4,
            max_bytes: 64,
            max_depth: 8,
        };
        let r = parse(b"a 1 b 2 c 3 d 4 e 5", &limits);
        assert_eq!(r.root.0.len(), 3);
        let r = parse(b"\"abcdefgh\" \"12345678\"", &limits);
        assert_eq!(r.root.str("abcd"), Some("1234"));
        let long = format!("a 1 {}", " ".repeat(100));
        assert!(parse(long.as_bytes(), &limits).recovered >= 1);
    }

    #[test]
    fn never_panics_on_prefixes_and_mutations() {
        // The fuzz target's stub (fuzz/fuzz_targets/steam_kv.rs): every prefix, every
        // single-byte deletion of the corpus.
        for doc in [MANIFEST, LIBRARY] {
            let b = doc.as_bytes();
            for i in 0..b.len() {
                let _ = parse(&b[..i], &KvLimits::SMALL);
                let mut m = b.to_vec();
                m.remove(i);
                let _ = parse(&m, &KvLimits::SMALL);
            }
        }
        for junk in [
            &b"{{{{}}}}"[..],
            b"\"\\",
            b"[",
            b"//",
            b"\"a\"{",
            b"}}}}",
            b"\0\0",
        ] {
            let _ = parse(junk, &KvLimits::SMALL);
        }
    }
}

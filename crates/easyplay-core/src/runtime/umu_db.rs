//! The game-fix database shipped with umu-protonfixes, `protonfixes/umu-database.csv`
//! inside a Proton build (UMU_INTEGRATION §8, RT-014).
//!
//! The file is parsed with a bounded CSV reader (RFC 4180 quoting, at most [`MAX_ROWS`]
//! rows, fields cut at [`MAX_FIELD`] bytes, the file itself at most [`MAX_FILE`] bytes).
//! Columns are located by their header names (`TITLE`, `STORE`, `CODENAME`, `UMU_ID`,
//! optional `COMMON ACRONYM`, `NOTE`); rows without a title or umu ID are skipped. No
//! network.
//!
//! Titles are matched exactly after [`normalize_title`] (case-insensitive, punctuation
//! removed, whitespace collapsed). [`UmuDbCache`] keeps one parsed database per build
//! directory, keyed by the file's size and modification time.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;

use crate::error::{Error, ErrorCode, Result};
use crate::fsx::Fs;

/// Rows read at most.
pub const MAX_ROWS: usize = 100_000;
/// Bytes kept per field.
pub const MAX_FIELD: usize = 512;
/// Largest file read (the 2026 database is about 90 KiB).
pub const MAX_FILE: u64 = 16 << 20;
/// Path of the database inside a build.
pub const DB_FILE: &str = "protonfixes/umu-database.csv";

/// One row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UmuDbEntry {
    /// Game title.
    pub title: String,
    /// Store (`steam`, `gog`, `egs`, `none`, …), lowercase.
    pub store: String,
    /// The store's identifier of the game (`CODENAME`; the app ID for `steam`).
    pub codename: String,
    /// umu `GAMEID` (`umu-<n>`).
    pub umu_id: String,
    /// Common acronym.
    pub acronym: Option<String>,
    /// Note.
    pub note: Option<String>,
}

impl UmuDbEntry {
    /// The Steam app ID when this entry's store is `steam`.
    pub fn steam_app_id(&self) -> Option<u32> {
        (self.store == "steam")
            .then(|| self.codename.parse().ok())
            .flatten()
    }
}

/// Lowercase, keep letters, digits and single spaces (UMU_INTEGRATION §8).
pub fn normalize_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.extend(c.to_lowercase());
        } else if c.is_whitespace() || c == '_' || c == '-' {
            space = true;
        }
    }
    out
}

/// Split CSV text into records (RFC 4180 quoting; CR LF or LF line ends).
fn records(text: &str, mut on_row: impl FnMut(Vec<String>) -> bool) {
    let mut fields: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    let push = |field: &mut String, fields: &mut Vec<String>| {
        let mut f = std::mem::take(field);
        if f.len() > MAX_FIELD {
            let mut cut = MAX_FIELD;
            while !f.is_char_boundary(cut) {
                cut -= 1;
            }
            f.truncate(cut);
        }
        fields.push(f);
    };
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                c if field.len() <= MAX_FIELD * 4 => field.push(c),
                _ => {}
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => push(&mut field, &mut fields),
            '\r' => {}
            '\n' => {
                push(&mut field, &mut fields);
                if !on_row(std::mem::take(&mut fields)) {
                    return;
                }
            }
            c if field.len() <= MAX_FIELD * 4 => field.push(c),
            _ => {}
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        push(&mut field, &mut fields);
        on_row(fields);
    }
}

/// A parsed database.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UmuDb {
    /// Every usable row, in file order.
    pub entries: Vec<UmuDbEntry>,
    /// Rows skipped (missing columns, empty title or ID).
    pub skipped: usize,
    by_title: HashMap<String, Vec<usize>>,
}

impl UmuDb {
    /// Parse the file's text.
    pub fn parse(text: &str) -> Result<Self> {
        let mut header: Option<[Option<usize>; 6]> = None;
        let mut db = UmuDb::default();
        let mut rows = 0usize;
        let mut bad_header = false;
        records(text, |row| {
            rows += 1;
            if rows > MAX_ROWS + 1 {
                return false;
            }
            let Some(h) = header else {
                let find = |name: &str| {
                    row.iter().position(|c| {
                        c.trim().eq_ignore_ascii_case(name)
                            || c.trim()
                                .to_ascii_uppercase()
                                .starts_with(&format!("{name} ("))
                    })
                };
                let h = [
                    find("TITLE"),
                    find("STORE"),
                    find("CODENAME"),
                    find("UMU_ID"),
                    find("COMMON ACRONYM"),
                    find("NOTE"),
                ];
                if h[0].is_none() || h[1].is_none() || h[2].is_none() || h[3].is_none() {
                    bad_header = true;
                    return false;
                }
                header = Some(h);
                return true;
            };
            let get = |i: Option<usize>| {
                i.and_then(|i| row.get(i))
                    .map(|s| s.trim().to_owned())
                    .filter(|s| !s.is_empty())
            };
            match (get(h[0]), get(h[3])) {
                (Some(title), Some(umu_id)) => db.entries.push(UmuDbEntry {
                    title,
                    store: get(h[1]).unwrap_or_default().to_ascii_lowercase(),
                    codename: get(h[2]).unwrap_or_default(),
                    umu_id,
                    acronym: get(h[4]),
                    note: get(h[5]),
                }),
                _ if row.iter().all(|f| f.trim().is_empty()) => {}
                _ => db.skipped += 1,
            }
            true
        });
        if bad_header {
            return Err(Error::new(ErrorCode::RtInvalidFolder)
                .with("file", DB_FILE)
                .with("rule", "no TITLE, STORE, CODENAME and UMU_ID columns"));
        }
        for (i, e) in db.entries.iter().enumerate() {
            db.by_title
                .entry(normalize_title(&e.title))
                .or_default()
                .push(i);
        }
        Ok(db)
    }

    /// Read `<build>/protonfixes/umu-database.csv`.
    pub fn load(fs: &dyn Fs, build_dir: &Path) -> Result<Self> {
        let bytes = fs.read(&build_dir.join(DB_FILE), MAX_FILE)?;
        Self::parse(&String::from_utf8_lossy(&bytes))
    }

    /// Entries whose normalised title equals `name`'s.
    pub fn by_title(&self, name: &str) -> Vec<&UmuDbEntry> {
        self.by_title
            .get(&normalize_title(name))
            .map(|v| v.iter().filter_map(|&i| self.entries.get(i)).collect())
            .unwrap_or_default()
    }

    /// The umu ID of an exact title match, when the match is unambiguous.
    pub fn exact_title(&self, name: &str) -> Option<String> {
        let hits = self.by_title(name);
        let first = hits.first()?;
        hits.iter()
            .all(|e| e.umu_id == first.umu_id)
            .then(|| first.umu_id.clone())
    }

    /// Entries with this umu ID.
    pub fn by_umu_id(&self, umu_id: &str) -> Vec<&UmuDbEntry> {
        self.entries.iter().filter(|e| e.umu_id == umu_id).collect()
    }

    /// The Steam app ID of the game: a `steam` entry with its umu ID, else a `steam` entry
    /// with its exact title.
    pub fn steam_app_id(&self, umu_id: Option<&str>, title: &str) -> Option<u32> {
        if let Some(id) = umu_id
            && let Some(app) = self.by_umu_id(id).iter().find_map(|e| e.steam_app_id())
        {
            return Some(app);
        }
        self.by_title(title).iter().find_map(|e| e.steam_app_id())
    }
}

/// A cached database with the file's `(size, mtime)`.
type Cached = ((u64, i64), Arc<UmuDb>);

/// Parsed databases per build directory.
#[derive(Default)]
pub struct UmuDbCache {
    map: Mutex<HashMap<PathBuf, Cached>>,
}

impl std::fmt::Debug for UmuDbCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UmuDbCache")
            .field("builds", &self.map.lock().len())
            .finish()
    }
}

impl UmuDbCache {
    /// Empty.
    pub fn new() -> Self {
        Self::default()
    }

    /// The database of `build_dir` (`None` when the build has none or it is unreadable).
    pub fn get(&self, fs: &dyn Fs, build_dir: &Path) -> Option<Arc<UmuDb>> {
        let file = build_dir.join(DB_FILE);
        let md = fs.metadata_follow(&file).ok().filter(|m| m.is_file())?;
        let key = (md.size, md.mtime_ns);
        if let Some((k, db)) = self.map.lock().get(build_dir)
            && *k == key
        {
            return Some(Arc::clone(db));
        }
        match UmuDb::load(fs, build_dir) {
            Ok(db) => {
                let db = Arc::new(db);
                self.map
                    .lock()
                    .insert(build_dir.to_path_buf(), (key, Arc::clone(&db)));
                Some(db)
            }
            Err(e) => {
                tracing::debug!(build = %build_dir.display(), error = %e, "umu database unreadable");
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "TITLE,STORE,CODENAME,UMU_ID,COMMON ACRONYM (Optional),NOTE (Optional),EXE_STRINGS (Optional)\n\
Age of Wonders,gog,1207658883,umu-61500,aow,,\n\
\"Warhammer 40,000: Space Marine\",steam,55150,umu-55150,,,\n\
Trackmania,ubisoft,5595,umu-2225070,tm,\"also known as 'Trackmania (2020)', \"\"quoted\"\"\",\n\
,steam,1,umu-1,,,\n\
Borderlands 3,egs,Catnip,umu-397540,bl3,,\r\n";

    #[test]
    fn parses_quoting_and_skips_bad_rows() {
        let db = UmuDb::parse(SAMPLE).unwrap();
        assert_eq!(db.entries.len(), 4);
        assert_eq!(db.skipped, 1);
        assert_eq!(db.entries[1].title, "Warhammer 40,000: Space Marine");
        assert_eq!(
            db.entries[2].note.as_deref(),
            Some("also known as 'Trackmania (2020)', \"quoted\"")
        );
        assert_eq!(db.entries[3].codename, "Catnip");
        assert_eq!(db.entries[0].acronym.as_deref(), Some("aow"));
    }

    #[test]
    fn matcher() {
        let db = UmuDb::parse(SAMPLE).unwrap();
        assert_eq!(
            db.exact_title("warhammer 40000 space marine").as_deref(),
            Some("umu-55150")
        );
        assert_eq!(
            db.exact_title("BORDERLANDS 3").as_deref(),
            Some("umu-397540")
        );
        assert_eq!(db.exact_title("Borderlands"), None);
        assert_eq!(db.steam_app_id(Some("umu-55150"), "x"), Some(55150));
        assert_eq!(
            db.steam_app_id(None, "Warhammer 40,000 - Space Marine"),
            Some(55150)
        );
        assert_eq!(db.steam_app_id(Some("umu-61500"), "Age of Wonders"), None);
        assert_eq!(
            normalize_title("  Hollow   Knight: Silksong! "),
            "hollow knight silksong"
        );
    }

    #[test]
    fn bad_header_and_caps() {
        assert!(UmuDb::parse("A,B\n1,2\n").is_err());
        let long = "x".repeat(2000);
        let db = UmuDb::parse(&format!(
            "TITLE,STORE,CODENAME,UMU_ID\n{long},steam,1,umu-1\n"
        ))
        .unwrap();
        assert_eq!(db.entries[0].title.len(), MAX_FIELD);
    }
}

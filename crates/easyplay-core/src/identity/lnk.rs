//! Minimal reader of Windows shortcut files (MS-SHLLINK) for the installer flow
//! (LOGIC_SPECS §13): the target path of shortcuts an installer created inside a prefix.
//!
//! Bounded (at most [`LNK_MAX_BYTES`], strings at most [`LNK_MAX_STRING_CHARS`]), no
//! allocation driven by untrusted sizes beyond those caps, and target resolution never
//! follows a symlink.

use std::path::{Component, Path, PathBuf};

use crate::fsx::{FileType, Fs};

/// Largest shortcut file read.
pub const LNK_MAX_BYTES: u64 = 1024 * 1024;
/// Longest string kept, in characters.
pub const LNK_MAX_STRING_CHARS: usize = 4096;
/// Most path components resolved inside a prefix.
const MAX_COMPONENTS: usize = 64;
/// Most entries listed per directory during case-insensitive resolution.
const MAX_DIR_ENTRIES: usize = 4096;

const HEADER_SIZE: usize = 0x4c;
const LINK_CLSID: [u8; 16] = [
    0x01, 0x14, 0x02, 0x00, 0x00, 0x00, 0x00, 0x00, 0xc0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46,
];

const HAS_ID_LIST: u32 = 0x01;
const HAS_LINK_INFO: u32 = 0x02;
const HAS_NAME: u32 = 0x04;
const HAS_RELATIVE_PATH: u32 = 0x08;
const HAS_WORKING_DIR: u32 = 0x10;
const HAS_ARGUMENTS: u32 = 0x20;
const HAS_ICON_LOCATION: u32 = 0x40;
const IS_UNICODE: u32 = 0x80;

/// Why a shortcut could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LnkError {
    /// Wrong header size or class ID.
    #[error("not a shortcut file")]
    NotShortcut,
    /// A structure extends past the end of the data.
    #[error("shortcut is truncated")]
    Truncated,
    /// A string or the file is larger than the limits.
    #[error("shortcut exceeds limits")]
    TooLarge,
    /// The file could not be read.
    #[error("shortcut could not be read")]
    Unreadable,
}

/// What a shortcut points at. Strings are Windows paths as stored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LnkTarget {
    /// `LinkInfo` local base path plus common path suffix (`C:\Games\X\x.exe`).
    pub local_path: Option<String>,
    /// `RELATIVE_PATH` string data, relative to the shortcut's folder.
    pub relative_path: Option<String>,
    /// Working directory.
    pub working_dir: Option<String>,
    /// Command-line arguments.
    pub arguments: Option<String>,
    /// Description (`NAME_STRING`).
    pub name: Option<String>,
    /// Icon location.
    pub icon_location: Option<String>,
}

struct Cursor<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], LnkError> {
        let end = self.pos.checked_add(n).ok_or(LnkError::Truncated)?;
        let s = self.b.get(self.pos..end).ok_or(LnkError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    fn u16(&mut self) -> Result<u16, LnkError> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn clean(s: String) -> Option<String> {
    let s: String = s.chars().filter(|c| !c.is_control()).collect();
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_owned())
}

/// ANSI strings are decoded as Latin-1 (a lossless byte-to-char map); exact code pages do
/// not matter for the ASCII paths installers write.
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| char::from(c)).collect()
}

fn cstr(b: &[u8], off: usize) -> Option<String> {
    let tail = b.get(off..)?;
    let end = tail
        .iter()
        .take(LNK_MAX_STRING_CHARS + 1)
        .position(|c| *c == 0)?;
    clean(latin1(&tail[..end]))
}

fn wstr(b: &[u8], off: usize) -> Option<String> {
    let tail = b.get(off..)?;
    let mut words = Vec::new();
    for pair in tail
        .as_chunks::<2>()
        .0
        .iter()
        .take(LNK_MAX_STRING_CHARS + 1)
    {
        let w = u16::from_le_bytes([pair[0], pair[1]]);
        if w == 0 {
            return clean(String::from_utf16_lossy(&words));
        }
        words.push(w);
    }
    None
}

fn link_info(info: &[u8]) -> Option<String> {
    let header = u32_at(info, 4)? as usize;
    let flags = u32_at(info, 8)?;
    if flags & 1 == 0 {
        return None; // no VolumeIDAndLocalBasePath
    }
    let base_off = u32_at(info, 16)? as usize;
    let suffix_off = u32_at(info, 24)? as usize;
    let (base, suffix) = if header >= 0x24 {
        let ub = u32_at(info, 28).map(|o| o as usize);
        let us = u32_at(info, 32).map(|o| o as usize);
        (
            ub.and_then(|o| wstr(info, o))
                .or_else(|| cstr(info, base_off)),
            us.and_then(|o| wstr(info, o))
                .or_else(|| cstr(info, suffix_off)),
        )
    } else {
        (cstr(info, base_off), cstr(info, suffix_off))
    };
    let base = base?;
    Some(match suffix {
        Some(s) if !base.ends_with('\\') => format!("{base}\\{s}"),
        Some(s) => format!("{base}{s}"),
        None => base,
    })
}

/// Parse shortcut bytes.
pub fn parse(bytes: &[u8]) -> Result<LnkTarget, LnkError> {
    if bytes.len() as u64 > LNK_MAX_BYTES {
        return Err(LnkError::TooLarge);
    }
    if bytes.len() < HEADER_SIZE {
        return Err(LnkError::Truncated);
    }
    if u32_at(bytes, 0) != Some(HEADER_SIZE as u32) || bytes[4..20] != LINK_CLSID {
        return Err(LnkError::NotShortcut);
    }
    let flags = u32_at(bytes, 0x14).ok_or(LnkError::Truncated)?;
    let mut c = Cursor {
        b: bytes,
        pos: HEADER_SIZE,
    };
    if flags & HAS_ID_LIST != 0 {
        let n = usize::from(c.u16()?);
        c.take(n)?;
    }
    let mut out = LnkTarget::default();
    if flags & HAS_LINK_INFO != 0 {
        let size = u32_at(bytes, c.pos).ok_or(LnkError::Truncated)? as usize;
        if size < 0x1c {
            return Err(LnkError::Truncated);
        }
        let info = c.take(size)?;
        out.local_path = link_info(info);
    }
    let unicode = flags & IS_UNICODE != 0;
    let string = |c: &mut Cursor<'_>| -> Result<Option<String>, LnkError> {
        let n = usize::from(c.u16()?);
        if n > LNK_MAX_STRING_CHARS {
            return Err(LnkError::TooLarge);
        }
        if unicode {
            let raw = c.take(n * 2)?;
            let words: Vec<u16> = raw
                .as_chunks::<2>()
                .0
                .iter()
                .map(|p| u16::from_le_bytes([p[0], p[1]]))
                .collect();
            Ok(clean(String::from_utf16_lossy(&words)))
        } else {
            Ok(clean(latin1(c.take(n)?)))
        }
    };
    if flags & HAS_NAME != 0 {
        out.name = string(&mut c)?;
    }
    if flags & HAS_RELATIVE_PATH != 0 {
        out.relative_path = string(&mut c)?;
    }
    if flags & HAS_WORKING_DIR != 0 {
        out.working_dir = string(&mut c)?;
    }
    if flags & HAS_ARGUMENTS != 0 {
        out.arguments = string(&mut c)?;
    }
    if flags & HAS_ICON_LOCATION != 0 {
        out.icon_location = string(&mut c)?;
    }
    Ok(out)
}

/// Read and parse the shortcut at `path` (not through a final symlink).
pub fn read(fs: &dyn Fs, path: &Path) -> Result<LnkTarget, LnkError> {
    match fs.read(path, LNK_MAX_BYTES) {
        Ok(b) => parse(&b),
        Err(e) if matches!(e.kind, crate::fsx::FsErrorKind::TooLarge { .. }) => {
            Err(LnkError::TooLarge)
        }
        Err(_) => Err(LnkError::Unreadable),
    }
}

/// The path inside `prefix` that a `C:\…` Windows path names, lexically
/// (`<prefix>/drive_c/…`). Other drives (including Wine's `Z:` = host root), UNC paths and
/// `..` components are refused.
pub fn windows_to_prefix(prefix: &Path, win: &str) -> Option<PathBuf> {
    let rest = win.strip_prefix("C:").or_else(|| win.strip_prefix("c:"))?;
    let rest = rest.strip_prefix('\\').or_else(|| rest.strip_prefix('/'))?;
    let mut out = prefix.join("drive_c");
    for part in rest.split(['\\', '/']) {
        match part {
            "" | "." => {}
            ".." => return None,
            p if p.contains('\0') => return None,
            p => out.push(p),
        }
    }
    Some(out)
}

/// The target of `lnk` (a shortcut file inside `prefix`) as a host path inside the prefix,
/// resolved case-insensitively like Wine does, component by component, refusing symlinks.
/// Tries the local path first, then the relative path. Returns only regular files.
pub fn resolve_target(
    fs: &dyn Fs,
    prefix: &Path,
    lnk: &Path,
    target: &LnkTarget,
) -> Option<PathBuf> {
    let from_local = target
        .local_path
        .as_deref()
        .and_then(|w| windows_to_prefix(prefix, w));
    let from_relative = target.relative_path.as_deref().and_then(|r| {
        let mut p = lnk.parent()?.to_path_buf();
        for part in r.split(['\\', '/']) {
            match part {
                "" | "." => {}
                ".." => {
                    p.pop();
                }
                x => p.push(x),
            }
        }
        Some(p)
    });
    [from_local, from_relative]
        .into_iter()
        .flatten()
        .filter(|p| p.starts_with(prefix))
        .find_map(|p| resolve_case_insensitive(fs, prefix, &p))
}

fn resolve_case_insensitive(fs: &dyn Fs, prefix: &Path, want: &Path) -> Option<PathBuf> {
    let rel = want.strip_prefix(prefix).ok()?;
    let parts: Vec<_> = rel.components().collect();
    if parts.len() > MAX_COMPONENTS {
        return None;
    }
    let mut cur = prefix.to_path_buf();
    for (i, comp) in parts.iter().enumerate() {
        let Component::Normal(name) = comp else {
            return None;
        };
        let exact = cur.join(name);
        let next = if fs.metadata(&exact).is_ok() {
            exact
        } else {
            let lname = name.to_string_lossy().to_lowercase();
            let entries = fs.read_dir(&cur).ok()?;
            entries
                .into_iter()
                .take(MAX_DIR_ENTRIES)
                .find(|e| e.name.to_string_lossy().to_lowercase() == lname)?
                .path
        };
        let md = fs.metadata(&next).ok()?;
        let last = i + 1 == parts.len();
        match md.file_type {
            FileType::Dir if !last => {}
            FileType::File if last => {}
            _ => return None,
        }
        cur = next;
    }
    Some(cur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_translation() {
        let p = Path::new("/pfx");
        assert_eq!(
            windows_to_prefix(p, "C:\\Program Files\\Game\\game.exe"),
            Some(PathBuf::from("/pfx/drive_c/Program Files/Game/game.exe"))
        );
        assert_eq!(windows_to_prefix(p, "Z:\\etc\\passwd"), None);
        assert_eq!(windows_to_prefix(p, "C:\\a\\..\\..\\x"), None);
        assert_eq!(windows_to_prefix(p, "\\\\server\\share"), None);
        assert_eq!(windows_to_prefix(p, "C:relative"), None);
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse(b""), Err(LnkError::Truncated));
        assert_eq!(parse(&[0u8; 0x4c]), Err(LnkError::NotShortcut));
    }
}

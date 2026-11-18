//! Reading PE files: headers, version strings, the largest icon and imported DLL names
//! (GAME_IDENTITY §6, THREAT_MODEL T-18).
//!
//! # How the file is read
//!
//! The file is untrusted. Nothing is memory-mapped: every byte comes from a bounds-checked
//! positional read ([`Fs::read_range`], i.e. `pread`) into an owned buffer, so a file that
//! shrinks while it is read yields a short read and an error, never `SIGBUS`.
//!
//! - DOS, COFF and optional headers and the section table are parsed here (≤ 4 KiB plus
//!   the section table, at most 96 sections).
//! - The resource directory is read into one owned buffer (at most
//!   [`PeLimits::max_map_bytes`]) and handed to `pelite::resources::Resources::new`, which
//!   parses resources from a byte slice and bounds-checks every offset. Before `pelite`
//!   looks anything up, [`walk_resources`] counts the tree with the depth and entry caps, so
//!   cyclic or exploding directories are rejected up front.
//! - Import names are read from the section that holds the import directory (at most
//!   [`IMPORT_SECTION_MAX_BYTES`]).
//!
//! Evaluation of `pelite` (IDENT-002, recorded in ADR-0008): its `PeFile` needs the whole
//! image as one slice, which conflicts with requirement 2, but its resource parser works on
//! an owned resource-section buffer, does no unchecked indexing, and needs no allocation per
//! entry. So headers and imports are parsed in-house (≈ 200 lines) and `pelite` is used only
//! for resources. Every public entry point also catches panics.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use pelite::image::IMAGE_DATA_DIRECTORY;
use pelite::resources::version_info::{Language, VersionInfo, Visit};
use pelite::resources::{Directory, Entry, Name, Resources};

use crate::clock::{Clock, MonoInstant};
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, FsError, FsErrorKind};
use crate::model::{PeInfo, PeMachine, PeSubsystem};

/// Hard limits applied to one file (GAME_IDENTITY §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PeLimits {
    /// Most bytes of the file read into memory for one request (headers + resources).
    pub max_map_bytes: u64,
    /// Deepest resource directory level visited (the normal tree has 3).
    pub max_resource_depth: u8,
    /// Most resource directory entries visited.
    pub max_resource_entries: u32,
    /// Longest version string kept, in UTF-8 bytes.
    pub max_string_bytes: usize,
    /// Largest icon image accepted.
    pub max_icon_bytes: usize,
    /// Time budget of one request.
    pub deadline: Duration,
}

impl Default for PeLimits {
    fn default() -> Self {
        Self {
            max_map_bytes: 256 * 1024 * 1024,
            max_resource_depth: 4,
            max_resource_entries: 4096,
            max_string_bytes: 256,
            max_icon_bytes: 4 * 1024 * 1024,
            deadline: Duration::from_secs(2),
        }
    }
}

/// Bytes of the section holding the import directory that are read at most.
pub const IMPORT_SECTION_MAX_BYTES: usize = 16 * 1024 * 1024;
/// Most import descriptors (normal and delay-load together) read.
pub const MAX_IMPORTS: usize = 1024;
/// Longest DLL name accepted.
const MAX_DLL_NAME: usize = 255;
/// Most sections accepted (the PE specification's limit).
const MAX_SECTIONS: usize = 96;
/// Most icon groups considered.
const MAX_ICON_GROUPS: usize = 64;

/// A single-image `.ico` file built from the largest icon of the executable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IconBytes {
    /// Complete ICO file (header, one directory entry, image).
    pub ico: Vec<u8>,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Bits per pixel as declared by the group entry.
    pub bit_count: u16,
}

/// What kind of `MZ` executable a file is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExeFormat {
    /// Portable Executable (Win32/Win64): what Zaun runs.
    Pe,
    /// 16-bit Windows "New Executable".
    Ne,
    /// OS/2 or VxD linear executable (`LE`/`LX`).
    Le,
    /// Plain DOS program.
    DosOnly,
}

/// Why a file could not be read as a PE file. Errors are values; callers fall back to
/// [`PeInfo::unknown`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PeError {
    /// No `MZ` signature.
    #[error("not an executable")]
    NotExecutable,
    /// An `MZ` executable that is not PE (DOS, Win16, OS/2).
    #[error("not a Windows PE program ({0:?})")]
    NotPe(ExeFormat),
    /// A header or table extends past the end of the file.
    #[error("file is truncated")]
    Truncated,
    /// A structure is invalid.
    #[error("malformed PE: {0}")]
    Malformed(&'static str),
    /// A limit of [`PeLimits`] was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(&'static str),
    /// The time budget ran out.
    #[error("deadline exceeded")]
    DeadlineExceeded,
    /// The file could not be read.
    #[error("read failed: {0:?}")]
    Io(FsErrorKind),
    /// The parser panicked (caught at the boundary).
    #[error("parser panicked")]
    Panicked,
}

impl From<FsError> for PeError {
    fn from(e: FsError) -> Self {
        PeError::Io(e.kind)
    }
}

impl From<PeError> for Error {
    fn from(e: PeError) -> Self {
        let code = match e {
            PeError::NotExecutable | PeError::NotPe(_) => ErrorCode::IdNotWindowsProgram,
            _ => ErrorCode::IdPeUnreadable,
        };
        Error::new(code).with("reason", e.to_string())
    }
}

/// Stable tokens stored in [`PeInfo::notes`]. Notes inform the user and the hints; they
/// never change a policy.
pub mod note {
    /// The image is a DLL (accepted; the user may know better).
    pub const DLL: &str = "dll";
    /// Native subsystem: a driver.
    pub const DRIVER: &str = "driver";
    /// Has a CLR header (.NET).
    pub const DOTNET: &str = "dotnet";
    /// A `.bind` section: SteamStub DRM wrapper (needs Steam running).
    pub const STEAM_DRM: &str = "steam_drm_bind_section";
    /// The resource section was larger than `max_map_bytes` and was read partially.
    pub const RESOURCES_TRUNCATED: &str = "resources_truncated";
    /// The resource tree exceeded the depth or entry caps and was ignored.
    pub const RESOURCES_OVER_LIMIT: &str = "resources_over_limit";
    /// The resource directory could not be read.
    pub const RESOURCES_UNREADABLE: &str = "resources_unreadable";
    /// The version resource is present but unreadable.
    pub const VERSION_UNREADABLE: &str = "version_unreadable";
    /// Not an x86 or x86-64 image (ARM64, Itanium …).
    pub const UNUSUAL_MACHINE: &str = "unusual_machine";
    /// Neither GUI nor console subsystem (EFI, POSIX …).
    pub const UNUSUAL_SUBSYSTEM: &str = "unusual_subsystem";
}

/// Reads PE files. Implementations run only on the job pool.
pub trait PeInspector: Send + Sync {
    /// Headers and version strings.
    fn inspect(&self, path: &Path, limits: &PeLimits) -> Result<PeInfo, PeError>;
    /// The largest embedded icon as ICO bytes, if any.
    fn largest_icon(&self, path: &Path, limits: &PeLimits) -> Result<Option<IconBytes>, PeError>;
    /// Imported DLL names (normal and delay-load), lower-case, sorted, deduplicated.
    fn imports(&self, path: &Path, limits: &PeLimits) -> Result<Vec<String>, PeError>;
}

impl PeInfo {
    /// What setup uses when a file cannot be parsed: nothing known.
    pub fn unknown() -> Self {
        PeInfo::default()
    }
}

/// [`PeInspector`] reading through [`Fs`] (never following a final symlink).
#[derive(Clone)]
pub struct FsPeInspector {
    fs: Arc<dyn Fs>,
    clock: Arc<dyn Clock>,
}

impl FsPeInspector {
    /// An inspector reading through `fs`, with deadlines measured on `clock`.
    pub fn new(fs: Arc<dyn Fs>, clock: Arc<dyn Clock>) -> Self {
        Self { fs, clock }
    }

    fn run<T>(
        &self,
        path: &Path,
        limits: &PeLimits,
        f: impl FnOnce(&Reader<'_>) -> Result<T, PeError>,
    ) -> Result<T, PeError> {
        let md = self.fs.metadata(path)?;
        if !md.is_file() {
            return Err(PeError::Io(if md.is_symlink() {
                FsErrorKind::Symlink
            } else {
                FsErrorKind::InvalidInput
            }));
        }
        let src = Source::File {
            fs: &*self.fs,
            path,
            len: md.size,
        };
        let budget = Budget {
            clock: Some(&*self.clock),
            until: self.clock.mono().saturating_add(limits.deadline),
        };
        guarded(|| f(&Reader::new(src, limits, budget)))
    }

    /// Classify the `MZ` executable at `path` from its headers only.
    pub fn format(&self, path: &Path) -> Result<ExeFormat, PeError> {
        self.run(path, &PeLimits::default(), |r| r.format())
    }
}

impl PeInspector for FsPeInspector {
    fn inspect(&self, path: &Path, limits: &PeLimits) -> Result<PeInfo, PeError> {
        self.run(path, limits, |r| r.inspect())
    }

    fn largest_icon(&self, path: &Path, limits: &PeLimits) -> Result<Option<IconBytes>, PeError> {
        self.run(path, limits, |r| r.largest_icon())
    }

    fn imports(&self, path: &Path, limits: &PeLimits) -> Result<Vec<String>, PeError> {
        self.run(path, limits, |r| r.imports())
    }
}

fn guarded<T>(f: impl FnOnce() -> Result<T, PeError>) -> Result<T, PeError> {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(Err(PeError::Panicked))
}

fn slice_budget() -> Budget<'static> {
    Budget {
        clock: None,
        until: MonoInstant::ZERO,
    }
}

/// [`PeInspector::inspect`] over bytes in memory (tests and fuzzing; no deadline).
pub fn inspect_bytes(bytes: &[u8], limits: &PeLimits) -> Result<PeInfo, PeError> {
    guarded(|| Reader::new(Source::Slice(bytes), limits, slice_budget()).inspect())
}

/// [`PeInspector::largest_icon`] over bytes in memory.
pub fn largest_icon_bytes(bytes: &[u8], limits: &PeLimits) -> Result<Option<IconBytes>, PeError> {
    guarded(|| Reader::new(Source::Slice(bytes), limits, slice_budget()).largest_icon())
}

/// [`PeInspector::imports`] over bytes in memory.
pub fn imports_bytes(bytes: &[u8], limits: &PeLimits) -> Result<Vec<String>, PeError> {
    guarded(|| Reader::new(Source::Slice(bytes), limits, slice_budget()).imports())
}

/// The `MZ` format of bytes in memory.
pub fn format_bytes(bytes: &[u8]) -> Result<ExeFormat, PeError> {
    guarded(|| Reader::new(Source::Slice(bytes), &PeLimits::default(), slice_budget()).format())
}

// ---- Sanitising --------------------------------------------------------------------------

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// Untrusted UTF-16 → display string: lossy conversion, control and bidirectional control
/// characters removed, trimmed, truncated to `max_bytes` UTF-8 bytes at a character
/// boundary. Empty results are `None`.
pub fn sanitize_utf16(words: &[u16], max_bytes: usize) -> Option<String> {
    // Each UTF-16 unit yields at least one byte, so reading more units than `4 × max` cannot
    // add kept characters beyond what truncation removes; it only bounds the work.
    let words = &words[..words.len().min(max_bytes.saturating_mul(4).max(1))];
    let mut out = String::new();
    for c in char::decode_utf16(words.iter().copied()) {
        let c = c.unwrap_or(char::REPLACEMENT_CHARACTER);
        if c.is_control() || is_bidi_control(c) {
            continue;
        }
        out.push(c);
    }
    sanitize_str(&out, max_bytes)
}

/// [`sanitize_utf16`] for text that is already a Rust string.
pub fn sanitize_str(s: &str, max_bytes: usize) -> Option<String> {
    let cleaned: String = s
        .chars()
        .filter(|c| !c.is_control() && !is_bidi_control(*c))
        .collect();
    let trimmed = cleaned.trim();
    let mut end = trimmed.len().min(max_bytes);
    while !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    let t = trimmed[..end].trim_end();
    (!t.is_empty()).then(|| t.to_owned())
}

// ---- Byte sources ------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Source<'a> {
    File {
        fs: &'a dyn Fs,
        path: &'a Path,
        len: u64,
    },
    Slice(&'a [u8]),
}

impl Source<'_> {
    fn len(&self) -> u64 {
        match self {
            Source::File { len, .. } => *len,
            Source::Slice(b) => b.len() as u64,
        }
    }

    /// Exactly `len` bytes at `offset`, or [`PeError::Truncated`].
    fn read(&self, offset: u64, len: usize) -> Result<Vec<u8>, PeError> {
        let end = offset.checked_add(len as u64).ok_or(PeError::Truncated)?;
        if end > self.len() {
            return Err(PeError::Truncated);
        }
        let data = match self {
            Source::File { fs, path, .. } => fs.read_range(path, offset, len)?,
            Source::Slice(b) => {
                let start = usize::try_from(offset).map_err(|_| PeError::Truncated)?;
                b[start..start + len].to_vec()
            }
        };
        if data.len() != len {
            // The file shrank while it was read.
            return Err(PeError::Truncated);
        }
        Ok(data)
    }
}

#[derive(Clone, Copy)]
struct Budget<'a> {
    clock: Option<&'a dyn Clock>,
    until: MonoInstant,
}

impl Budget<'_> {
    fn check(&self) -> Result<(), PeError> {
        match self.clock {
            Some(c) if c.mono() >= self.until => Err(PeError::DeadlineExceeded),
            _ => Ok(()),
        }
    }
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

// ---- Headers -----------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
struct Section {
    name: [u8; 8],
    va: u32,
    vsize: u32,
    raw_ptr: u32,
    raw_size: u32,
}

#[derive(Clone, Debug)]
struct Headers {
    machine: u16,
    timestamp: u32,
    characteristics: u16,
    subsystem: u16,
    size_of_headers: u32,
    dirs: Vec<(u32, u32)>,
    sections: Vec<Section>,
}

const DIR_IMPORT: usize = 1;
const DIR_RESOURCE: usize = 2;
const DIR_DELAY_IMPORT: usize = 13;
const DIR_CLR: usize = 14;

impl Headers {
    fn dir(&self, i: usize) -> Option<(u32, u32)> {
        self.dirs.get(i).copied().filter(|(rva, _)| *rva != 0)
    }

    /// File offset of `rva` and the number of file bytes available from there within its
    /// section (or the headers).
    fn rva_to_offset(&self, rva: u32) -> Option<(u64, u64)> {
        if rva < self.size_of_headers {
            return Some((u64::from(rva), u64::from(self.size_of_headers - rva)));
        }
        for s in &self.sections {
            let span = s.vsize.max(s.raw_size);
            if rva >= s.va && rva - s.va < span {
                let delta = rva - s.va;
                if delta >= s.raw_size {
                    return None; // uninitialised data: not in the file
                }
                return Some((
                    u64::from(s.raw_ptr) + u64::from(delta),
                    u64::from(s.raw_size - delta),
                ));
            }
        }
        None
    }

    fn section_of(&self, rva: u32) -> Option<&Section> {
        self.sections
            .iter()
            .find(|s| rva >= s.va && rva - s.va < s.vsize.max(s.raw_size))
    }
}

struct Reader<'a> {
    src: Source<'a>,
    limits: &'a PeLimits,
    budget: Budget<'a>,
}

impl<'a> Reader<'a> {
    fn new(src: Source<'a>, limits: &'a PeLimits, budget: Budget<'a>) -> Self {
        Self {
            src,
            limits,
            budget,
        }
    }

    /// `(format, e_lfanew)` from the DOS header.
    fn dos(&self) -> Result<(ExeFormat, u64), PeError> {
        if self.src.len() < 2 {
            return Err(PeError::NotExecutable);
        }
        let mz = self.src.read(0, 2)?;
        if mz != b"MZ" {
            return Err(PeError::NotExecutable);
        }
        if self.src.len() < 0x40 {
            return Ok((ExeFormat::DosOnly, 0));
        }
        let dos = self.src.read(0, 0x40)?;
        let lfanew = u64::from(u32_at(&dos, 0x3c).unwrap_or(0));
        if lfanew < 0x40 || lfanew.saturating_add(4) > self.src.len() {
            return Ok((ExeFormat::DosOnly, lfanew));
        }
        let sig = self.src.read(lfanew, 4)?;
        let format = match &sig[..] {
            b"PE\0\0" => ExeFormat::Pe,
            [b'N', b'E', ..] => ExeFormat::Ne,
            [b'L', b'E', ..] | [b'L', b'X', ..] => ExeFormat::Le,
            _ => ExeFormat::DosOnly,
        };
        Ok((format, lfanew))
    }

    fn format(&self) -> Result<ExeFormat, PeError> {
        self.dos().map(|(f, _)| f)
    }

    fn headers(&self) -> Result<Headers, PeError> {
        let (format, lfanew) = self.dos()?;
        if format != ExeFormat::Pe {
            return Err(PeError::NotPe(format));
        }
        let coff = self.src.read(lfanew + 4, 20)?;
        let machine = u16_at(&coff, 0).ok_or(PeError::Truncated)?;
        let nsections = usize::from(u16_at(&coff, 2).ok_or(PeError::Truncated)?);
        let timestamp = u32_at(&coff, 4).ok_or(PeError::Truncated)?;
        let opt_size = usize::from(u16_at(&coff, 16).ok_or(PeError::Truncated)?);
        let characteristics = u16_at(&coff, 18).ok_or(PeError::Truncated)?;
        if nsections > MAX_SECTIONS {
            return Err(PeError::Malformed("too many sections"));
        }
        let opt_off = lfanew + 24;
        let opt = self.src.read(opt_off, opt_size)?;
        let magic = u16_at(&opt, 0).ok_or(PeError::Malformed("no optional header"))?;
        let (dirs_count_off, dirs_off) = match magic {
            0x10b => (92, 96),
            0x20b => (108, 112),
            _ => return Err(PeError::Malformed("unknown optional header magic")),
        };
        let subsystem = u16_at(&opt, 68).ok_or(PeError::Truncated)?;
        let size_of_headers = u32_at(&opt, 60).ok_or(PeError::Truncated)?;
        let ndirs = u32_at(&opt, dirs_count_off).unwrap_or(0).min(16) as usize;
        let mut dirs = Vec::with_capacity(ndirs);
        for i in 0..ndirs {
            match (
                u32_at(&opt, dirs_off + i * 8),
                u32_at(&opt, dirs_off + i * 8 + 4),
            ) {
                (Some(rva), Some(size)) => dirs.push((rva, size)),
                _ => break,
            }
        }
        let table = self.src.read(opt_off + opt_size as u64, nsections * 40)?;
        let sections = table
            .as_chunks::<40>()
            .0
            .iter()
            .map(|s| Section {
                name: s[..8].try_into().unwrap_or([0; 8]),
                vsize: u32_at(s, 8).unwrap_or(0),
                va: u32_at(s, 12).unwrap_or(0),
                raw_size: u32_at(s, 16).unwrap_or(0),
                raw_ptr: u32_at(s, 20).unwrap_or(0),
            })
            .collect();
        Ok(Headers {
            machine,
            timestamp,
            characteristics,
            subsystem,
            size_of_headers: size_of_headers.min(64 * 1024),
            dirs,
            sections,
        })
    }

    /// The resource directory as an owned buffer starting at its RVA, and whether the read
    /// was cut by `max_map_bytes`.
    fn resource_buffer(&self, h: &Headers) -> Result<Option<(Vec<u8>, u32, bool)>, PeError> {
        let Some((rva, _size)) = h.dir(DIR_RESOURCE) else {
            return Ok(None);
        };
        let Some((off, avail)) = h.rva_to_offset(rva) else {
            return Err(PeError::Malformed("resource directory outside the file"));
        };
        let avail = avail.min(self.src.len().saturating_sub(off));
        let cap = self.limits.max_map_bytes.min(usize::MAX as u64);
        let truncated = avail > cap;
        let len = usize::try_from(avail.min(cap)).map_err(|_| PeError::Limit("resources"))?;
        self.budget.check()?;
        let buf = self.src.read(off, len)?;
        self.budget.check()?;
        Ok(Some((buf, rva, truncated)))
    }

    fn inspect(&self) -> Result<PeInfo, PeError> {
        let h = self.headers()?;
        self.budget.check()?;
        let mut info = PeInfo {
            machine: match h.machine {
                0x14c => PeMachine::X86,
                0x8664 => PeMachine::X86_64,
                0xaa64 => PeMachine::Arm64,
                _ => PeMachine::Other,
            },
            subsystem: match h.subsystem {
                2 => PeSubsystem::Gui,
                3 => PeSubsystem::Console,
                _ => PeSubsystem::Other,
            },
            timestamp: (h.timestamp != 0).then_some(h.timestamp),
            is_dll: h.characteristics & 0x2000 != 0,
            is_dotnet: h.dir(DIR_CLR).is_some(),
            ..PeInfo::default()
        };
        let mut notes: Vec<&'static str> = Vec::new();
        if info.is_dll {
            notes.push(note::DLL);
        }
        if h.subsystem == 1 {
            notes.push(note::DRIVER);
        } else if info.subsystem == PeSubsystem::Other {
            notes.push(note::UNUSUAL_SUBSYSTEM);
        }
        if info.is_dotnet {
            notes.push(note::DOTNET);
        }
        if !matches!(info.machine, PeMachine::X86 | PeMachine::X86_64) {
            notes.push(note::UNUSUAL_MACHINE);
        }
        if h.sections.iter().any(|s| s.name.starts_with(b".bind")) {
            notes.push(note::STEAM_DRM);
        }
        match self.resource_buffer(&h) {
            Ok(Some((buf, rva, truncated))) => {
                if truncated {
                    notes.push(note::RESOURCES_TRUNCATED);
                }
                self.read_resources(&buf, rva, &mut info, &mut notes)?;
            }
            Ok(None) => {}
            Err(PeError::DeadlineExceeded) => return Err(PeError::DeadlineExceeded),
            Err(_) => notes.push(note::RESOURCES_UNREADABLE),
        }
        info.notes = notes.into_iter().map(str::to_owned).collect();
        Ok(info)
    }

    fn read_resources(
        &self,
        buf: &[u8],
        rva: u32,
        info: &mut PeInfo,
        notes: &mut Vec<&'static str>,
    ) -> Result<(), PeError> {
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: rva,
            Size: u32::try_from(buf.len()).unwrap_or(u32::MAX),
        };
        let res = Resources::new(buf, &dir);
        match walk_resources(res, self.limits, &self.budget) {
            Ok(()) => {}
            Err(PeError::DeadlineExceeded) => return Err(PeError::DeadlineExceeded),
            Err(PeError::Limit(_)) => {
                notes.push(note::RESOURCES_OVER_LIMIT);
                return Ok(());
            }
            Err(_) => {
                notes.push(note::RESOURCES_UNREADABLE);
                return Ok(());
            }
        }
        let Ok(root) = res.root() else {
            notes.push(note::RESOURCES_UNREADABLE);
            return Ok(());
        };
        info.has_icon = root
            .get_dir(Name::GROUP_ICON)
            .is_ok_and(|d| d.entries().next().is_some());
        if let Ok(vdir) = root.get_dir(Name::VERSION) {
            let bytes = vdir
                .first_dir()
                .and_then(|d| d.first_data())
                .and_then(|d| d.bytes().map_err(Into::into));
            match bytes.ok().and_then(|b| VersionInfo::try_from(b).ok()) {
                Some(vi) => self.version_strings(vi, info),
                None => notes.push(note::VERSION_UNREADABLE),
            }
        }
        Ok(())
    }

    fn version_strings(&self, vi: VersionInfo<'_>, info: &mut PeInfo) {
        let preferred = vi.translation().first().copied();
        let mut v = Strings {
            tables: Vec::new(),
            current: None,
        };
        vi.visit(&mut v);
        let table = v
            .tables
            .iter()
            .find(|t| preferred.is_some() && t.lang == preferred)
            .or_else(|| v.tables.first());
        let Some(t) = table else { return };
        let max = self.limits.max_string_bytes;
        let get = |k: usize| t.values[k].and_then(|w| sanitize_utf16(w, max));
        info.product_name = get(0);
        info.file_description = get(1);
        info.company = get(2);
        info.original_filename = get(3);
        info.file_version = get(4);
    }

    fn largest_icon(&self) -> Result<Option<IconBytes>, PeError> {
        let h = self.headers()?;
        let Some((buf, rva, _)) = self.resource_buffer(&h)? else {
            return Ok(None);
        };
        let dir = IMAGE_DATA_DIRECTORY {
            VirtualAddress: rva,
            Size: u32::try_from(buf.len()).unwrap_or(u32::MAX),
        };
        let res = Resources::new(&buf, &dir);
        walk_resources(res, self.limits, &self.budget)?;
        let mut best: Option<(u64, u16, Vec<u8>, u32, u32)> = None;
        for group in res.icons().take(MAX_ICON_GROUPS) {
            self.budget.check()?;
            let Ok((_, group)) = group else { continue };
            for e in group.entries() {
                let w = if e.bWidth == 0 {
                    256
                } else {
                    u32::from(e.bWidth)
                };
                let hgt = if e.bHeight == 0 {
                    256
                } else {
                    u32::from(e.bHeight)
                };
                let area = u64::from(w) * u64::from(hgt);
                let better = best
                    .as_ref()
                    .is_none_or(|(a, bits, ..)| (area, e.wBitCount) > (*a, *bits));
                if !better {
                    continue;
                }
                let Ok(img) = group.image(e.nId) else {
                    continue;
                };
                if img.len() > self.limits.max_icon_bytes || img.is_empty() {
                    continue;
                }
                best = Some((area, e.wBitCount, img.to_vec(), w, hgt));
            }
        }
        Ok(best.map(|(_, bits, img, w, hgt)| IconBytes {
            ico: build_ico(&img, w, hgt, bits),
            width: w,
            height: hgt,
            bit_count: bits,
        }))
    }

    fn imports(&self) -> Result<Vec<String>, PeError> {
        let h = self.headers()?;
        let mut names = Vec::new();
        for (dir, stride, name_off) in [(DIR_IMPORT, 20usize, 12usize), (DIR_DELAY_IMPORT, 32, 4)] {
            let Some((rva, _)) = h.dir(dir) else { continue };
            let Some(sec) = h.section_of(rva).copied() else {
                continue;
            };
            let len = (sec.raw_size as usize).min(IMPORT_SECTION_MAX_BYTES);
            if u64::from(sec.raw_ptr) >= self.src.len() {
                continue;
            }
            let len = len.min((self.src.len() - u64::from(sec.raw_ptr)) as usize);
            self.budget.check()?;
            let data = self.src.read(u64::from(sec.raw_ptr), len)?;
            let at = |r: u32| -> Option<usize> {
                let d = r.checked_sub(sec.va)? as usize;
                (d < data.len()).then_some(d)
            };
            let Some(mut pos) = at(rva) else { continue };
            loop {
                if names.len() >= MAX_IMPORTS {
                    break;
                }
                let Some(desc) = data.get(pos..pos + stride) else {
                    break;
                };
                if desc.iter().all(|b| *b == 0) {
                    break;
                }
                pos += stride;
                let Some(name_rva) = u32_at(desc, name_off) else {
                    break;
                };
                let Some(start) = at(name_rva) else { continue };
                let tail = &data[start..data.len().min(start + MAX_DLL_NAME + 1)];
                let Some(end) = tail.iter().position(|b| *b == 0) else {
                    continue;
                };
                let raw = &tail[..end];
                if raw.is_empty() || !raw.iter().all(|b| b.is_ascii_graphic() || *b == b' ') {
                    continue;
                }
                names.push(String::from_utf8_lossy(raw).to_ascii_lowercase());
            }
        }
        names.sort();
        names.dedup();
        Ok(names)
    }
}

/// Count the resource tree depth-first with the caps of `limits`. A directory entry that
/// points back to an ancestor is caught by the depth cap; an exploding fan-out by the entry
/// cap.
fn walk_resources(
    res: Resources<'_>,
    limits: &PeLimits,
    budget: &Budget<'_>,
) -> Result<(), PeError> {
    fn walk(
        dir: Directory<'_>,
        depth: u8,
        limits: &PeLimits,
        budget: &Budget<'_>,
        seen: &mut u32,
    ) -> Result<(), PeError> {
        if depth > limits.max_resource_depth {
            return Err(PeError::Limit("resource depth"));
        }
        for e in dir.entries() {
            *seen += 1;
            if *seen > limits.max_resource_entries {
                return Err(PeError::Limit("resource entries"));
            }
            if seen.is_multiple_of(256) {
                budget.check()?;
            }
            if e.is_dir()
                && let Ok(Entry::Directory(d)) = e.entry()
            {
                walk(d, depth + 1, limits, budget, seen)?;
            }
        }
        Ok(())
    }
    let root = res
        .root()
        .map_err(|_| PeError::Malformed("resource root"))?;
    let mut seen = 0;
    walk(root, 1, limits, budget, &mut seen)
}

const KEYS: [&str; 5] = [
    "ProductName",
    "FileDescription",
    "CompanyName",
    "OriginalFilename",
    "FileVersion",
];
const MAX_STRING_TABLES: usize = 8;

struct Table<'a> {
    lang: Option<Language>,
    values: [Option<&'a [u16]>; 5],
}

struct Strings<'a> {
    tables: Vec<Table<'a>>,
    current: Option<usize>,
}

impl<'a> Visit<'a> for Strings<'a> {
    fn string_table(&mut self, lang: &'a [u16]) -> bool {
        if self.tables.len() >= MAX_STRING_TABLES {
            self.current = None;
            return false;
        }
        self.tables.push(Table {
            lang: Language::parse(lang).ok(),
            values: [None; 5],
        });
        self.current = Some(self.tables.len() - 1);
        true
    }

    fn string(&mut self, key: &'a [u16], value: &'a [u16]) {
        let Some(i) = self.current else { return };
        let Some(k) = KEYS
            .iter()
            .position(|k| k.encode_utf16().eq(key.iter().copied()))
        else {
            return;
        };
        let slot = &mut self.tables[i].values[k];
        if slot.is_none() {
            *slot = Some(value);
        }
    }
}

/// A single-image ICO file.
fn build_ico(img: &[u8], w: u32, h: u32, bits: u16) -> Vec<u8> {
    let mut out = Vec::with_capacity(22 + img.len());
    out.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    out.push(if w >= 256 { 0 } else { w as u8 });
    out.push(if h >= 256 { 0 } else { h as u8 });
    out.push(0); // colour count
    out.push(0); // reserved
    out.extend_from_slice(&1u16.to_le_bytes()); // planes
    out.extend_from_slice(&bits.to_le_bytes());
    out.extend_from_slice(&(img.len() as u32).to_le_bytes());
    out.extend_from_slice(&22u32.to_le_bytes());
    out.extend_from_slice(img);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitising() {
        let w: Vec<u16> = "  Hollow\u{202E}\u{0007} Knight \n"
            .encode_utf16()
            .collect();
        assert_eq!(sanitize_utf16(&w, 256).as_deref(), Some("Hollow Knight"));
        assert_eq!(
            sanitize_utf16(&[0xD800, 0x41], 256).as_deref(),
            Some("\u{FFFD}A")
        );
        assert_eq!(sanitize_utf16(&[], 256), None);
        assert_eq!(sanitize_str("ééé", 3).as_deref(), Some("é"));
        assert_eq!(sanitize_str("   ", 3), None);
    }

    #[test]
    fn non_executables() {
        assert_eq!(format_bytes(b""), Err(PeError::NotExecutable));
        assert_eq!(format_bytes(b"ELF"), Err(PeError::NotExecutable));
        assert_eq!(format_bytes(b"MZ"), Ok(ExeFormat::DosOnly));
        let mut ne = vec![0u8; 0x80];
        ne[..2].copy_from_slice(b"MZ");
        ne[0x3c] = 0x40;
        ne[0x40..0x42].copy_from_slice(b"NE");
        assert_eq!(format_bytes(&ne), Ok(ExeFormat::Ne));
        assert_eq!(
            inspect_bytes(&ne, &PeLimits::default()),
            Err(PeError::NotPe(ExeFormat::Ne))
        );
    }
}

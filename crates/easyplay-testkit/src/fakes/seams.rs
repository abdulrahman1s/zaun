//! Fakes for the remaining seams of ARCHITECTURE §7 (TEST-001): host probe, PE inspector,
//! image codec, MIME registrar and sandbox spawner. Each records its calls and can be
//! scripted to fail, so a use case can be driven through every branch without a process,
//! a disk or a display.
//!
//! The other seams have their fakes beside this file ([`FakeClock`](super::FakeClock),
//! [`MemFs`](super::MemFs), [`FakeProc`](super::FakeProc),
//! [`ScriptedRunner`](super::ScriptedRunner), [`FakeLauncher`](super::FakeLauncher)) or in
//! core itself where they are also used in production paths: `desktop::RecordingNotifier`
//! (`Notifier`), `desktop::instance::InProcessTransport` (`InstanceTransport`),
//! `host::StaticHostFacts` (a fixed `HostProbe`), the in-memory SQLite `Db` (`Store`).
//! Failure injection for `Fs` is [`crate::inject`].
//!
//! ```
//! use std::path::Path;
//! use easyplay_core::identity::pe::{PeError, PeInspector, PeLimits};
//! use easyplay_core::model::PeInfo;
//! use easyplay_testkit::fakes::TablePeInspector;
//!
//! let pe = TablePeInspector::default()
//!     .with("/g/game.exe", Ok(PeInfo::unknown()))
//!     .with("/g/broken.exe", Err(PeError::NotExecutable));
//! let limits = PeLimits::default();
//! assert!(pe.inspect(Path::new("/g/game.exe"), &limits).is_ok());
//! assert_eq!(pe.inspect(Path::new("/g/broken.exe"), &limits), Err(PeError::NotExecutable));
//! assert_eq!(pe.calls().len(), 2);
//! ```

use std::collections::{HashMap, VecDeque};
use std::os::fd::BorrowedFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use easyplay_core::artwork::codec::{
    CodecError, ImageCodec, ImageFormat, ImageInfo, ImageLimits, RgbaImage,
};
use easyplay_core::desktop::mime::{HandlerStatus, MimeRegistrar, MimeType, PreviousHandlers};
use easyplay_core::error::{Error, ErrorCode};
use easyplay_core::host::wayland::ContextRequest;
use easyplay_core::host::{HostFacts, HostProbe, Revalidation};
use easyplay_core::identity::pe::{IconBytes, PeError, PeInspector, PeLimits};
use easyplay_core::launch::PlanSandbox;
use easyplay_core::model::PeInfo;
use easyplay_core::session::{SandboxChild, SandboxIo, SandboxSpawner, SoundServer};
use parking_lot::Mutex;

// ---- HostProbe ---------------------------------------------------------------------------

/// A host probe whose facts change on [`ScriptedHostProbe::refresh`] as scripted ("the host
/// changes between plan and spawn": `bwrap` removed, display gone).
pub struct ScriptedHostProbe {
    current: Mutex<Arc<HostFacts>>,
    next: Mutex<VecDeque<Arc<HostFacts>>>,
    revalidations: Mutex<VecDeque<Revalidation>>,
    calls: Mutex<Vec<&'static str>>,
}

impl ScriptedHostProbe {
    /// Start with `facts`.
    pub fn new(facts: HostFacts) -> Self {
        Self {
            current: Mutex::new(Arc::new(facts)),
            next: Mutex::new(VecDeque::new()),
            revalidations: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The facts the next [`HostProbe::refresh`] returns (in order).
    pub fn then(self, facts: HostFacts) -> Self {
        self.next.lock().push_back(Arc::new(facts));
        self
    }

    /// The next [`HostProbe::revalidate`] reports `changed` (in order; default: valid).
    pub fn revalidation(self, changed: &[&str]) -> Self {
        self.revalidations.lock().push_back(Revalidation {
            changed: changed.iter().map(|s| (*s).to_owned()).collect(),
        });
        self
    }

    /// `facts`, `refresh` and `revalidate` calls in order.
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().clone()
    }
}

impl HostProbe for ScriptedHostProbe {
    fn facts(&self) -> Arc<HostFacts> {
        self.calls.lock().push("facts");
        Arc::clone(&self.current.lock())
    }
    fn refresh(&self) -> Arc<HostFacts> {
        self.calls.lock().push("refresh");
        if let Some(f) = self.next.lock().pop_front() {
            *self.current.lock() = f;
        }
        Arc::clone(&self.current.lock())
    }
    fn revalidate(&self) -> Revalidation {
        self.calls.lock().push("revalidate");
        self.revalidations.lock().pop_front().unwrap_or_default()
    }
}

// ---- PeInspector -------------------------------------------------------------------------

/// A table-driven PE inspector: results by path; unknown paths are `NotExecutable`.
#[derive(Default)]
pub struct TablePeInspector {
    table: HashMap<PathBuf, Result<PeInfo, PeError>>,
    icons: HashMap<PathBuf, IconBytes>,
    imports: HashMap<PathBuf, Vec<String>>,
    calls: Mutex<Vec<PathBuf>>,
}

impl TablePeInspector {
    /// `inspect(path)` returns `result`.
    pub fn with(mut self, path: impl Into<PathBuf>, result: Result<PeInfo, PeError>) -> Self {
        self.table.insert(path.into(), result);
        self
    }

    /// `largest_icon(path)` returns `icon`.
    pub fn with_icon(mut self, path: impl Into<PathBuf>, icon: IconBytes) -> Self {
        self.icons.insert(path.into(), icon);
        self
    }

    /// `imports(path)` returns `dlls`.
    pub fn with_imports(mut self, path: impl Into<PathBuf>, dlls: &[&str]) -> Self {
        self.imports
            .insert(path.into(), dlls.iter().map(|s| (*s).to_owned()).collect());
        self
    }

    /// Every path asked about, in order.
    pub fn calls(&self) -> Vec<PathBuf> {
        self.calls.lock().clone()
    }

    fn lookup(&self, path: &Path) -> Result<(), PeError> {
        self.calls.lock().push(path.to_owned());
        match self.table.get(path) {
            Some(Err(e)) => Err(*e),
            Some(Ok(_)) => Ok(()),
            None => Err(PeError::NotExecutable),
        }
    }
}

impl PeInspector for TablePeInspector {
    fn inspect(&self, path: &Path, _limits: &PeLimits) -> Result<PeInfo, PeError> {
        self.lookup(path)?;
        self.table
            .get(path)
            .cloned()
            .unwrap_or(Err(PeError::NotExecutable))
    }
    fn largest_icon(&self, path: &Path, _limits: &PeLimits) -> Result<Option<IconBytes>, PeError> {
        self.lookup(path)?;
        Ok(self.icons.get(path).cloned())
    }
    fn imports(&self, path: &Path, _limits: &PeLimits) -> Result<Vec<String>, PeError> {
        self.lookup(path)?;
        Ok(self.imports.get(path).cloned().unwrap_or_default())
    }
}

// ---- ImageCodec --------------------------------------------------------------------------

/// A codec that "decodes" any input to a solid-colour image and encodes to a short marker;
/// scripted failures are returned once each, in order.
pub struct SolidCodec {
    colour: [u8; 4],
    size: (u32, u32),
    failures: Mutex<VecDeque<CodecError>>,
    calls: Mutex<Vec<&'static str>>,
}

impl SolidCodec {
    /// `w × h` images of `colour`.
    pub fn new(colour: [u8; 4], w: u32, h: u32) -> Self {
        Self {
            colour,
            size: (w.max(1), h.max(1)),
            failures: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The next call fails with `e`.
    pub fn fail_next(self, e: CodecError) -> Self {
        self.failures.lock().push_back(e);
        self
    }

    /// `probe`, `decode`, `encode_png`, `encode_jpeg` in order.
    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.lock().clone()
    }

    fn enter(&self, what: &'static str) -> Result<(), CodecError> {
        self.calls.lock().push(what);
        match self.failures.lock().pop_front() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    fn image(&self) -> RgbaImage {
        let (w, h) = self.size;
        RgbaImage {
            width: w,
            height: h,
            pixels: self
                .colour
                .iter()
                .copied()
                .cycle()
                .take((w * h * 4) as usize)
                .collect(),
        }
    }
}

impl ImageCodec for SolidCodec {
    fn probe(&self, bytes: &[u8], limits: &ImageLimits) -> Result<ImageInfo, CodecError> {
        self.enter("probe")?;
        if bytes.len() as u64 > limits.max_bytes {
            return Err(CodecError::TooLarge(format!("{} bytes", bytes.len())));
        }
        Ok(ImageInfo {
            format: ImageFormat::Png,
            width: self.size.0,
            height: self.size.1,
        })
    }
    fn decode(&self, _bytes: &[u8], _limits: &ImageLimits) -> Result<RgbaImage, CodecError> {
        self.enter("decode")?;
        Ok(self.image())
    }
    fn encode_png(&self, image: &RgbaImage) -> Result<Vec<u8>, CodecError> {
        self.enter("encode_png")?;
        Ok(format!("PNG {}x{}", image.width, image.height).into_bytes())
    }
    fn encode_jpeg(&self, image: &RgbaImage, _quality: u8) -> Result<Vec<u8>, CodecError> {
        self.enter("encode_jpeg")?;
        Ok(format!("JPEG {}x{}", image.width, image.height).into_bytes())
    }
}

// ---- MimeRegistrar -----------------------------------------------------------------------

/// A MIME registrar over an in-memory default table.
pub struct RecordingMime {
    types: Vec<MimeType>,
    defaults: Mutex<HashMap<MimeType, Option<String>>>,
    our_id: String,
    fail_next: Mutex<Option<ErrorCode>>,
    calls: Mutex<Vec<String>>,
}

impl RecordingMime {
    /// Manage `types`; EasyPlay's desktop ID is `our_id`; nothing is default yet.
    pub fn new(types: &[&str], our_id: &str) -> Self {
        let types: Vec<MimeType> = types.iter().filter_map(|t| MimeType::parse(t)).collect();
        Self {
            defaults: Mutex::new(types.iter().map(|t| (t.clone(), None)).collect()),
            types,
            our_id: our_id.to_owned(),
            fail_next: Mutex::new(None),
            calls: Mutex::new(Vec::new()),
        }
    }

    /// Another application is the default for `mime`.
    pub fn with_default(self, mime: &str, desktop_id: &str) -> Self {
        if let Some(t) = MimeType::parse(mime) {
            self.defaults.lock().insert(t, Some(desktop_id.to_owned()));
        }
        self
    }

    /// The next `status`/`set_default`/`restore` fails with `code`.
    pub fn fail_next(&self, code: ErrorCode) {
        *self.fail_next.lock() = Some(code);
    }

    /// Calls in order (`status`, `set_default`, `restore`).
    pub fn calls(&self) -> Vec<String> {
        self.calls.lock().clone()
    }

    fn enter(&self, what: &str) -> Result<(), Error> {
        self.calls.lock().push(what.to_owned());
        match self.fail_next.lock().take() {
            Some(code) => Err(Error::new(code)),
            None => Ok(()),
        }
    }
}

impl MimeRegistrar for RecordingMime {
    fn managed_types(&self) -> Vec<MimeType> {
        self.types.clone()
    }
    fn status(&self) -> Result<Vec<HandlerStatus>, Error> {
        self.enter("status")?;
        let d = self.defaults.lock();
        Ok(self
            .types
            .iter()
            .map(|t| HandlerStatus {
                mime: t.clone(),
                default: d.get(t).cloned().flatten(),
                name: d.get(t).cloned().flatten(),
            })
            .collect())
    }
    fn set_default(&self, types: &[MimeType]) -> Result<PreviousHandlers, Error> {
        self.enter("set_default")?;
        let mut d = self.defaults.lock();
        let entries = types
            .iter()
            .map(|t| {
                let before = d.insert(t.clone(), Some(self.our_id.clone())).flatten();
                (t.clone(), before)
            })
            .collect();
        Ok(PreviousHandlers { entries })
    }
    fn restore(&self, previous: &PreviousHandlers) -> Result<(), Error> {
        self.enter("restore")?;
        let mut d = self.defaults.lock();
        for (t, before) in &previous.entries {
            d.insert(t.clone(), before.clone());
        }
        Ok(())
    }
}

// ---- SandboxSpawner ----------------------------------------------------------------------

/// What one scripted spawn does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpawnScript {
    /// `spawn` fails with this code (bwrap missing, `EAGAIN`, …).
    Fail(ErrorCode),
    /// The monitor exits with `code` after `polls` calls of `try_wait`, writing `status`
    /// to the `--json-status-fd` first (empty: exits before any status; garbage: anything).
    Exit {
        /// Exit code.
        code: i32,
        /// `try_wait` calls answered `None` first.
        polls: u32,
        /// Bytes written to the status descriptor at spawn.
        status: Vec<u8>,
    },
}

/// The calls a [`ScriptedSpawner`] saw: `(bwrap path, rendered arguments)`.
pub type SpawnCalls = Arc<Mutex<Vec<(PathBuf, Vec<String>)>>>;

/// A spawner that runs nothing: each spawn follows the next [`SpawnScript`].
pub struct ScriptedSpawner {
    scripts: VecDeque<SpawnScript>,
    calls: SpawnCalls,
    sound: Arc<Mutex<Vec<PathBuf>>>,
    wayland: Arc<Mutex<Vec<PathBuf>>>,
}

impl ScriptedSpawner {
    /// Spawns follow `scripts` in order; when they run out, every spawn fails.
    pub fn new(scripts: impl IntoIterator<Item = SpawnScript>) -> Self {
        Self {
            scripts: scripts.into_iter().collect(),
            calls: Arc::new(Mutex::new(Vec::new())),
            sound: Arc::new(Mutex::new(Vec::new())),
            wayland: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// The socket of every restricted Wayland socket created (nothing is created; the
    /// compositor is never reached), shared with clones of the handle.
    pub fn wayland_contexts(&self) -> Arc<Mutex<Vec<PathBuf>>> {
        Arc::clone(&self.wayland)
    }

    /// The socket of every sound server started, shared with clones of the handle.
    pub fn sound_servers(&self) -> Arc<Mutex<Vec<PathBuf>>> {
        Arc::clone(&self.sound)
    }

    /// `(bwrap path, rendered arguments)` of every spawn, shared with clones of the handle.
    pub fn calls(&self) -> SpawnCalls {
        Arc::clone(&self.calls)
    }
}

struct ScriptedChild {
    code: i32,
    polls: u32,
    killed: bool,
}

impl SandboxChild for ScriptedChild {
    fn monitor_pid(&self) -> u32 {
        // An ID no process of the test has: the fake never runs anything.
        u32::MAX - 1
    }
    fn try_wait(&mut self) -> Result<Option<i32>, Error> {
        if self.killed {
            return Ok(Some(128 + 9));
        }
        if self.polls > 0 {
            self.polls -= 1;
            return Ok(None);
        }
        Ok(Some(self.code))
    }
    fn kill_monitor(&mut self) -> Result<(), Error> {
        self.killed = true;
        Ok(())
    }
}

impl SandboxSpawner for ScriptedSpawner {
    fn spawn(
        &mut self,
        bwrap: &Path,
        sandbox: &PlanSandbox,
        io: SandboxIo,
    ) -> Result<Box<dyn SandboxChild>, Error> {
        self.calls.lock().push((
            bwrap.to_owned(),
            sandbox
                .args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect(),
        ));
        match self.scripts.pop_front() {
            None => Err(Error::new(ErrorCode::SbxSetupFailed).with("detail", "no script left")),
            Some(SpawnScript::Fail(code)) => Err(Error::new(code)),
            Some(SpawnScript::Exit {
                code,
                polls,
                status,
            }) => {
                if !status.is_empty() {
                    use std::io::Write as _;
                    let mut f = std::fs::File::from(io.status);
                    let _ = f.write_all(&status);
                }
                Ok(Box::new(ScriptedChild {
                    code,
                    polls,
                    killed: false,
                }))
            }
        }
    }

    fn start_sound_server(
        &mut self,
        server: SoundServer<'_>,
    ) -> Result<Box<dyn SandboxChild>, Error> {
        self.sound.lock().push(server.socket.to_owned());
        // Runs until the session stops it.
        Ok(Box::new(ScriptedChild {
            code: 0,
            polls: u32::MAX,
            killed: false,
        }))
    }

    fn create_wayland_context(
        &mut self,
        request: ContextRequest<'_>,
        _close_fd: BorrowedFd<'_>,
    ) -> Result<(), Error> {
        self.wayland.lock().push(request.socket.to_owned());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_failures_are_one_shot() {
        let c = SolidCodec::new([1, 2, 3, 255], 4, 2).fail_next(CodecError::Unsupported);
        let l = ImageLimits::DEFAULT;
        assert_eq!(c.decode(b"x", &l), Err(CodecError::Unsupported));
        let img = c.decode(b"x", &l).unwrap();
        assert_eq!((img.width, img.height, img.pixels.len()), (4, 2, 32));
        assert_eq!(c.calls(), vec!["decode", "decode"]);
    }

    #[test]
    fn mime_set_and_restore_round_trip() {
        let m = RecordingMime::new(&["application/x-ms-dos-executable"], "easyplay.desktop")
            .with_default("application/x-ms-dos-executable", "wine.desktop");
        let prev = m.set_default(&m.managed_types()).unwrap();
        assert_eq!(
            m.status().unwrap()[0].default.as_deref(),
            Some("easyplay.desktop")
        );
        m.restore(&prev).unwrap();
        assert_eq!(
            m.status().unwrap()[0].default.as_deref(),
            Some("wine.desktop")
        );
        m.fail_next(ErrorCode::DskDefaultHandlerFailed);
        assert!(m.status().is_err());
    }
}

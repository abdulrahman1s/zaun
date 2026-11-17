//! File and folder pickers. The system dialog (`rfd`) runs on its own thread; the result comes
//! back through a channel polled once per frame, so the UI thread never blocks on it
//! (SCREEN_SPECS §5). Tests use [`FakePicker`].

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use easyplay_core::ids::GameId;
use easyplay_core::model::ArtSlot;

use crate::strings;

/// What is being picked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PickKind {
    /// One or more Windows programs (`*.exe`, `*.msi`).
    Programs,
    /// One folder.
    Folder,
    /// One image.
    Image,
    /// A file to save (the diagnostics report).
    Save,
    /// A picture, GIF or video to send in Chat.
    Media,
    /// Any file to send in Chat.
    File,
}

/// Why it is being picked: where the result goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PickPurpose {
    /// *Choose folder…* in the first step of the *Add a game* dialog: a folder to scan for
    /// its programs, as a folder dropped on the window is.
    AddGameFolder,
    /// *Choose a file instead…* in the second step of the *Add a game* dialog.
    AddGameFile,
    /// *Add folder…* in a folder list (`None`: the setup sheet's draft).
    FolderGrant(Option<GameId>),
    /// *A folder I choose…* for save data.
    SaveFolder(Option<GameId>),
    /// *Choose folder…* for the game folder in setup.
    SetupRoot,
    /// *Choose image…* for an artwork slot.
    Artwork(GameId, ArtSlot),
    /// *Add program…*.
    AddProgram(GameId),
    /// *Run another program…*.
    RunProgram(GameId),
    /// *Locate…* a moved program.
    Locate(GameId),
    /// *Locate…* a granted folder that is missing (the old canonical path).
    RelocateFolder(GameId, PathBuf),
    /// Working folder *Other…*.
    WorkingDir(GameId),
    /// An extra compatibility-layer search folder (Settings).
    LayerFolder,
    /// Where to save the diagnostics report.
    SaveReport,
    /// The Chat composer's *Picture or video…* for this conversation.
    ChatPicture(easyplay_core::model::ChatRef),
    /// The Chat composer's *File…* for this conversation.
    ChatFile(easyplay_core::model::ChatRef),
}

impl PickPurpose {
    /// The kind of dialog this purpose needs.
    pub fn kind(&self) -> PickKind {
        match self {
            PickPurpose::AddGameFile
            | PickPurpose::AddProgram(_)
            | PickPurpose::RunProgram(_)
            | PickPurpose::Locate(_) => PickKind::Programs,
            PickPurpose::Artwork(..) => PickKind::Image,
            PickPurpose::FolderGrant(_)
            | PickPurpose::RelocateFolder(..)
            | PickPurpose::SaveFolder(_)
            | PickPurpose::SetupRoot
            | PickPurpose::AddGameFolder
            | PickPurpose::WorkingDir(_)
            | PickPurpose::LayerFolder => PickKind::Folder,
            PickPurpose::SaveReport => PickKind::Save,
            PickPurpose::ChatPicture(_) => PickKind::Media,
            PickPurpose::ChatFile(_) => PickKind::File,
        }
    }
}

/// A finished pick; `paths` is empty when the user cancelled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickResult {
    /// Why it was asked.
    pub purpose: PickPurpose,
    /// Chosen paths.
    pub paths: Vec<PathBuf>,
}

/// Opens pickers without blocking the caller.
pub trait Picker: Send {
    /// Open a dialog for `purpose`.
    fn pick(&self, purpose: PickPurpose);
    /// A finished pick, if any.
    fn try_result(&self) -> Option<PickResult>;
}

/// The system dialog through `rfd`, one thread per dialog.
pub struct RfdPicker {
    tx: Sender<PickResult>,
    rx: Receiver<PickResult>,
    waker: Arc<dyn Fn() + Send + Sync>,
}

impl RfdPicker {
    /// A picker that calls `waker` when a result is ready.
    pub fn new(waker: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = channel();
        Self { tx, rx, waker }
    }
}

impl Picker for RfdPicker {
    fn pick(&self, purpose: PickPurpose) {
        let tx = self.tx.clone();
        let waker = self.waker.clone();
        let spawned = std::thread::Builder::new()
            .name("easyplay-picker".into())
            .spawn(move || {
                let kind = purpose.kind();
                let dialog = rfd::FileDialog::new();
                let paths: Vec<PathBuf> = match kind {
                    PickKind::Programs => {
                        let d = dialog
                            .add_filter(strings::picker::PROGRAMS(), &["exe", "msi", "EXE", "MSI"]);
                        if purpose == PickPurpose::AddGameFile {
                            d.pick_files().unwrap_or_default()
                        } else {
                            d.pick_file().into_iter().collect()
                        }
                    }
                    PickKind::Image => dialog
                        .add_filter(
                            strings::picker::IMAGES(),
                            &["png", "jpg", "jpeg", "webp", "bmp", "ico"],
                        )
                        .pick_file()
                        .into_iter()
                        .collect(),
                    PickKind::Folder => dialog.pick_folder().into_iter().collect(),
                    PickKind::Media => dialog
                        .add_filter(
                            strings::chat::composer::MEDIA_FILTER(),
                            &[
                                "png", "jpg", "jpeg", "webp", "gif", "mp4", "webm", "mkv", "mov",
                                "PNG", "JPG", "JPEG", "WEBP", "GIF", "MP4", "WEBM", "MKV", "MOV",
                            ],
                        )
                        .pick_file()
                        .into_iter()
                        .collect(),
                    PickKind::File => dialog.pick_file().into_iter().collect(),
                    PickKind::Save => dialog
                        .set_file_name(strings::debug::REPORT_FILE_NAME)
                        .save_file()
                        .into_iter()
                        .collect(),
                };
                let _ = tx.send(PickResult { purpose, paths });
                waker();
            });
        if let Err(e) = spawned {
            tracing::warn!(error = %e, "could not start the file picker thread");
        }
    }

    fn try_result(&self) -> Option<PickResult> {
        self.rx.try_recv().ok()
    }
}

/// A picker for tests: answers each request with the next scripted paths (or a cancel).
#[derive(Clone, Default)]
pub struct FakePicker {
    inner: Arc<Mutex<FakeInner>>,
}

#[derive(Default)]
struct FakeInner {
    script: std::collections::VecDeque<Vec<PathBuf>>,
    asked: Vec<PickPurpose>,
    ready: std::collections::VecDeque<PickResult>,
}

impl FakePicker {
    /// Answer the next request with `paths`.
    pub fn answer(&self, paths: Vec<PathBuf>) {
        self.lock().script.push_back(paths);
    }

    /// Purposes asked so far.
    pub fn asked(&self) -> Vec<PickPurpose> {
        self.lock().asked.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeInner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Picker for FakePicker {
    fn pick(&self, purpose: PickPurpose) {
        let mut s = self.lock();
        s.asked.push(purpose.clone());
        let paths = s.script.pop_front().unwrap_or_default();
        s.ready.push_back(PickResult { purpose, paths });
    }

    fn try_result(&self) -> Option<PickResult> {
        self.lock().ready.pop_front()
    }
}

/// A picker that never answers (no display, previews).
pub struct NoPicker;

impl Picker for NoPicker {
    fn pick(&self, _purpose: PickPurpose) {}
    fn try_result(&self) -> Option<PickResult> {
        None
    }
}

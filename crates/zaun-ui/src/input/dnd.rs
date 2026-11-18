//! Drag and drop (USER_EXPERIENCE §5.4): classify the hovered payload, draw the drop zone,
//! route dropped files to the target under the pointer. Dropping a program never launches
//! it: it goes through `OpenPaths { origin: DragDrop }`, which leads to setup.

use std::path::{Path, PathBuf};

use egui::{Pos2, Rect};
use zaun_core::ids::GameId;

/// What is being dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Payload {
    /// Windows programs.
    Programs(usize),
    /// Folders (scanned for programs, or shared with a game).
    Folders(usize),
    /// Images (artwork).
    Images(usize),
    /// Something Zaun cannot open.
    Unsupported,
    /// Not known yet (some platforms report paths only on drop).
    Unknown,
}

fn ext(p: &Path) -> Option<String> {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
}

/// Whether `p` names a Windows program.
pub fn is_program(p: &Path) -> bool {
    matches!(ext(p).as_deref(), Some("exe" | "msi"))
}

/// Whether `p` names an image Zaun imports.
pub fn is_image(p: &Path) -> bool {
    matches!(
        ext(p).as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "bmp" | "ico")
    )
}

/// Classify paths. A path without an extension is taken for a folder (the UI does no I/O;
/// the service checks).
pub fn classify(paths: &[PathBuf]) -> Payload {
    if paths.is_empty() {
        return Payload::Unknown;
    }
    let n = paths.len();
    if paths.iter().all(|p| is_program(p)) {
        Payload::Programs(n)
    } else if paths.iter().all(|p| is_image(p)) {
        Payload::Images(n)
    } else if paths.iter().all(|p| ext(p).is_none()) {
        Payload::Folders(n)
    } else {
        Payload::Unsupported
    }
}

/// Where a drop can land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropTarget {
    /// The library (programs and folders).
    Library,
    /// A game's card or hero (images become the cover).
    Cover(GameId),
    /// A folder list: Protection (`Some`) or the setup sheet (`None`).
    FolderList(Option<GameId>),
}

/// What a drop does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropAction {
    /// `OpenPaths { origin: DragDrop }` (setup follows; never a launch).
    Open(Vec<PathBuf>),
    /// Scan a folder in the add-game sheet.
    ScanFolder(PathBuf),
    /// Set a game's cover.
    SetCover(GameId, PathBuf),
    /// Share folders with a game (or the draft).
    ShareFolders(Option<GameId>, Vec<PathBuf>),
    /// Refused: "Zaun can't open this kind of file".
    Refuse,
}

/// Route `paths` dropped on `target`.
pub fn route(target: DropTarget, paths: Vec<PathBuf>) -> DropAction {
    match (target, classify(&paths)) {
        (DropTarget::Cover(id), Payload::Images(_)) => {
            DropAction::SetCover(id, paths.into_iter().next().unwrap_or_default())
        }
        (DropTarget::FolderList(id), Payload::Folders(_)) => DropAction::ShareFolders(id, paths),
        (_, Payload::Programs(_)) => DropAction::Open(paths),
        (_, Payload::Folders(1)) => {
            DropAction::ScanFolder(paths.into_iter().next().unwrap_or_default())
        }
        (_, Payload::Folders(_)) => DropAction::Open(paths),
        _ => DropAction::Refuse,
    }
}

/// Drop targets registered by the screens this frame.
#[derive(Clone, Debug, Default)]
pub struct DndState {
    targets: Vec<(Rect, DropTarget)>,
    /// The payload hovering over the window.
    pub hovering: Option<Payload>,
    /// The target under the pointer while hovering.
    pub over: Option<DropTarget>,
}

impl DndState {
    /// Forget last frame's targets.
    pub fn begin_frame(&mut self) {
        self.targets.clear();
    }

    /// Register a target for this frame (later registrations win: they are on top).
    pub fn target(&mut self, rect: Rect, t: DropTarget) {
        self.targets.push((rect, t));
    }

    /// The target at `pos`.
    pub fn target_at(&self, pos: Option<Pos2>) -> DropTarget {
        pos.and_then(|p| {
            self.targets
                .iter()
                .rev()
                .find(|(r, _)| r.contains(p))
                .map(|(_, t)| *t)
        })
        .unwrap_or(DropTarget::Library)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn classifies_payloads() {
        assert_eq!(classify(&[p("/a/Game.EXE")]), Payload::Programs(1));
        assert_eq!(
            classify(&[p("/a/x.png"), p("/a/y.jpg")]),
            Payload::Images(2)
        );
        assert_eq!(classify(&[p("/a/Mods")]), Payload::Folders(1));
        assert_eq!(classify(&[p("/a/readme.txt")]), Payload::Unsupported);
        assert_eq!(classify(&[]), Payload::Unknown);
    }

    /// Dropping an `.exe` never launches it: it always becomes `Open` (setup), even on a card.
    #[test]
    fn programs_go_to_setup_never_launch() {
        let id = GameId::from_uuid(uuid::Uuid::from_u128(1));
        for t in [
            DropTarget::Library,
            DropTarget::Cover(id),
            DropTarget::FolderList(None),
        ] {
            assert_eq!(
                route(t, vec![p("/g/Game.exe")]),
                DropAction::Open(vec![p("/g/Game.exe")])
            );
        }
    }

    #[test]
    fn routes_by_target() {
        let id = GameId::from_uuid(uuid::Uuid::from_u128(1));
        assert_eq!(
            route(DropTarget::Cover(id), vec![p("/c.png")]),
            DropAction::SetCover(id, p("/c.png"))
        );
        assert_eq!(
            route(DropTarget::FolderList(Some(id)), vec![p("/home/sam/Mods")]),
            DropAction::ShareFolders(Some(id), vec![p("/home/sam/Mods")])
        );
        assert_eq!(
            route(DropTarget::Library, vec![p("/home/sam/Games/Celeste")]),
            DropAction::ScanFolder(p("/home/sam/Games/Celeste"))
        );
        assert_eq!(
            route(DropTarget::Library, vec![p("/x.png")]),
            DropAction::Refuse
        );
    }

    #[test]
    fn later_targets_win() {
        let mut s = DndState::default();
        let id = GameId::from_uuid(uuid::Uuid::from_u128(1));
        s.target(
            Rect::from_min_max(Pos2::ZERO, Pos2::new(100.0, 100.0)),
            DropTarget::Library,
        );
        s.target(
            Rect::from_min_max(Pos2::ZERO, Pos2::new(10.0, 10.0)),
            DropTarget::Cover(id),
        );
        assert_eq!(
            s.target_at(Some(Pos2::new(5.0, 5.0))),
            DropTarget::Cover(id)
        );
        assert_eq!(
            s.target_at(Some(Pos2::new(50.0, 50.0))),
            DropTarget::Library
        );
        assert_eq!(s.target_at(None), DropTarget::Library);
    }
}

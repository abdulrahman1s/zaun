//! Setup sheet view-model (SCREEN_SPECS §6, LOGIC_SPECS §2, UF-2): the draft being edited,
//! its assessment, the disclosure and acknowledgement state, and the launch that follows
//! *Play*.

use std::path::PathBuf;

use easyplay_core::ids::GameId;
use easyplay_core::library::GameDraft;
use easyplay_core::model::{BasePreset, EntryKind, validate_name};
use easyplay_core::sandbox::{IssueSeverity, classify};

use crate::bridge::{PolicyAssessment, SetupContext, SetupOrigin};

/// Where the sheet is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupPhase {
    /// Editing the draft.
    Editing,
    /// *Play* was pressed: waiting for the new game's launch (matched by program path).
    Launching {
        /// The game, once known.
        game: Option<GameId>,
    },
}

/// The setup sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct SetupVm {
    /// The draft as built by the service.
    pub original: GameDraft,
    /// Being edited.
    pub draft: GameDraft,
    /// Context (origin, batch position).
    pub context: SetupContext,
    /// Assessment of the draft's policy.
    pub assessment: PolicyAssessment,
    /// *More options* expanded.
    pub more: bool,
    /// Broad-folder warnings acknowledged ("I understand").
    pub acknowledged: bool,
    /// Phase.
    pub phase: SetupPhase,
    /// The name field.
    pub name: String,
    /// The arguments field (More options → Launch).
    pub args: String,
    /// The last rejected folder, explained.
    pub folder_error: Option<String>,
    /// Give the primary button focus when next drawn (initial focus).
    pub focus_primary: bool,
}

impl SetupVm {
    /// A sheet for `draft`.
    pub fn new(draft: GameDraft, context: SetupContext, assessment: PolicyAssessment) -> Self {
        Self {
            name: draft.name.clone(),
            args: easyplay_core::model::join_args(&draft.launch.args),
            original: draft.clone(),
            draft,
            context,
            assessment,
            more: false,
            acknowledged: false,
            phase: SetupPhase::Editing,
            folder_error: None,
            focus_primary: true,
        }
    }

    /// The primary button plays (file manager) or adds (inside EasyPlay); installers run.
    pub fn primary_plays(&self) -> bool {
        self.context.origin == SetupOrigin::FileManager || self.is_installer()
    }

    /// The draft is an installer.
    pub fn is_installer(&self) -> bool {
        self.draft.kind == EntryKind::Installer
    }

    /// The chosen base preset.
    pub fn base(&self) -> BasePreset {
        self.draft.policy.preset.base()
    }

    /// The draft differs from the preset in some field ("Reset to preset" link).
    pub fn customised(&self) -> bool {
        self.draft.policy.preset.is_custom()
    }

    /// Re-derive the preset label after an edit.
    pub fn reclassify(&mut self) {
        self.draft.policy.preset = classify(&self.draft.policy);
    }

    /// The name problem, if any.
    pub fn name_error(&self) -> Option<easyplay_core::model::Problem> {
        validate_name(self.name.trim()).err().map(|e| e.problem)
    }

    /// Blocking issues.
    pub fn errors(&self) -> usize {
        self.assessment
            .issues
            .iter()
            .filter(|i| i.severity >= IssueSeverity::Error)
            .count()
    }

    /// Warnings that need "I understand".
    pub fn warnings(&self) -> usize {
        self.assessment
            .issues
            .iter()
            .filter(|i| i.severity == IssueSeverity::Warning)
            .count()
    }

    /// Whether the primary button is enabled.
    pub fn can_continue(&self) -> bool {
        self.name_error().is_none()
            && self.errors() == 0
            && (self.warnings() == 0 || self.acknowledged)
            && easyplay_core::model::split_args(&self.args).is_ok()
    }

    /// The draft to send, with the typed name and arguments.
    pub fn finished(&self) -> GameDraft {
        let mut d = self.draft.clone();
        let name = self.name.trim().to_owned();
        if name != d.name {
            d.name_is_user_set = true;
        }
        d.name = name;
        if let Ok(args) = easyplay_core::model::split_args(&self.args) {
            d.launch.args = args;
        }
        d
    }

    /// Choose another game folder (`None`: this file only).
    pub fn set_root(&mut self, root: Option<PathBuf>) {
        self.draft.policy.game_dir = easyplay_core::library::draft::game_dir_for(root.as_deref());
        self.draft.root = root;
        self.reclassify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use std::path::Path;

    fn vm(origin: SetupOrigin) -> SetupVm {
        let d = demo::draft(
            Path::new("/home/sam/Games/Celeste/Celeste.exe"),
            demo::baseline_now(),
        );
        let a = demo::assess(&d.name, d.root.as_deref(), &d.policy);
        SetupVm::new(
            d,
            SetupContext {
                origin,
                queue: (1, 1),
            },
            a,
        )
    }

    #[test]
    fn primary_depends_on_origin() {
        assert!(vm(SetupOrigin::FileManager).primary_plays());
        assert!(!vm(SetupOrigin::InsideApp).primary_plays());
    }

    #[test]
    fn edits_are_validated() {
        let mut v = vm(SetupOrigin::InsideApp);
        assert!(v.can_continue());
        v.name = "   ".into();
        assert!(!v.can_continue());
        v.name = "Celeste".into();
        v.args = "\"unbalanced".into();
        assert!(!v.can_continue());
        v.args.clear();
        v.set_root(None);
        assert_eq!(
            v.draft.policy.game_dir,
            easyplay_core::model::GameDirAccess::FileOnly
        );
        assert!(v.finished().root.is_none());
    }
}

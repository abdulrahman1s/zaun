//! Game page view-model (SCREEN_SPECS §4, SCREEN_SPECS_2 §8–11): the loaded record, the
//! current tab, inline rename, notes autosave, and one draft per editable tab with dirty
//! tracking. Statements and issues come from `Backend::assess`, recomputed on change only.

use std::path::PathBuf;

use easyplay_core::clock::{MonoInstant, Timestamp};
use easyplay_core::error::UserError;
use easyplay_core::ids::{GameId, QueryId};
use easyplay_core::model::{
    BasePreset, EnvVar, FolderAccess, FolderGrant, GrantOrigin, LaunchConfig, Preset,
    SandboxPolicy, UserDir, join_args, split_args,
};
use easyplay_core::sandbox::{IssueSeverity, Widening, classify, widening};
use easyplay_core::stats::{DateRange, GameOverview, HistoryRow};

use super::AsyncValue;
use crate::bridge::{FixEntry, GameDetail, GameEdit, PolicyAssessment};

/// Game page tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DetailsTab {
    /// Overview.
    #[default]
    Overview,
    /// Protection (and folder permissions).
    Protection,
    /// Compatibility.
    Compatibility,
    /// Launch options.
    LaunchOptions,
    /// Activity.
    Activity,
    /// Logs (Developer Mode, SCREEN_SPECS_2 §15).
    Logs,
}

impl DetailsTab {
    /// In display order.
    pub const ALL: [DetailsTab; 6] = [
        DetailsTab::Overview,
        DetailsTab::Protection,
        DetailsTab::Compatibility,
        DetailsTab::LaunchOptions,
        DetailsTab::Activity,
        DetailsTab::Logs,
    ];

    /// The tabs shown: Logs only in Developer Mode.
    pub fn visible(developer: bool) -> &'static [DetailsTab] {
        if developer {
            &Self::ALL
        } else {
            &Self::ALL[..5]
        }
    }

    /// Index in [`Self::ALL`].
    pub fn index(self) -> usize {
        Self::ALL.iter().position(|t| *t == self).unwrap_or(0)
    }
}

/// The Protection tab's draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PolicyEditor {
    /// As saved.
    pub saved: SandboxPolicy,
    /// Being edited.
    pub draft: SandboxPolicy,
    /// Assessment of the draft.
    pub assessment: PolicyAssessment,
    /// Statement texts of the saved policy (for "added"/"removed" markers).
    pub saved_texts: Vec<String>,
    /// Advanced disclosure.
    pub advanced: bool,
    /// The last rejected folder, explained.
    pub folder_error: Option<String>,
    /// A preset choice waiting for the inline confirmation (it replaces custom edits).
    pub confirm_preset: Option<BasePreset>,
}

impl PolicyEditor {
    /// Start from `saved` with its assessment.
    pub fn new(saved: SandboxPolicy, assessment: PolicyAssessment) -> Self {
        Self {
            saved_texts: assessment
                .statements
                .iter()
                .map(|s| s.text.clone())
                .collect(),
            draft: saved.clone(),
            saved,
            assessment,
            advanced: false,
            folder_error: None,
            confirm_preset: None,
        }
    }

    /// Whether the draft differs.
    pub fn dirty(&self) -> bool {
        self.draft != self.saved
    }

    /// Recompute the derived preset label after an edit.
    pub fn reclassify(&mut self) {
        self.draft.preset = classify(&self.draft);
    }

    /// Blocking problems (`Error` and worse).
    pub fn errors(&self) -> usize {
        self.assessment
            .issues
            .iter()
            .filter(|i| i.severity >= IssueSeverity::Error)
            .count()
    }

    /// How the draft widens the saved policy.
    pub fn widenings(&self) -> Vec<Widening> {
        widening(&self.saved, &self.draft)
    }

    /// Whether a statement is new relative to the saved policy.
    pub fn is_added(&self, text: &str) -> bool {
        self.dirty() && !self.saved_texts.iter().any(|t| t == text)
    }

    /// Statements of the saved policy that the draft no longer has.
    pub fn removed(&self) -> Vec<&str> {
        if !self.dirty() {
            return Vec::new();
        }
        self.saved_texts
            .iter()
            .filter(|t| !self.assessment.statements.iter().any(|s| &s.text == *t))
            .map(String::as_str)
            .collect()
    }

    /// Toggle a well-known folder (read-only grant).
    pub fn toggle_user_dir(&mut self, d: UserDir, path: PathBuf, now: Timestamp) {
        if let Some(i) = self
            .draft
            .folders
            .iter()
            .position(|g| g.origin == GrantOrigin::WellKnown(d))
        {
            self.draft.folders.remove(i);
        } else {
            self.draft.folders.push(FolderGrant {
                canonical: path.clone(),
                path,
                access: FolderAccess::ReadOnly,
                origin: GrantOrigin::WellKnown(d),
                drive_letter: None,
                granted_at: now,
            });
        }
        self.reclassify();
    }

    /// Add a user-chosen folder (read-only by default). Returns false when it is already
    /// in the list.
    pub fn add_folder(&mut self, path: PathBuf, now: Timestamp) -> bool {
        if self.draft.folders.iter().any(|g| g.canonical == path) {
            return false;
        }
        self.draft.folders.push(FolderGrant {
            canonical: path.clone(),
            path,
            access: FolderAccess::ReadOnly,
            origin: GrantOrigin::UserChosen,
            drive_letter: None,
            granted_at: now,
        });
        self.reclassify();
        true
    }
}

/// The Compatibility tab's draft: layer, fixes and options (a slice of the game record).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompatDraft {
    /// Layer.
    pub runtime: easyplay_core::model::RuntimeRef,
    /// Follow updates.
    pub follow: bool,
    /// Fix entry.
    pub umu_id: Option<String>,
    /// Options (part of the launch configuration).
    pub compat: easyplay_core::model::CompatOptions,
}

/// The Launch options tab's draft.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchDraft {
    /// The configuration being edited (arguments come from `args_text`).
    pub config: LaunchConfig,
    /// The arguments as typed.
    pub args_text: String,
}

impl LaunchDraft {
    /// From a saved configuration.
    pub fn new(config: LaunchConfig) -> Self {
        Self {
            args_text: join_args(&config.args),
            config,
        }
    }

    /// The configuration with the typed arguments, or the split error.
    pub fn resolved(&self) -> Result<LaunchConfig, easyplay_core::model::SplitError> {
        let args = split_args(&self.args_text)?;
        Ok(LaunchConfig {
            args,
            ..self.config.clone()
        })
    }
}

/// Activity tab and Overview playtime card.
#[derive(Clone, Debug, Default)]
pub struct ActivityVm {
    /// Range index (7 days, 30 days, year, all time).
    pub range: usize,
    /// Overview for the range.
    pub overview: AsyncValue<GameOverview, UserError>,
    /// Its query.
    pub overview_q: Option<QueryId>,
    /// History rows loaded so far.
    pub history: Vec<HistoryRow>,
    /// More pages exist.
    pub has_more: bool,
    /// Next page to load.
    pub next_page: u32,
    /// A page is loading.
    pub history_q: Option<QueryId>,
    /// The last page could not be loaded (*Load more* tries again).
    pub history_failed: bool,
    /// Show the chart as a table.
    pub table: bool,
    /// The focused bar.
    pub bar: Option<usize>,
}

/// Game page state.
#[derive(Clone, Debug, Default)]
pub struct DetailsVm {
    /// The game shown.
    pub game: Option<GameId>,
    /// Tab.
    pub tab: DetailsTab,
    /// The full record.
    pub detail: AsyncValue<Box<GameDetail>, UserError>,
    /// Its query.
    pub detail_q: Option<QueryId>,
    /// The library revision the record was loaded for.
    pub loaded_rev: u64,
    /// Inline rename text.
    pub rename: Option<String>,
    /// Notes being edited.
    pub notes: String,
    /// Last keystroke in the notes (autosave 800 ms later).
    pub notes_edited: Option<MonoInstant>,
    /// When the notes were last saved ("Saved" tick).
    pub notes_saved: Option<MonoInstant>,
    /// Protection draft.
    pub protection: Option<PolicyEditor>,
    /// Compatibility draft.
    pub compat: Option<CompatDraft>,
    /// Launch options draft.
    pub launch: Option<LaunchDraft>,
    /// Activity.
    pub activity: ActivityVm,
    /// The last seven days, for the strip beside Play (whatever range the Activity tab
    /// shows).
    pub week: AsyncValue<GameOverview, UserError>,
    /// Its query.
    pub week_q: Option<QueryId>,
    /// The sessions and playtime the week was asked for: asked again when they change.
    pub week_for: Option<(u32, u64)>,
    /// Private-storage size.
    pub prefix_size: AsyncValue<u64>,
    /// Its query.
    pub size_q: Option<QueryId>,
    /// Fix picker search text.
    pub fix_search: String,
    /// Fix search results.
    pub fixes: AsyncValue<Vec<FixEntry>>,
    /// Its query.
    pub fixes_q: Option<QueryId>,
    /// Compatibility advanced disclosure.
    pub compat_advanced: bool,
    /// Launch options advanced disclosure.
    pub launch_advanced: bool,
    /// The cover menu is open.
    pub cover_menu: bool,
    /// Initial focus went to the Play pill.
    pub play_focused: bool,
    /// A program's label or own arguments being edited inline (Launch options).
    pub program_edit: Option<ProgramEdit>,
}

/// What is edited about one program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramField {
    /// Its label ("Settings").
    Label,
    /// Its own arguments (empty = the game's arguments).
    Args,
}

/// An inline program edit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgramEdit {
    /// The program.
    pub exe: easyplay_core::ids::ExecutableId,
    /// The field.
    pub field: ProgramField,
    /// The text.
    pub text: String,
}

/// The four ranges of the Activity tab and Statistics screen.
pub const RANGE_DAYS: [Option<u32>; 4] = [Some(7), Some(30), Some(365), None];

/// The date range for a range index ending today (local time at `offset_s`).
pub fn range_for(index: usize, now: Timestamp, offset_s: i32) -> Option<DateRange> {
    use easyplay_core::stats::rollup::{key_date, local_day};
    let today = key_date(local_day(now, (offset_s / 60) as i16))?;
    Some(match RANGE_DAYS.get(index).copied().flatten() {
        Some(365) => DateRange::this_year(today),
        Some(n) => DateRange::last_days(today, n),
        None => DateRange::all_time(today),
    })
}

impl DetailsVm {
    /// Whether any tab has unsaved changes.
    pub fn dirty(&self) -> bool {
        self.protection.as_ref().is_some_and(PolicyEditor::dirty)
            || self.compat_dirty()
            || self.launch_dirty()
    }

    /// The loaded record.
    pub fn record(&self) -> Option<&GameDetail> {
        self.detail.ready().map(|b| &**b)
    }

    /// Whether the Compatibility draft differs.
    pub fn compat_dirty(&self) -> bool {
        match (&self.compat, self.record()) {
            (Some(c), Some(d)) => *c != compat_of(d),
            _ => false,
        }
    }

    /// Whether the Launch options draft differs.
    pub fn launch_dirty(&self) -> bool {
        match (&self.launch, self.record()) {
            (Some(l), Some(d)) => match l.resolved() {
                Ok(cfg) => cfg != d.game.launch,
                Err(_) => true,
            },
            _ => false,
        }
    }

    /// A freshly loaded record: keep drafts the user is editing, refresh the rest.
    pub fn loaded(&mut self, d: Box<GameDetail>, assessment: PolicyAssessment, rev: u64) {
        if !self.protection.as_ref().is_some_and(PolicyEditor::dirty) {
            self.protection = Some(PolicyEditor::new(d.game.policy.clone(), assessment));
        }
        if !self.compat_dirty() {
            self.compat = Some(compat_of(&d));
        }
        if !self.launch_dirty() {
            self.launch = Some(LaunchDraft::new(d.game.launch.clone()));
        }
        if self.notes_edited.is_none() {
            self.notes.clone_from(&d.game.notes);
        }
        self.loaded_rev = rev;
        self.detail = AsyncValue::Ready(d);
    }

    /// The edits that save the Compatibility draft.
    pub fn compat_edits(&self) -> Vec<GameEdit> {
        let (Some(c), Some(d)) = (&self.compat, self.record()) else {
            return Vec::new();
        };
        let mut launch = self
            .launch
            .as_ref()
            .and_then(|l| l.resolved().ok())
            .unwrap_or_else(|| d.game.launch.clone());
        launch.compat = c.compat.clone();
        vec![
            GameEdit::Runtime {
                reference: c.runtime.clone(),
                follow: c.follow,
            },
            GameEdit::UmuId(c.umu_id.clone()),
            GameEdit::Launch(Box::new(launch)),
        ]
    }

    /// Reset every draft to the saved record.
    pub fn discard(&mut self) {
        if let Some(p) = &mut self.protection {
            p.draft = p.saved.clone();
            p.folder_error = None;
            p.confirm_preset = None;
        }
        if let Some(d) = self.record() {
            let c = compat_of(d);
            let l = LaunchDraft::new(d.game.launch.clone());
            self.compat = Some(c);
            self.launch = Some(l);
        }
    }
}

/// The Compatibility slice of a record.
pub fn compat_of(d: &GameDetail) -> CompatDraft {
    CompatDraft {
        runtime: d.game.runtime.clone(),
        follow: d.game.runtime_follow_updates,
        umu_id: d.game.umu_id.clone(),
        compat: d.game.launch.compat.clone(),
    }
}

/// The preset a policy is based on.
pub fn base_of(p: &SandboxPolicy) -> BasePreset {
    p.preset.base()
}

/// Whether `p` is a custom policy.
pub fn is_custom(p: &SandboxPolicy) -> bool {
    matches!(p.preset, Preset::Custom { .. })
}

/// A user-variable row as typed.
pub fn env_rows(cfg: &LaunchConfig) -> Vec<EnvVar> {
    cfg.env.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    fn editor() -> PolicyEditor {
        let p = easyplay_core::sandbox::offline();
        PolicyEditor::new(p.clone(), demo::assess("X", None, &p))
    }

    #[test]
    fn dirty_tracking_and_markers() {
        let mut e = editor();
        assert!(!e.dirty());
        e.toggle_user_dir(
            UserDir::Music,
            PathBuf::from("/home/sam/Music"),
            demo::baseline_now(),
        );
        assert!(e.dirty());
        assert!(matches!(e.draft.preset, Preset::Custom { .. }));
        e.assessment = demo::assess("X", None, &e.draft);
        assert!(e.is_added("Can read Music"));
        assert!(e.removed().iter().any(|t| t.contains("personal files")));
        assert_eq!(e.widenings().len(), 1);
        e.toggle_user_dir(
            UserDir::Music,
            PathBuf::from("/home/sam/Music"),
            demo::baseline_now(),
        );
        assert!(!e.dirty());
    }

    #[test]
    fn errors_count_blocking_issues() {
        let mut e = editor();
        assert!(e.add_folder(PathBuf::from("/home/sam"), demo::baseline_now()));
        assert!(!e.add_folder(PathBuf::from("/home/sam"), demo::baseline_now()));
        e.assessment = demo::assess("X", None, &e.draft);
        assert_eq!(e.errors(), 1);
    }

    #[test]
    fn launch_draft_splits_arguments() {
        let mut l = LaunchDraft::new(LaunchConfig::default());
        l.args_text = "-a \"b c\"".into();
        assert_eq!(l.resolved().map(|c| c.args.len()), Ok(2));
        l.args_text = "\"open".into();
        assert!(l.resolved().is_err());
    }
}

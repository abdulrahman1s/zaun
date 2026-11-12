//! OpenPaths (SVC-003, LOGIC_SPECS §1): classify and fingerprint each path on the job pool,
//! match it against the library, then launch, prompt, show or draft.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::super::app_service::{AppService, Internal};
use super::super::command::{LaunchOrigin, OpenOrigin};
use super::super::event::{Event, Notice, SetupContext};
use super::super::prompts::Continuation;
use crate::artwork::IconSource;
use crate::clock::Clock;
use crate::consts::MIN_PE_SIZE;
use crate::error::{Error, ErrorCode};
use crate::fsx::{Fs, MountTable};
use crate::identity::matcher::{self, Change, MatchOutcome, PathProbe};
use crate::identity::pe::{PeError, PeInspector, PeLimits};
use crate::identity::{SharedLocations, fingerprint};
use crate::jobs::{JobLabel, JobPriority};
use crate::library::draft::UmuTitleMatch;
use crate::library::{DraftEnv, ExeIndex, GameDraft, PolicyCheck, build_draft};
use crate::model::{ExeRole, Fingerprint, PeInfo, PromptAnswer, PromptKind, SandboxPolicy};
use crate::sandbox::presets;

/// Most paths handled per request.
pub const MAX_PATHS: usize = 64;

/// The classification job's result.
pub(crate) struct Opened {
    pub path: PathBuf,
    pub origin: OpenOrigin,
    pub result: Result<Classified, Error>,
}

/// A Windows program, fingerprinted and matched.
pub(crate) struct Classified {
    pub canonical: PathBuf,
    pub fingerprint: Fingerprint,
    pub pe: PeInfo,
    pub outcome: MatchOutcome,
    /// Built when the outcome is `Unknown`.
    pub draft: Option<Box<GameDraft>>,
}

struct FsProbe<'a> {
    fs: &'a dyn Fs,
    mounts: &'a MountTable,
}

impl PathProbe for FsProbe<'_> {
    fn exists(&self, path: &Path) -> bool {
        self.fs.exists(path)
    }
    fn is_mounted(&self, mount_point: &Path) -> bool {
        self.mounts
            .mounts
            .iter()
            .any(|m| m.mount_point == mount_point)
    }
}

/// Everything the job needs, owned.
pub(crate) struct DraftInputs {
    pub fs: Arc<dyn Fs>,
    pub clock: Arc<dyn Clock>,
    pub pe: Arc<dyn PeInspector>,
    pub icons: Arc<dyn IconSource>,
    pub shared: SharedLocations,
    pub mounts: MountTable,
    pub index: ExeIndex,
    pub base_policy: SandboxPolicy,
    pub umu: Arc<dyn UmuTitleMatch>,
    pub check: Arc<dyn PolicyCheck>,
}

impl DraftInputs {
    pub(crate) fn draft(&self, path: &Path) -> Result<GameDraft, Error> {
        let env = DraftEnv {
            fs: &*self.fs,
            clock: &*self.clock,
            pe: &*self.pe,
            limits: PeLimits::default(),
            icons: &*self.icons,
            shared: &self.shared,
            mounts: &self.mounts,
            index: &self.index,
            base_policy: self.base_policy.clone(),
            umu: &*self.umu,
            check: &*self.check,
        };
        build_draft(&env, path)
    }

    /// Steps "Classify", "Fingerprint", "Match" and, for unknown programs, "Draft".
    pub(crate) fn classify(&self, path: &Path) -> Result<Classified, Error> {
        let not_windows =
            || Error::new(ErrorCode::IdNotWindowsProgram).with("path", path.display());
        if !path.is_absolute() {
            return Err(not_windows());
        }
        let canonical = self.fs.canonicalize(path)?;
        let md = self.fs.metadata(&canonical)?;
        if !md.is_file() || md.size < MIN_PE_SIZE {
            return Err(not_windows());
        }
        let fp = fingerprint::compute(&*self.fs, &canonical)?;
        let pe = match self.pe.inspect(&canonical, &PeLimits::default()) {
            Ok(i) => i,
            Err(PeError::NotExecutable | PeError::NotPe(_))
                if !canonical
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("msi")) =>
            {
                return Err(not_windows());
            }
            Err(_) => PeInfo::unknown(),
        };
        let probe = FsProbe {
            fs: &*self.fs,
            mounts: &self.mounts,
        };
        let outcome = matcher::resolve(&canonical, &fp, &pe, &self.index, &probe);
        let draft = match outcome {
            MatchOutcome::Unknown { .. } => Some(Box::new(self.draft(&canonical)?)),
            _ => None,
        };
        Ok(Classified {
            canonical,
            fingerprint: fp,
            pe,
            outcome,
            draft,
        })
    }
}

impl AppService {
    pub(crate) fn draft_inputs(&self) -> DraftInputs {
        let base = self.prefs.protection.default_preset;
        let base_policy = match &self.host {
            Some(h) => presets::preset_policy(base, h),
            None => presets::offline(),
        };
        DraftInputs {
            fs: Arc::clone(&self.deps.fs),
            clock: Arc::clone(&self.deps.clock),
            pe: Arc::clone(&self.deps.pe),
            icons: Arc::clone(&self.deps.icons),
            shared: self.deps.shared.clone(),
            mounts: self.deps.launch_env.mounts.clone(),
            index: self.lib.exe_index().clone(),
            base_policy,
            umu: Arc::clone(&self.deps.umu_titles),
            check: Arc::clone(&self.check) as Arc<dyn PolicyCheck>,
        }
    }

    pub(crate) fn open_paths(&mut self, mut paths: Vec<PathBuf>, origin: OpenOrigin) {
        if paths.len() > MAX_PATHS {
            paths.truncate(MAX_PATHS);
            self.notice(Notice::OnlyFirstPaths {
                processed: MAX_PATHS,
            });
        }
        for path in paths {
            let inputs = self.draft_inputs();
            let post = self.poster();
            let _ =
                self.deps
                    .jobs
                    .submit(JobPriority::Interactive, JobLabel("open-path"), move |_| {
                        let result = inputs.classify(&path);
                        post(Internal::Opened(Box::new(Opened {
                            path,
                            origin,
                            result,
                        })));
                    });
        }
    }

    /// Build a draft for a path the user wants set up separately.
    pub(crate) fn draft_job(&self, path: PathBuf, origin: OpenOrigin) {
        let inputs = self.draft_inputs();
        let post = self.poster();
        let _ = self
            .deps
            .jobs
            .submit(JobPriority::Interactive, JobLabel("draft"), move |_| {
                let result = inputs.draft(&path).map(Box::new);
                post(Internal::Drafted { result, origin });
            });
    }

    pub(crate) fn on_drafted(&mut self, result: Result<Box<GameDraft>, Error>, origin: OpenOrigin) {
        match result {
            Ok(d) => self.request_setup(d, origin),
            Err(e) => self.fail(e),
        }
    }

    /// Show the setup dialog, or queue the draft behind the one shown.
    pub(crate) fn request_setup(&mut self, draft: Box<GameDraft>, origin: OpenOrigin) {
        if self.setup_open {
            self.setup_queue.push_back((draft, origin));
            return;
        }
        self.setup_open = true;
        self.emit(Event::SetupRequested {
            draft,
            context: SetupContext {
                origin,
                queued: self.setup_queue.len(),
            },
        });
    }

    /// The dialog closed (added or discarded): show the next draft.
    pub(crate) fn setup_closed(&mut self) {
        self.setup_open = false;
        if let Some((d, o)) = self.setup_queue.pop_front() {
            self.request_setup(d, o);
        }
    }

    fn launch_or_show(
        &mut self,
        game: crate::ids::GameId,
        exe: Option<crate::ids::ExecutableId>,
        origin: OpenOrigin,
    ) {
        if origin.launches_known() {
            let lo = if origin == OpenOrigin::Cli {
                LaunchOrigin::Cli
            } else {
                LaunchOrigin::QuickLaunch
            };
            self.start_launch(game, exe, lo);
        } else {
            self.emit(Event::ShowGame(game));
            self.notice(Notice::AlreadyInLibrary { game });
        }
    }

    pub(crate) fn on_opened(&mut self, o: Opened) {
        let c = match o.result {
            Ok(c) => c,
            Err(e) => {
                tracing::info!(path = %o.path.display(), "opened path not usable");
                return self.fail(e);
            }
        };
        match c.outcome {
            MatchOutcome::Known { game, exe } => self.launch_or_show(game, Some(exe), o.origin),
            MatchOutcome::KnownChanged {
                game,
                exe,
                change: Change::SameIdentity,
            } => {
                if let Err(e) = self.update_fingerprint(exe, c.fingerprint, Some(c.pe)) {
                    return self.fail(e);
                }
                self.launch_or_show(game, Some(exe), o.origin);
            }
            MatchOutcome::KnownChanged {
                game,
                exe,
                change: Change::IdentityDiffers,
            } => {
                let old = self
                    .lib
                    .game(game)
                    .ok()
                    .and_then(|g| {
                        g.executables
                            .iter()
                            .find(|e| e.id == exe)
                            .map(|e| e.fingerprint)
                    })
                    .unwrap_or(c.fingerprint);
                let req = self.prompts.park(
                    &*self.deps.clock,
                    PromptKind::ExeChanged {
                        game,
                        exe,
                        old,
                        new: c.fingerprint,
                    },
                    Continuation::OpenChanged {
                        path: c.canonical,
                        game,
                        exe,
                        fingerprint: c.fingerprint,
                        pe: Box::new(c.pe),
                        origin: o.origin,
                    },
                );
                self.emit(Event::Prompt(req));
            }
            MatchOutcome::InsideKnownRoot { game } => {
                let req = self.prompts.park(
                    &*self.deps.clock,
                    PromptKind::InsideKnownRoot {
                        game,
                        path: c.canonical.clone(),
                    },
                    Continuation::OpenAttach {
                        path: c.canonical,
                        game,
                        origin: o.origin,
                    },
                );
                self.emit(Event::Prompt(req));
            }
            MatchOutcome::CopyCandidate { game, .. } => {
                let req = self.prompts.park(
                    &*self.deps.clock,
                    PromptKind::CopyOfKnown {
                        game,
                        path: c.canonical.clone(),
                    },
                    Continuation::OpenAttach {
                        path: c.canonical,
                        game,
                        origin: o.origin,
                    },
                );
                self.emit(Event::Prompt(req));
            }
            MatchOutcome::MovedCandidate { game, exe } => {
                match self.lib.relink(exe, &c.canonical, true) {
                    Ok(_) => {
                        self.publish_library();
                        self.notice(Notice::FoundMoved { game });
                        self.launch_or_show(game, Some(exe), o.origin);
                    }
                    Err(e) => self.fail(e),
                }
            }
            MatchOutcome::Unknown { .. } => match c.draft {
                Some(d) => self.request_setup(d, o.origin),
                None => self.draft_job(c.canonical, o.origin),
            },
        }
    }

    /// Store a fresh fingerprint (and PE information) for an executable.
    pub(crate) fn update_fingerprint(
        &mut self,
        exe: crate::ids::ExecutableId,
        fp: Fingerprint,
        pe: Option<PeInfo>,
    ) -> Result<(), Error> {
        use crate::storage::ExecutableRepo;
        let Some((game, mut row)) = self.lib.db_mut().executable(exe)? else {
            return Err(Error::new(ErrorCode::LibInvalidField).with("detail", "no such executable"));
        };
        row.fingerprint = fp;
        if let Some(pe) = pe {
            row.pe = pe;
        }
        self.lib.db_mut().update_executable(&row)?;
        self.lib.refresh_game(game)?;
        self.publish_library();
        Ok(())
    }

    /// Answers for the open-flow prompts.
    pub(crate) fn resume_open(&mut self, then: Continuation, answer: &PromptAnswer) {
        match (then, answer) {
            (_, PromptAnswer::Cancel) => {}
            (
                Continuation::OpenChanged {
                    game,
                    exe,
                    fingerprint,
                    pe,
                    origin,
                    ..
                },
                PromptAnswer::ItsAnUpdate,
            ) => match self.update_fingerprint(exe, fingerprint, Some(*pe)) {
                Ok(()) => self.launch_or_show(game, Some(exe), origin),
                Err(e) => self.fail(e),
            },
            (
                Continuation::OpenChanged { path, origin, .. }
                | Continuation::OpenAttach { path, origin, .. },
                PromptAnswer::SetUpAsNew,
            ) => {
                // "Set up as new": the path leaves the old game first (rule 2/3); for
                // InsideKnownRoot/CopyOfKnown the path belongs to no game.
                self.set_up_as_new(path, origin);
            }
            (
                Continuation::OpenAttach { path, game, origin },
                PromptAnswer::RunWithGameSettings { .. } | PromptAnswer::UseSameSettings,
            ) => match self.lib.add_executable(game, &path, ExeRole::Alternate) {
                Ok(exe) => {
                    self.publish_library();
                    self.launch_or_show(game, Some(exe), origin);
                }
                Err(e) => self.fail(e),
            },
            (other, a) => tracing::warn!(?a, ?other, "unexpected answer for an open prompt"),
        }
    }
}

//! Draft a program for the setup dialog and queue the dialogs (SVC-004, LOGIC_SPECS §2).
use std::path::{Path, PathBuf};

use crate::artwork::IconSource;
use crate::clock::Clock;
use crate::fsx::{Fs, MountTable};
use crate::identity::SharedLocations;
use crate::identity::pe::{PeInspector, PeLimits};
use crate::library::draft::UmuTitleMatch;
use crate::library::{DraftEnv, ExeIndex, GameDraft, PolicyCheck, build_draft};
use crate::model::SandboxPolicy;
use crate::sandbox::presets;
use crate::service::event::SetupContext;
use crate::service::prelude::*;

/// Everything the draft and classify jobs need, owned.
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
    pub(crate) fn draft(&self, path: &Path) -> Result<GameDraft> {
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
}

impl AppService {
    pub(super) fn draft_inputs(&self) -> DraftInputs {
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

    pub(crate) fn on_drafted(
        &mut self,
        result: Result<Box<GameDraft>>,
        origin: OpenOrigin,
    ) -> Result<()> {
        self.request_setup(result?, origin);
        Ok(())
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
    pub(crate) fn setup_closed(&mut self) -> Result<()> {
        self.setup_open = false;
        if let Some((d, o)) = self.setup_queue.pop_front() {
            self.request_setup(d, o);
        }
        Ok(())
    }
}

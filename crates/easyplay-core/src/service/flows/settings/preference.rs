//! `SetPreference`: validate, save, publish (LOGIC_SPECS §10; SVC-009).
use crate::config::{self, LoadOptions, PreferencePatch};
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn set_preference(&mut self, patch: &PreferencePatch) -> Result<()> {
        let next = self
            .prefs
            .apply(patch, &*self.deps.fs, &LoadOptions { env_rule: None })?;
        config::save(&*self.deps.fs, &self.deps.paths.config_file, &next)?;
        let recheck = self.prefs.advanced.developer_mode != next.advanced.developer_mode
            || self.prefs.launch.global_env != next.launch.global_env;
        self.prefs = next.clone();
        self.hub.prefs.store(Arc::new(next));
        if recheck {
            self.install_policy_check();
        }
        self.emit(Event::PreferencesChanged);
        self.steam_preference_changed();
        self.chat_preference_changed();
        self.overlay_sync();
        Ok(())
    }
}

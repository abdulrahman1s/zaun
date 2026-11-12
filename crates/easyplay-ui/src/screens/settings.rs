//! Settings. Arrives with UI-023.

use egui::Ui;

use super::Env;
use crate::app::UiState;
use crate::route::SettingsTab;

/// Tabs in order.
pub const TABS: [SettingsTab; 7] = [
    SettingsTab::General,
    SettingsTab::Appearance,
    SettingsTab::Protection,
    SettingsTab::CompatibilityLayers,
    SettingsTab::Integration,
    SettingsTab::Advanced,
    SettingsTab::About,
];

/// Show the screen.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, tab: SettingsTab) {
    let _ = (ui, state, env, tab);
}

/// Set a boolean preference: optimistic local value plus `SetPreference`.
pub fn set_bool(
    state: &mut UiState,
    env: &mut Env<'_>,
    key: &str,
    value: bool,
    apply: impl FnOnce(&mut easyplay_core::config::Preferences, bool),
) {
    let mut next = (**env.bridge.prefs()).clone();
    apply(&mut next, value);
    let patch = easyplay_core::config::PreferencePatch::new().set(key, value);
    env.bridge.send(crate::bridge::UiCommand::SetPreference {
        patch,
        preview: Box::new(next.clone()),
    });
    env.bridge.set_prefs_preview(next);
    state.settings.saved.insert(key.to_owned(), env.mono);
}

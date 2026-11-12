use easyplay_core::{model::{GameSummary, Activity}, library::query::run_query};
use easyplay_core::sandbox::explain::explain_policy;
use crate::strings;
// "This comment is a sentence that must be ignored."
fn f(ui: &mut Ui, c: [u8; 3]) {
    ui.label(strings::library::EMPTY);
    let _ = egui::Color32::from_rgb(c[0], c[1], c[2]);
    tracing::debug!("Something went wrong in the grid");
}

#[cfg(test)]
mod tests {
    use easyplay_core::storage::Db;
    fn t() { let _ = egui::Color32::from_rgb(1, 2, 3); unsafe {} }
}

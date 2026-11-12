//! EasyPlay's user interface: present snapshots, collect intent, send commands.
//!
//! Depends only on the public API of `easyplay-core` listed in ARCHITECTURE §3 rule 2 plus
//! `egui`, `eframe`, `egui_extras`, `rfd` and `gilrs`. Performs no I/O (exceptions: `rfd`
//! dialogs on their own thread, and `textures` receiving decoded pixels from jobs), holds no
//! core lock across frames, contains no business rules, and keeps every user-facing string in
//! [`strings`]. The entry point `run(start, deps)` arrives with task UI-001.

#![forbid(unsafe_code)]

pub mod a11y;
pub mod app;
pub mod bridge;
pub mod input;
pub mod route;
pub mod screens;
pub mod strings;
pub mod textures;
pub mod theme;
pub mod vm;
pub mod widgets;

#[cfg(test)]
mod tests {
    /// Smoke test: the UI links against the core model and egui.
    #[test]
    fn links_core_and_egui() {
        assert_eq!(easyplay_core::model::BasePreset::ALL.len(), 3);
        let ctx = egui::Context::default();
        assert!(ctx.pixels_per_point() > 0.0);
    }
}

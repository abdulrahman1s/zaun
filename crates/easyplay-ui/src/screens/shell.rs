//! The application shell (SCREEN_SPECS §0): sidebar, content frame, banner slot, toast area,
//! overlay layers, window title per route.
//!
//! `Panel::left("sidebar")` then `CentralPanel` last [F-305]; overlays are `egui::Modal`
//! [F-306]; toasts an `Area` anchored bottom-centre.

use egui::{CentralPanel, Frame, Id, Key, KeyboardShortcut, Modifiers, Panel, Ui, ViewportCommand};

use super::{Env, gallery, library, placeholder};
use crate::app::UiState;
use crate::route::{Overlay, Route};
use crate::strings;
use crate::theme::tokens::{layout, space};
use crate::theme::{self, Dur};
use crate::widgets::banner::{Severity, banner};
use crate::widgets::modal::{self, Action, DialogOutcome};
use crate::widgets::{button::Variant, sheet, toast};

/// A persistent condition shown at the top of the content area (USER_EXPERIENCE §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    /// Stable key (dismissal is remembered per key).
    pub key: &'static str,
    /// Severity; higher wins the slot.
    pub severity: Severity,
    /// Sentence.
    pub text: String,
    /// One action label.
    pub action: Option<String>,
    /// Whether it can be dismissed.
    pub dismissible: bool,
}

/// One banner at a time, by priority.
#[derive(Clone, Debug, Default)]
pub struct BannerQueue {
    items: Vec<Banner>,
    dismissed: Vec<&'static str>,
}

impl BannerQueue {
    /// Add or replace a banner by key.
    pub fn set(&mut self, b: Banner) {
        self.items.retain(|x| x.key != b.key);
        self.items.push(b);
    }

    /// Remove a banner by key.
    pub fn clear(&mut self, key: &str) {
        self.items.retain(|x| x.key != key);
    }

    /// Dismiss by key (until the app restarts).
    pub fn dismiss(&mut self, key: &'static str) {
        self.dismissed.push(key);
    }

    /// The banner that owns the slot: highest severity, then most recent.
    pub fn top(&self) -> Option<&Banner> {
        // `max_by_key` keeps the last maximum: the most recent of equal severity.
        self.items
            .iter()
            .filter(|b| !self.dismissed.contains(&b.key))
            .max_by_key(|b| b.severity)
    }
}

/// Effective sidebar collapse: the user's choice, else automatic below 900 px.
pub fn sidebar_collapsed(state: &UiState, window_w: f32) -> bool {
    window_w < layout::COMPACT_BELOW
        || state
            .sidebar_pref
            .unwrap_or(window_w < layout::SIDEBAR_AUTO_COLLAPSE_BELOW)
}

/// The window title for the current route.
pub fn window_title(state: &UiState, env: &Env<'_>) -> String {
    let route = state.nav.current();
    let game_name = match route {
        Route::Game(id) => env.bridge.library().game(id).map(|g| g.name.to_string()),
        _ => None,
    };
    strings::title::window(route.title(game_name.as_deref()))
}

/// Paint the whole window.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let ctx = ui.ctx().clone();
    let theme = theme::theme(&ctx);
    let p = theme.palette.clone();
    modal::apply_focus_return(&ctx);
    handle_global_keys(ui, state);

    let title = window_title(state, env);
    if state.last_title.as_deref() != Some(title.as_str()) {
        ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
        state.last_title = Some(title);
    }

    let window_w = ctx.content_rect().width();
    let route = state.nav.current();
    if route.has_sidebar() {
        let collapsed = sidebar_collapsed(state, window_w);
        let full_w = if window_w >= layout::WIDE_FROM {
            layout::SIDEBAR_W_WIDE
        } else {
            layout::SIDEBAR_W
        };
        let t = ctx.animate_bool_with_time(
            Id::unique("sidebar.width"),
            collapsed,
            theme.motion.movement(Dur::Sidebar),
        );
        let w = egui::lerp(full_w..=layout::RAIL_W, t);
        Panel::left("sidebar")
            .resizable(false)
            .exact_size(w)
            .show_separator_line(false)
            .frame(Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| {
                library::sidebar::show(ui, state, env, t > 0.5);
            });
    }

    CentralPanel::default()
        .frame(Frame::NONE.fill(p.bg_base))
        .show(ui, |ui| {
            let margin = if window_w < layout::COMPACT_BELOW {
                layout::PAGE_MARGIN_COMPACT
            } else {
                layout::PAGE_MARGIN
            };
            if let Some(b) = state.banners.top().cloned() {
                Frame::NONE
                    .inner_margin(egui::Margin {
                        left: margin as i8,
                        right: margin as i8,
                        top: space::S4 as i8,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        let r = banner(ui, b.severity, &b.text, b.action.as_deref(), b.dismissible);
                        if r.dismissed {
                            state.banners.dismiss(b.key);
                        }
                    });
            }
            match route {
                Route::Library => library::show(ui, state, env),
                Route::Gallery => gallery::show(ui, state, env),
                Route::Game(id) => placeholder::game(ui, state, env, id),
                Route::Statistics | Route::Settings(_) | Route::Onboarding => {
                    placeholder::show(ui, state, route)
                }
            }
        });

    show_overlays(&ctx, state);
    crate::a11y::Announcer::show(ui);
    let out = toast::show(&ctx, &mut state.toasts, env.mono, 0.0);
    let _ = out;
}

fn handle_global_keys(ui: &mut Ui, state: &mut UiState) {
    let toggle = KeyboardShortcut::new(Modifiers::COMMAND, Key::B);
    if ui.input_mut(|i| i.consume_shortcut(&toggle)) {
        let w = ui.ctx().content_rect().width();
        let now = sidebar_collapsed(state, w);
        state.sidebar_pref = Some(!now);
    }
    let back = KeyboardShortcut::new(Modifiers::ALT, Key::ArrowLeft);
    if ui.input_mut(|i| i.consume_shortcut(&back)) {
        state.nav.back();
    }
    // Esc goes back from a page when no overlay or text field has it.
    if state.nav.overlays().is_empty()
        && !matches!(state.nav.current(), Route::Library)
        && ui.ctx().memory(|m| m.focused()).is_none()
        && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        state.nav.back();
    }
    if ui.input(|i| i.pointer.button_clicked(egui::PointerButton::Extra1)) {
        state.nav.back();
    }
    #[cfg(debug_assertions)]
    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F12)) {
        if state.nav.current() == Route::Gallery {
            state.nav.back();
        } else {
            state.nav.push(Route::Gallery);
        }
    }
}

fn overlay_id(o: Overlay) -> Id {
    Id::unique(("overlay", o))
}

fn show_overlays(ctx: &egui::Context, state: &mut UiState) {
    let overlays = state.nav.overlays().to_vec();
    for o in overlays {
        let id = overlay_id(o);
        match o {
            Overlay::Dialog(_) => {
                let outcome = modal::dialog(
                    ctx,
                    id,
                    Some(theme::icons::TRASH),
                    strings::gallery::DIALOG_TITLE,
                    strings::gallery::DIALOG_BODY,
                    &[
                        Action {
                            label: strings::gallery::KEEP,
                            variant: Variant::Secondary,
                        },
                        Action {
                            label: strings::gallery::DANGER,
                            variant: Variant::Danger,
                        },
                    ],
                    0,
                );
                if outcome != DialogOutcome::Open {
                    state.nav.close(o);
                }
            }
            _ => {
                let r = sheet::sheet(
                    ctx,
                    id,
                    strings::gallery::SHEET_TITLE,
                    false,
                    |ui| {
                        ui.label(theme::Type::Body.rich(strings::gallery::SHEET_BODY));
                        ui.add_space(space::S4);
                        ui.label(theme::Type::Caption.rich(strings::placeholder::COMING_BODY));
                    },
                    |ui| {
                        let mut close = false;
                        if ui
                            .add(crate::widgets::button::Button::primary(strings::play::PLAY))
                            .clicked()
                        {
                            close = true;
                        }
                        if ui
                            .add(crate::widgets::button::Button::new(
                                strings::feedback::CLOSE,
                            ))
                            .clicked()
                        {
                            close = true;
                        }
                        close
                    },
                );
                if r.close_requested || r.footer {
                    if r.footer {
                        modal::closed(ctx, id);
                    }
                    state.nav.close(o);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_priority() {
        let mut q = BannerQueue::default();
        q.set(Banner {
            key: "a",
            severity: Severity::Info,
            text: "a".into(),
            action: None,
            dismissible: true,
        });
        q.set(Banner {
            key: "b",
            severity: Severity::Warning,
            text: "b".into(),
            action: None,
            dismissible: true,
        });
        q.set(Banner {
            key: "c",
            severity: Severity::Info,
            text: "c".into(),
            action: None,
            dismissible: true,
        });
        assert_eq!(q.top().map(|b| b.key), Some("b"));
        q.dismiss("b");
        assert_eq!(q.top().map(|b| b.key), Some("c"));
        q.clear("c");
        assert_eq!(q.top().map(|b| b.key), Some("a"));
    }
}

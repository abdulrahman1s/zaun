//! The keyboard shortcut sheet (`?`, `F1`), generated from the shortcut table (UI-030).

use egui::{Context, Id};

use crate::app::UiState;
use crate::input::shortcuts::{Scope, TABLE};
use crate::route::Overlay;
use crate::strings;
use crate::theme::tokens::space;
use crate::theme::{self, Type};
use crate::widgets::button::Button;
use crate::widgets::key_hint::key_hint;
use crate::widgets::sheet::sheet;

fn scope_title(s: Scope) -> &'static str {
    match s {
        Scope::Global => strings::shortcuts::SCOPE_GLOBAL,
        Scope::Library => strings::shortcuts::SCOPE_LIBRARY,
        Scope::GamePage => strings::shortcuts::SCOPE_GAME,
        Scope::Pages => strings::shortcuts::SCOPE_PAGES,
        Scope::Games => strings::shortcuts::SCOPE_GAMES,
    }
}

/// Show the sheet.
pub fn show(ctx: &Context, id: Id, state: &mut UiState) {
    let r = sheet(
        ctx,
        id,
        strings::shortcuts::TITLE,
        false,
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            for scope in [
                Scope::Global,
                Scope::Library,
                Scope::Games,
                Scope::GamePage,
                Scope::Pages,
            ] {
                ui.add_space(space::S3);
                ui.label(Type::Heading.rich(scope_title(scope)).color(p.text_primary));
                ui.add_space(space::S1);
                for b in TABLE.iter().filter(|b| b.scope == scope) {
                    ui.horizontal(|ui| {
                        ui.set_min_height(space::S8);
                        ui.label(Type::Body.rich(b.label).color(p.text_secondary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for (i, k) in b.keys.iter().enumerate().rev() {
                                key_hint(ui, &k.label());
                                if i > 0 {
                                    ui.label(
                                        Type::Caption
                                            .rich(strings::shortcuts::OR)
                                            .color(p.text_muted),
                                    );
                                }
                            }
                        });
                    });
                }
            }
        },
        |ui| ui.add(Button::primary(strings::feedback::CLOSE)).clicked(),
    );
    if r.close_requested || r.footer {
        crate::widgets::modal::closed(ctx, id);
        state.nav.close(Overlay::Shortcuts);
    }
}

//! Tag editor (SCREEN_SPECS_2 §14, `T`): chips with remove, a field with autocomplete over
//! existing tags; `Enter` adds (creating a new tag when the name is new). Changes apply at
//! once with *Undo* in a toast.

use easyplay_core::ids::{GameId, TagName};
use egui::{Context, Id};

use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings;
use crate::theme::tokens::space;
use crate::theme::{self, Type};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::context_menu::item;
use crate::widgets::sheet::sheet;
use crate::widgets::text_field::TextField;

/// Show the editor for `game`.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(game) else {
        state.nav.close(Overlay::Tags(game));
        return;
    };
    let current: Vec<String> = g
        .tags
        .iter()
        .filter_map(|t| lib.tag(*t))
        .map(|t| t.name.to_string())
        .collect();
    let mut next = current.clone();
    let title = strings::tags::title(&g.name);
    let r = sheet(
        ctx,
        id,
        &title,
        !state.tag_text.is_empty(),
        |ui| {
            let p = theme::theme(ui.ctx()).palette.clone();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(space::S2, space::S2);
                if next.is_empty() {
                    ui.label(Type::Body.rich(strings::tags::NONE).color(p.text_muted));
                }
                for name in current.iter() {
                    let color = lib
                        .tags
                        .iter()
                        .find(|t| *t.name == **name)
                        .map_or(0, |t| t.color.index());
                    if chip(
                        ui,
                        ChipKind::Tag {
                            color,
                            removable: true,
                        },
                        name,
                        None,
                        false,
                    )
                    .removed
                    {
                        next.retain(|n| n != name);
                    }
                }
            });
            ui.add_space(space::S3);
            let field = TextField::new(&mut state.tag_text, "tags.field")
                .label(strings::tags::ADD)
                .hint(strings::tags::HINT)
                .show(ui);
            let typed = state.tag_text.trim().to_lowercase();
            let enter =
                field.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter && !typed.is_empty() && !next.iter().any(|n| n.to_lowercase() == typed) {
                next.push(state.tag_text.trim().to_owned());
                state.tag_text.clear();
                field.response.request_focus();
            }
            if !typed.is_empty() {
                let candidates: Vec<std::sync::Arc<str>> = lib
                    .tags
                    .iter()
                    .filter(|t| {
                        t.name.to_lowercase().contains(&typed)
                            && !next.iter().any(|n| *n == *t.name)
                    })
                    .take(6)
                    .map(|t| t.name.clone())
                    .collect();
                for name in candidates {
                    let t = &name;
                    if item(ui, Some(theme::icons::TAG), t, None, false).clicked() {
                        next.push(t.to_string());
                        state.tag_text.clear();
                    }
                }
                if !lib.tags.iter().any(|t| t.name.to_lowercase() == typed)
                    && item(
                        ui,
                        Some(theme::icons::PLUS),
                        &strings::tags::create(state.tag_text.trim()),
                        Some("Enter"),
                        false,
                    )
                    .clicked()
                {
                    next.push(state.tag_text.trim().to_owned());
                    state.tag_text.clear();
                }
            }
        },
        |ui| ui.add(Button::primary(strings::tags::DONE)).clicked(),
    );
    if next != current {
        let names = |v: &[String]| {
            v.iter()
                .filter_map(|n| TagName::new(n).ok())
                .collect::<Vec<_>>()
        };
        env.bridge.send(UiCommand::SetTags {
            id: game,
            tags: names(&next),
        });
        state.toast(
            strings::feedback::tags_changed(&g.name),
            ToastKind::Success,
            Some(UiCommand::SetTags {
                id: game,
                tags: names(&current),
            }),
            env.mono,
        );
    }
    if r.close_requested || r.footer {
        crate::widgets::modal::closed(ctx, id);
        state.tag_text.clear();
        state.nav.close(Overlay::Tags(game));
    }
}

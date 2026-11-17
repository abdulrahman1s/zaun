//! Tags (script `tagItems`): the design has no tag dialog. Tags are ticked and unticked in a
//! menu that stays open — each item a 7 pt dot and the tag's name, ticked when every game
//! it is for has the tag — which here ends with a separator and a compact "New tag" field,
//! since the app can create tags.
//!
//! Call [`tag_menu`] inside a menu (a submenu "Tags", or the menu of "+ Tag"), in a popup
//! whose close behaviour is `PopupCloseBehavior::CloseOnClickOutside` so that ticking keeps
//! it open:
//!
//! ```ignore
//! context_menu::popup(
//!     Popup::menu(&plus_tag).close_behavior(PopupCloseBehavior::CloseOnClickOutside),
//!     |ui| tags::tag_menu(ui, state, env, &[game]),
//! );
//! ```
//!
//! Where `state` and `env` are out of reach (the game menu's own function), [`tag_items`]
//! paints the same items and returns the commands to send.
//!
//! [`show`] is the tag dialog: the same items with *Done*, for `T` and the game menu's
//! "New tag…" on the game page, where no cover is there to hang a menu from.

use easyplay_core::ids::{GameId, TagName};
use egui::{Context, Id, Ui};

use crate::app::UiState;
use crate::bridge::{LibraryView, UiCommand};
use crate::route::Overlay;
use crate::screens::Env;
use crate::strings;
use crate::theme;
use crate::theme::tokens::overlay;
use crate::widgets::button::Button;
use crate::widgets::context_menu::{separator, tag_item};
use crate::widgets::modal::{Spec, show as dialog};
use crate::widgets::text_field::TextField;

/// The names of `game`'s tags.
fn names_of(lib: &LibraryView, game: GameId) -> Vec<String> {
    lib.get(game)
        .map(|g| {
            g.tags
                .iter()
                .filter_map(|t| lib.tag(*t))
                .map(|t| t.name.to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn set_tags(game: GameId, names: &[String]) -> UiCommand {
    UiCommand::SetTags {
        id: game,
        tags: names.iter().filter_map(|n| TagName::new(n).ok()).collect(),
    }
}

/// Paint the tag items for `games` (one game, or a selection) and the "New tag" field,
/// whose text is `new_tag`. Returns the commands that apply what was ticked, unticked or
/// created; the caller sends them.
pub fn tag_items(
    ui: &mut Ui,
    lib: &LibraryView,
    games: &[GameId],
    new_tag: &mut String,
) -> Vec<UiCommand> {
    items(ui, lib, games, new_tag, false)
}

/// [`tag_items`]; `focus_field` puts focus in the "New tag" field.
fn items(
    ui: &mut Ui,
    lib: &LibraryView,
    games: &[GameId],
    new_tag: &mut String,
    focus_field: bool,
) -> Vec<UiCommand> {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut out = Vec::new();
    let has = |g: GameId, name: &str| names_of(lib, g).iter().any(|n| n == name);
    for t in &lib.tags {
        let name = t.name.to_string();
        let all = !games.is_empty() && games.iter().all(|g| has(*g, &name));
        if tag_item(ui, &name, p.tag_dot(&name), all).clicked() {
            // Ticking adds the tag to every game; unticking takes it from every game.
            for g in games {
                let mut names = names_of(lib, *g);
                names.retain(|n| *n != name);
                if !all {
                    names.push(name.clone());
                }
                out.push(set_tags(*g, &names));
            }
        }
    }
    if !lib.tags.is_empty() {
        separator(ui);
    }
    let [side, _] = overlay::MENU_RULE;
    ui.horizontal(|ui| {
        ui.add_space(side);
        let w = ui.available_width() - side;
        let f = TextField::new(new_tag, "tags.new")
            .hint(strings::tags::NEW_TAG())
            .accessible_name(strings::tags::NEW_TAG_NAME())
            .width(w)
            .show(ui);
        // Until the field has taken focus once (the dialog's first passes may be sizing
        // passes that cannot take it).
        let pending = ui.make_persistent_id("tags.focus_pending");
        if focus_field {
            ui.data_mut(|d| d.insert_temp(pending, true));
        }
        if ui.data(|d| d.get_temp::<bool>(pending)).unwrap_or(false) {
            if f.response.has_focus() {
                ui.data_mut(|d| d.remove_temp::<bool>(pending));
            } else {
                f.response.request_focus();
            }
        }
        let typed = new_tag.trim().to_owned();
        let enter = f.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if enter && !typed.is_empty() {
            for g in games {
                let mut names = names_of(lib, *g);
                if !names.iter().any(|n| n.eq_ignore_ascii_case(&typed)) {
                    names.push(typed.clone());
                    out.push(set_tags(*g, &names));
                }
            }
            new_tag.clear();
            f.response.request_focus();
        }
    });
    ui.add_space(side);
    out
}

/// Paint the tag items for `games` and send what is ticked, unticked or created.
pub fn tag_menu(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, games: &[GameId]) {
    tag_menu_opened(ui, state, env, games, false);
}

/// [`tag_menu`], told in the menu's first pass when it was opened from the keyboard (`T`):
/// focus then goes to the first tag, or to the "New tag" field while there are no tags.
pub fn tag_menu_opened(
    ui: &mut Ui,
    state: &mut UiState,
    env: &mut Env<'_>,
    games: &[GameId],
    by_keyboard: bool,
) {
    let lib = env.bridge.library().clone();
    let no_tags = lib.tags.is_empty();
    if by_keyboard && !no_tags {
        crate::widgets::context_menu::focus_first(ui);
    }
    for c in items(ui, &lib, games, &mut state.tag_text, by_keyboard && no_tags) {
        env.bridge.send(c);
    }
}

/// The tag dialog for `game`: the menu's items and *Done*.
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>, game: GameId) {
    let lib = env.bridge.library().clone();
    let Some(g) = lib.get(game) else {
        state.nav.close(Overlay::Tags(game));
        return;
    };
    let title = strings::tags::title(&g.name);
    let r = dialog(
        ctx,
        id,
        Spec::new(&title)
            .close_button()
            .unsaved(!state.tag_text.is_empty()),
        |ui| {
            // Opened with `T`: focus goes to the field, ready to type a tag.
            let first = crate::widgets::modal::opening(ui.ctx(), id.with("tags"));
            for c in items(ui, &lib, &[game], &mut state.tag_text, first) {
                env.bridge.send(c);
            }
        },
        |ui| ui.add(Button::primary(strings::tags::DONE())).clicked(),
    );
    if r.close_requested || r.footer {
        if r.footer {
            crate::widgets::modal::closed(ctx, id);
        }
        state.tag_text.clear();
        state.nav.close(Overlay::Tags(game));
    }
}

//! The game menu (GAME_LIBRARY §10; the design's `gameMenu`): right-click on a cover or a
//! row opens it at the pointer; the Menu key or `Shift+F10` hangs it from the focused
//! cover; the game page's more button opens it too. Text only, in the design's order:
//!
//! Play / Stop · Open game page (library only) | Favourite · Tags ▸ · Protection ▸ |
//! Open game folder · Open save data · Add to / Remove from applications menu · Run another
//! program in this game… · Change artwork… | Hide · Remove… | the developer items.
//!
//! Tags and Protection replace the items in place under a back row; the items that toggle
//! (a tag, a level) keep the menu open.
//!
//! A Steam game's menu (ADR-0031) has what applies to a game Steam runs: Play (no Stop) ·
//! Open game page | Favourite · Tags ▸ | Open in Steam · Update in Steam (while an update
//! waits) · Open game folder · Open Windows storage folder (a Proton game with storage) |
//! Hide. No protection, removal, shortcut, artwork, save data, other programs or developer
//! items.

use egui::{
    Id, LayerId, Order, Popup, PopupAnchor, PopupCloseBehavior, PopupKind, Pos2, Rect, RectAlign,
    Response, Ui,
};
use zaun_core::ids::GameId;
use zaun_core::model::{Activity, Availability, BasePreset, GameSummary, Preset, SteamUpdate};

use crate::bridge::{LibraryView, UiCommand};
use crate::screens::actions::{GameAction, widens};
use crate::strings;
use crate::strings::menu as m;
use crate::theme;
use crate::widgets::context_menu::{
    self, back, check_item, danger_item_with, item, open_sub, separator, sub_item, tag_item,
};

/// Facts the menu needs that are not in the summary.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuFacts {
    /// The game has an applications-menu shortcut.
    pub has_shortcut: bool,
    /// Developer Mode.
    pub developer: bool,
}

/// Where a menu opens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// At the pointer (a right-click).
    Pointer(Pos2),
    /// Hanging from a cover, a row or a button: under it, its right edges aligned.
    Below(Rect),
}

/// What the Tags submenu needs to tick tags and make new ones in place
/// ([`crate::screens::dialogs::tags::tag_items`]): the "New tag" field's text and where the
/// commands that apply them go (the caller sends them).
pub struct TagEdit<'a> {
    /// The "New tag" field's text.
    pub new_tag: &'a mut String,
    /// Commands to send.
    pub sent: &'a mut Vec<UiCommand>,
}

/// The keys of the submenus.
const SUB_TAGS: &str = "tags";
const SUB_PROTECTION: &str = "protection";

/// The game page's menu for `id` at `at` while `open`; returns the chosen action.
pub fn show(
    ctx: &egui::Context,
    lib: &LibraryView,
    id: GameId,
    at: Pos2,
    open: &mut bool,
    facts: MenuFacts,
) -> Option<GameAction> {
    show_at(ctx, lib, id, Anchor::Pointer(at), open, facts, false, None)
}

/// The menu for `id` from `anchor` while `open`; `library` adds "Open game page". With
/// `tags`, the Tags submenu ticks tags and makes new ones in place (the "New tag" field);
/// without, a tick is a [`GameAction::ToggleTag`] and "New tag…" opens the tag editor.
/// Returns the chosen action.
#[allow(clippy::too_many_arguments)]
pub fn show_at(
    ctx: &egui::Context,
    lib: &LibraryView,
    id: GameId,
    anchor: Anchor,
    open: &mut bool,
    facts: MenuFacts,
    library: bool,
    tags: Option<TagEdit<'_>>,
) -> Option<GameAction> {
    let popup_id = Id::unique(("game_menu", id));
    // Opening from the keyboard moves focus to the first item (ACCESSIBILITY §2); closing
    // hands it back to the cover (the library sets `pending_focus`).
    let seen = popup_id.with("seen");
    let first_frame = ctx.data(|d| d.get_temp::<bool>(seen)).is_none();
    ctx.data_mut(|d| d.insert_temp(seen, true));
    let focus_first = first_frame && matches!(anchor, Anchor::Below(_));
    let (popup_anchor, align) = match anchor {
        Anchor::Pointer(p) => (PopupAnchor::Position(p), RectAlign::BOTTOM_START),
        Anchor::Below(r) => (PopupAnchor::ParentRect(r), RectAlign::BOTTOM_END),
    };
    let popup = Popup::new(
        popup_id,
        ctx.clone(),
        popup_anchor,
        LayerId::new(Order::Foreground, popup_id),
    )
    .kind(PopupKind::Menu)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
    .accessible_name(strings::menu::NAME())
    .align(align)
    .open_bool(open);
    let out = context_menu::popup(popup, |ui| {
        items(ui, lib, id, facts, library, focus_first, tags)
    })
    .flatten();
    if out.is_some_and(|(_, keep)| !keep) {
        *open = false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
    }
    if !*open {
        ctx.data_mut(|d| d.remove_temp::<bool>(seen));
    }
    out.map(|(a, _)| a)
}

/// An item that cannot be chosen now: quiet, answers nothing.
fn off_item(ui: &mut Ui, label: &str, shortcut: Option<&str>) {
    ui.add_enabled_ui(false, |ui| {
        let _ = item(ui, None, label, shortcut, false);
    });
}

/// The items; returns the chosen action and whether the menu stays open.
fn items(
    ui: &mut Ui,
    lib: &LibraryView,
    id: GameId,
    facts: MenuFacts,
    library: bool,
    focus_first: bool,
    tags: Option<TagEdit<'_>>,
) -> Option<(GameAction, bool)> {
    let g = lib.get(id)?;
    let mut first = focus_first;
    let mut out: Option<(GameAction, bool)> = None;
    let mut pick = |r: Response, a: GameAction, keep: bool| {
        if std::mem::take(&mut first) {
            r.request_focus();
        }
        if r.clicked() {
            out = Some((a, keep));
        }
    };
    let missing = matches!(
        g.availability,
        Availability::Missing | Availability::DriveDisconnected | Availability::Unreadable
    );
    match open_sub(ui).as_deref() {
        Some(SUB_TAGS) => {
            let back_row = back(ui, m::TAGS());
            match tags {
                Some(edit) => {
                    // The tag rows are drawn by the tag editor: focus goes to the first row
                    // here, as `pick` gives it to the first item of the other pages.
                    if std::mem::take(&mut first) {
                        back_row.request_focus();
                    }
                    let sent =
                        crate::screens::dialogs::tags::tag_items(ui, lib, &[id], edit.new_tag);
                    edit.sent.extend(sent);
                }
                None => {
                    let p = theme::theme(ui.ctx()).palette.clone();
                    for t in &lib.tags {
                        let on = g.tags.contains(&t.id);
                        pick(
                            tag_item(ui, &t.name, p.tag_dot(&t.name), on),
                            GameAction::ToggleTag(t.id),
                            true,
                        );
                    }
                    if !lib.tags.is_empty() {
                        separator(ui);
                    }
                    // New tags are made in the tag editor.
                    pick(
                        item(ui, None, m::NEW_TAG(), None, false),
                        GameAction::EditTags,
                        false,
                    );
                }
            }
        }
        // A Steam game has no level (ADR-0031): its menu has no Protection submenu.
        Some(SUB_PROTECTION) if g.is_steam() => return None,
        Some(SUB_PROTECTION) => {
            back(ui, m::PROTECTION());
            for b in BasePreset::ALL {
                let on = g.preset == Preset::from(b);
                // Widening asks first: the menu gives way to the question.
                let keep = !widens(g, b);
                pick(
                    check_item(ui, strings::shield::preset_name(b.into()), on),
                    GameAction::Preset(b),
                    keep,
                );
            }
            separator(ui);
            pick(
                item(ui, None, m::EDIT_PROTECTION(), None, false),
                GameAction::EditProtection,
                false,
            );
        }
        _ if g.is_steam() => steam_items(ui, g, library, &mut pick, missing),
        _ => {
            let running = matches!(g.activity, Activity::Running { .. } | Activity::Lingering);
            let starting = matches!(g.activity, Activity::Launching(_) | Activity::Stopping);
            let label = if running { m::STOP } else { m::PLAY };
            if (missing && !running) || starting {
                off_item(ui, label(), Some(m::KEY_PLAY()));
            } else {
                pick(
                    item(ui, None, label(), Some(m::KEY_PLAY()), false),
                    GameAction::Play,
                    false,
                );
            }
            if library {
                pick(
                    item(ui, None, m::OPEN_PAGE(), Some(m::KEY_OPEN()), false),
                    GameAction::Open,
                    false,
                );
            }
            separator(ui);
            let fav = if g.favorite {
                m::UNFAVORITE
            } else {
                m::FAVORITE
            };
            pick(
                item(ui, None, fav(), Some(m::KEY_FAVORITE()), false),
                GameAction::Favorite,
                false,
            );
            sub_item(ui, SUB_TAGS, m::TAGS());
            sub_item(ui, SUB_PROTECTION, m::PROTECTION());
            separator(ui);
            if g.root.is_none() || missing {
                off_item(ui, m::OPEN_FOLDER(), None);
            } else {
                pick(
                    item(ui, None, m::OPEN_FOLDER(), None, false),
                    GameAction::OpenFolder,
                    false,
                );
            }
            pick(
                item(ui, None, m::OPEN_SAVES(), None, false),
                GameAction::OpenSaveData,
                false,
            );
            let shortcut = if facts.has_shortcut {
                m::REMOVE_SHORTCUT
            } else {
                m::ADD_SHORTCUT
            };
            pick(
                item(ui, None, shortcut(), None, false),
                GameAction::Shortcut(!facts.has_shortcut),
                false,
            );
            if g.activity == Activity::Idle {
                pick(
                    item(ui, None, m::RUN_OTHER(), None, false),
                    GameAction::RunOther,
                    false,
                );
            } else {
                off_item(ui, m::RUN_OTHER(), None);
            }
            pick(
                item(ui, None, m::ARTWORK(), None, false),
                GameAction::Artwork,
                false,
            );
            separator(ui);
            let hide = if g.hidden { m::UNHIDE } else { m::HIDE };
            pick(
                item(ui, None, hide(), Some(m::KEY_HIDE()), false),
                GameAction::Hide,
                false,
            );
            pick(
                danger_item_with(ui, None, m::REMOVE(), Some(m::KEY_REMOVE())),
                GameAction::Remove,
                false,
            );
            if facts.developer {
                separator(ui);
                pick(
                    item(ui, None, m::COPY_COMMAND(), None, false),
                    GameAction::CopyCommand,
                    false,
                );
                pick(
                    item(ui, None, m::INSPECT(), Some(m::KEY_INSPECT()), false),
                    GameAction::Inspect,
                    false,
                );
            }
        }
    }
    out
}

/// The items of a Steam game's menu (ADR-0031).
fn steam_items(
    ui: &mut Ui,
    g: &GameSummary,
    library: bool,
    pick: &mut impl FnMut(Response, GameAction, bool),
    missing: bool,
) {
    // Play hands the game to Steam; there is no Stop: while Steam runs it, or while it is
    // starting, Play is shown and cannot be chosen.
    if g.activity == Activity::Idle {
        pick(
            item(ui, None, m::PLAY(), Some(m::KEY_PLAY()), false),
            GameAction::Play,
            false,
        );
    } else {
        off_item(ui, m::PLAY(), Some(m::KEY_PLAY()));
    }
    if library {
        pick(
            item(ui, None, m::OPEN_PAGE(), Some(m::KEY_OPEN()), false),
            GameAction::Open,
            false,
        );
    }
    separator(ui);
    let fav = if g.favorite {
        m::UNFAVORITE
    } else {
        m::FAVORITE
    };
    pick(
        item(ui, None, fav(), Some(m::KEY_FAVORITE()), false),
        GameAction::Favorite,
        false,
    );
    sub_item(ui, SUB_TAGS, m::TAGS());
    separator(ui);
    use strings::steam::action as sa;
    pick(
        item(ui, None, sa::OPEN_IN_STEAM(), None, false),
        GameAction::OpenInSteam,
        false,
    );
    if g.steam().is_some_and(|f| f.update != SteamUpdate::UpToDate) {
        pick(
            item(ui, None, sa::UPDATE_IN_STEAM(), None, false),
            GameAction::UpdateInSteam,
            false,
        );
    }
    if missing {
        off_item(ui, m::OPEN_FOLDER(), None);
    } else {
        pick(
            item(ui, None, m::OPEN_FOLDER(), None, false),
            GameAction::OpenFolder,
            false,
        );
    }
    if g.steam().is_some_and(|f| f.prefix().is_some()) {
        pick(
            item(ui, None, sa::OPEN_STORAGE(), None, false),
            GameAction::OpenStorage,
            false,
        );
    }
    separator(ui);
    let hide = if g.hidden { m::UNHIDE } else { m::HIDE };
    pick(
        item(ui, None, hide(), Some(m::KEY_HIDE()), false),
        GameAction::Hide,
        false,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening_closes_the_menu_for_the_question() {
        let lib = crate::demo::library(crate::demo::baseline_now());
        let g = lib.games.iter().find(|g| g.preset == Preset::Offline);
        assert!(g.is_some_and(|g| widens(g, BasePreset::Online)));
        assert!(g.is_some_and(|g| !widens(g, BasePreset::Offline)));
    }
}

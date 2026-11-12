//! The per-game context menu (GAME_LIBRARY §10): right-click, `Shift+F10`, the Menu key, or
//! the game page's overflow button. Items and conditions follow the table; the Tags and
//! Protection submenus are inline groups with a caret.

use easyplay_core::ids::GameId;
use easyplay_core::model::{Activity, BasePreset};
use egui::{Id, LayerId, Order, Popup, PopupCloseBehavior, PopupKind, Pos2, Ui};

use crate::bridge::LibraryView;
use crate::screens::actions::GameAction;
use crate::strings;
use crate::strings::menu as m;
use crate::theme::icons;
use crate::widgets::context_menu::{danger_item, item, separator};

/// Facts the menu needs that are not in the summary.
#[derive(Clone, Copy, Debug, Default)]
pub struct MenuFacts {
    /// The game has an applications-menu shortcut.
    pub has_shortcut: bool,
    /// Developer Mode.
    pub developer: bool,
}

/// Show the menu for `id` at `at` while `open`; returns the chosen action.
pub fn show(
    ctx: &egui::Context,
    lib: &LibraryView,
    id: GameId,
    at: Pos2,
    open: &mut bool,
    facts: MenuFacts,
) -> Option<GameAction> {
    let mut out = None;
    let popup_id = Id::unique(("game_menu", id));
    // Opening the menu moves keyboard focus to its first item (ACCESSIBILITY §2); closing
    // it hands focus back to the card (the library sets `pending_focus`).
    let seen = popup_id.with("seen");
    let focus_first = ctx.data(|d| d.get_temp::<bool>(seen)).is_none();
    ctx.data_mut(|d| d.insert_temp(seen, true));
    Popup::new(
        popup_id,
        ctx.clone(),
        at,
        LayerId::new(Order::Foreground, popup_id),
    )
    .kind(PopupKind::Menu)
    .layout(egui::Layout::top_down_justified(egui::Align::Min))
    .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
    .accessible_name(strings::menu::NAME)
    .width(240.0)
    .open_bool(open)
    .show(|ui| out = items(ui, lib, id, facts, focus_first));
    if out.is_some() {
        *open = false;
    }
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
    }
    if !*open {
        ctx.data_mut(|d| d.remove_temp::<bool>(seen));
    }
    out
}

fn items(
    ui: &mut Ui,
    lib: &LibraryView,
    id: GameId,
    facts: MenuFacts,
    focus_first: bool,
) -> Option<GameAction> {
    let g = lib.get(id)?;
    let mut first = focus_first;
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.spacing_mut().button_padding.x = crate::theme::tokens::space::S2;
    let mut out = None;
    let mut pick = |r: egui::Response, a: GameAction| {
        if std::mem::take(&mut first) {
            r.request_focus();
        }
        if r.clicked() {
            out = Some(a);
        }
    };
    let running = g.activity != Activity::Idle;
    if running {
        pick(
            item(ui, Some(icons::STOP), m::STOP, Some("Ctrl+."), false),
            GameAction::Stop,
        );
    } else {
        pick(
            item(ui, Some(icons::PLAY), m::PLAY, Some("Ctrl+Enter"), false),
            GameAction::Play,
        );
    }
    pick(
        item(ui, Some(icons::OPEN), m::OPEN_PAGE, Some("Enter"), false),
        GameAction::Open,
    );
    pick(
        item(
            ui,
            Some(if g.favorite {
                icons::STAR_FILL
            } else {
                icons::STAR
            }),
            if g.favorite {
                m::UNFAVORITE
            } else {
                m::FAVORITE
            },
            Some("F"),
            false,
        ),
        GameAction::Favorite,
    );
    separator(ui);
    let pal = crate::theme::theme(ui.ctx()).palette.clone();
    let sub = |label: &str, icon: crate::theme::Icon| {
        egui::containers::menu::SubMenuButton::from_button(
            egui::Button::new((
                icons::icon(icon, crate::theme::tokens::icon::INLINE).color(pal.text_secondary),
                crate::theme::Type::Body.rich(label).color(pal.text_primary),
            ))
            .right_text(egui::containers::menu::SubMenuButton::arrow_atom(Some(
                pal.text_muted,
            )))
            .frame(false)
            .min_size(egui::vec2(0.0, crate::theme::tokens::control::MENU_ITEM)),
        )
    };
    let tags_btn = sub(m::TAGS, icons::TAG);
    tags_btn.ui(ui, |ui| {
        ui.set_min_width(180.0);
        for t in &lib.tags {
            let on = g.tags.contains(&t.id);
            pick(
                item(ui, Some(icons::TAG), &t.name, None, on),
                GameAction::ToggleTag(t.id),
            );
        }
        pick(
            item(ui, Some(icons::PLUS), m::NEW_TAG, Some("T"), false),
            GameAction::EditTags,
        );
    });
    let prot_btn = sub(
        m::PROTECTION,
        crate::widgets::shield::glyph(g.preset, false),
    );
    prot_btn.ui(ui, |ui| {
        ui.set_min_width(180.0);
        for p in BasePreset::ALL {
            let on = g.preset == p.into();
            pick(
                item(
                    ui,
                    Some(crate::widgets::shield::glyph(p.into(), false)),
                    strings::shield::preset_name(p.into()),
                    None,
                    on,
                ),
                GameAction::Preset(p),
            );
        }
        pick(
            item(ui, Some(icons::WRENCH), m::EDIT_PROTECTION, None, false),
            GameAction::EditProtection,
        );
    });
    separator(ui);
    if g.root.is_some() {
        pick(
            item(
                ui,
                Some(icons::FOLDER),
                m::OPEN_FOLDER,
                Some("Ctrl+O"),
                false,
            ),
            GameAction::OpenFolder,
        );
    }
    pick(
        item(ui, Some(icons::FLOPPY), m::OPEN_SAVES, None, false),
        GameAction::OpenSaveData,
    );
    pick(
        item(
            ui,
            Some(icons::LIST),
            if facts.has_shortcut {
                m::REMOVE_SHORTCUT
            } else {
                m::ADD_SHORTCUT
            },
            None,
            false,
        ),
        GameAction::Shortcut(!facts.has_shortcut),
    );
    if !running {
        pick(
            item(ui, Some(icons::TERMINAL), m::RUN_OTHER, None, false),
            GameAction::RunOther,
        );
    }
    pick(
        item(ui, Some(icons::GRID), m::ARTWORK, None, false),
        GameAction::Artwork,
    );
    pick(
        item(
            ui,
            Some(icons::EYE_OFF),
            if g.hidden { m::UNHIDE } else { m::HIDE },
            Some("H"),
            false,
        ),
        GameAction::Hide,
    );
    if facts.developer {
        pick(
            item(ui, Some(icons::COPY), m::COPY_COMMAND, None, false),
            GameAction::CopyCommand,
        );
        pick(
            item(ui, Some(icons::SEARCH), m::INSPECT, Some("Ctrl+L"), false),
            GameAction::Inspect,
        );
    }
    separator(ui);
    pick(
        danger_item(ui, Some(icons::TRASH), m::REMOVE),
        GameAction::Remove,
    );
    out
}

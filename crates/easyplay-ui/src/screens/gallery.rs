//! The widget gallery: a debug route (`F12` in debug builds) that shows every control and
//! part of the design in its states, with live theme, accent and reduced-motion switches.
//! Sample text comes from `strings::gallery`.

use easyplay_core::model::{Activity, Availability, LaunchStateTag, Preset};
use egui::{Popup, PopupCloseBehavior, Rect, RectAlign, ScrollArea, Sense, Ui, pos2, vec2};

use super::Env;
use crate::app::UiState;
use crate::route::Overlay;
use crate::strings;
use crate::strings::gallery as g;
use crate::theme::tokens::{control, icon as icon_size, layout, space};
use crate::theme::{self, Accent, Appearance, Density, ThemeChoice, Type, icons};
use crate::vm::toasts::{ToastAction, ToastKind};
use crate::widgets::banner::{Note, Severity, banner};
use crate::widgets::bar_chart::{self, Bar};
use crate::widgets::button::{
    Button, PillSize, PlayState, Size, Variant, play_disc, play_disc_blocked, play_pill,
    play_pill_blocked,
};
use crate::widgets::card::{CardCtx, WallLook, wall_card};
use crate::widgets::choice::{PickKind, checkbox, pick, swatch};
use crate::widgets::context_menu::{
    self, check_item_with, danger_item, option_item, section, separator, sub_item, tag_item,
    unavailable_item,
};
use crate::widgets::cover::{self, CoverLook, CoverSource};
use crate::widgets::empty_state::blank;
use crate::widgets::icon_button::IconButton;
use crate::widgets::key_hint::key_hint;
use crate::widgets::progress;
use crate::widgets::rows::{
    Col, Leads, SettingRow, door, fact_text, heading, heading_note, inline_icon, reach, table_row,
    table_text,
};
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::{select, select_inline};
use crate::widgets::skeleton;
use crate::widgets::slider::slider;
use crate::widgets::tabs::Tabs;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::{switch, toggle};
use crate::widgets::word::{Word, word_on, word_rule, word_strong, word_text};

/// Gallery-local state: what its sample controls hold.
#[derive(Clone, Debug, Default)]
pub struct GalleryState {
    toggle: bool,
    switch: bool,
    check: bool,
    pick: usize,
    swatch: usize,
    slider: f32,
    name: String,
    empty_name: String,
    search: String,
    mono: String,
    segment: usize,
    view: usize,
    select: usize,
    inline: usize,
    tab: usize,
    scope: usize,
    chart_table: bool,
    chart_focus: Option<usize>,
}

/// The icons the design keeps.
const ICONS: [theme::Icon; 21] = [
    icons::SEARCH,
    icons::PLUS,
    icons::CLOSE,
    icons::CHECK,
    icons::SHIELD_STANDARD,
    icons::CARET_DOWN,
    icons::CARET_RIGHT,
    icons::CARET_LEFT,
    icons::ARROW_UP,
    icons::ARROW_DOWN,
    icons::PLAY,
    icons::STOP,
    icons::STAR,
    icons::DOTS,
    icons::GRID,
    icons::LIST,
    icons::OPEN,
    icons::WARNING,
    icons::CLOCK,
    icons::CHECK_CIRCLE,
    icons::ERROR,
];

/// A section: the design's heading (13 / 600, 26 above).
fn section_head(ui: &mut Ui, title: &str) {
    ui.add_space(space::S4);
    heading(ui, title, false);
}

/// A line of parts, 8 apart, wrapping.
fn line(ui: &mut Ui, content: impl FnOnce(&mut Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(space::S2, space::S2);
        content(ui);
    });
    ui.add_space(space::S2);
}

/// Theme, accent and reduced-motion switches.
pub fn appearance_panel(ui: &mut Ui, _g: &mut GalleryState) {
    let ctx = ui.ctx().clone();
    let mut a: Appearance = theme::appearance(&ctx);
    let before = a;
    let mut t = ThemeChoice::ALL
        .iter()
        .position(|c| *c == a.theme)
        .unwrap_or(0);
    let topts: Vec<Segment<'_>> = ThemeChoice::ALL
        .iter()
        .map(|c| Segment {
            icon: None,
            label: strings::appearance::theme(*c),
        })
        .collect();
    SettingRow::new(g::THEME()).show(ui, |ui| {
        if segmented(ui, g::THEME(), &mut t, &topts, false).changed() {
            a.theme = ThemeChoice::ALL[t];
        }
    });
    let mut ac = Accent::ALL.iter().position(|x| *x == a.accent).unwrap_or(0);
    let aopts: Vec<Segment<'_>> = Accent::ALL
        .iter()
        .map(|x| Segment {
            icon: None,
            label: strings::appearance::accent(*x),
        })
        .collect();
    SettingRow::new(g::ACCENT()).show(ui, |ui| {
        if segmented(ui, g::ACCENT(), &mut ac, &aopts, false).changed() {
            a.accent = Accent::ALL[ac];
        }
    });
    let mut rm = a.reduce_motion;
    SettingRow::new(g::REDUCE_MOTION())
        .help(g::REDUCE_MOTION_HINT())
        .show(ui, |ui| {
            if switch(ui, &mut rm, g::REDUCE_MOTION()).changed() {
                a.reduce_motion = rm;
            }
        });
    if a != before {
        theme::apply(&ctx, &a);
    }
}

/// Show the gallery.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let m = super::library::margin(ui);
    ScrollArea::vertical()
        .id_salt("gallery")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(m as i8, space::S6 as i8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    ui.label(Type::Title.rich(strings::nav::GALLERY()).color(p.t1));
                    section_head(ui, g::APPEARANCE());
                    ui.scope(|ui| {
                        ui.set_max_width(layout::COL_MAX);
                        appearance_panel(ui, &mut state.gallery);
                    });
                    type_and_icons(ui);
                    buttons(ui);
                    plays(ui);
                    words(ui, &mut state.gallery);
                    inputs(ui, &mut state.gallery);
                    rows(ui);
                    notes(ui);
                    loading(ui);
                    section_head(ui, g::BLANK());
                    blank(
                        ui,
                        g::BLANK_TITLE(),
                        g::BLANK_TEXT(),
                        |ui| {
                            ui.add(
                                Button::primary(strings::library::ADD_GAME())
                                    .size(Size::Lg)
                                    .icon(icons::PLUS),
                            )
                        },
                        Some(g::BLANK_FINE()),
                    );
                    covers(ui, env);
                    marks(ui, &mut state.gallery);
                    overlays(ui, state, env);
                });
        });
}

/// Every type role, and the icons in their two weights.
fn type_and_icons(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    section_head(ui, g::TYPE());
    for t in Type::ALL {
        ui.label(
            t.rich(format!("{} · {} / {}", t.name(), t.size(), t.line_height()))
                .color(p.t1),
        );
    }
    ui.label(Type::Display.numeric(g::CLOCK_SAMPLE()).color(p.t1));
    section_head(ui, g::ICONS());
    for filled in [false, true] {
        line(ui, |ui| {
            for i in ICONS {
                inline_glyph(ui, i, filled, icon_size::INLINE);
                inline_glyph(ui, i, filled, icon_size::SMALL);
            }
        });
    }
}

/// One glyph, named after itself for assistive technology.
fn inline_glyph(ui: &mut Ui, icon: theme::Icon, filled: bool, size: f32) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon.text(filled),
        icons::font(size),
        p.t2,
    );
    crate::a11y::labelled(ui.ctx(), resp.id, egui::Role::Image, icon.name);
}

/// Every variant at every size, with an icon, busy, disabled, a caret, a count; icon
/// buttons; a key hint.
fn buttons(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    section_head(ui, g::BUTTONS());
    for size in [Size::Xs, Size::Sm, Size::Md, Size::Lg] {
        line(ui, |ui| {
            ui.add(Button::primary(g::PRIMARY()).icon(icons::PLUS).size(size));
            ui.add(Button::new(g::SECONDARY()).size(size));
            ui.add(Button::ghost(g::GHOST()).icon(icons::OPEN).size(size));
            ui.add(Button::new(g::DANGER()).variant(Variant::Danger).size(size));
            ui.add(Button::new(g::WARN()).variant(Variant::Warn).size(size));
            ui.add(Button::new(g::LINK()).variant(Variant::Link).size(size));
            ui.add(Button::new(g::BUSY()).busy(true).size(size));
            ui.add(
                Button::primary(g::DISABLED())
                    .disabled(Some(g::DISABLED_REASON()))
                    .size(size),
            );
        });
    }
    line(ui, |ui| {
        ui.add(Button::new(g::MENU()).caret());
        ui.add(Button::ghost(g::MENU()).caret().open(true));
        ui.add(Button::ghost(g::COUNTED()).badge(2));
        ui.add(
            Button::ghost(strings::library::ADD_GAME())
                .icon(icons::PLUS)
                .icon_only(true)
                .shortcut(strings::library::KEY_ADD()),
        );
        ui.add(IconButton::new(icons::DOTS, strings::menu::NAME()));
        ui.add(IconButton::new(icons::GRID, strings::library::VIEW_GRID()).selected(true));
        ui.add(IconButton::new(icons::PLAY, strings::play::PLAY()).filled(true));
        ui.add(
            IconButton::new(icons::STAR, strings::card::FAVORITE())
                .round(true)
                .toggle(true),
        );
        ui.add(IconButton::new(icons::DOTS, strings::menu::NAME()).round(true));
        key_hint(ui, g::SHORTCUT());
        ui.label(Type::Caption.rich(g::SHORTCUT_PLAY()).color(p.t3));
    });
}

/// Play in every state and every place: the page's pill, the tab bar's, a list row's, a
/// cover's disc; and while another game runs.
fn plays(ui: &mut Ui) {
    section_head(ui, g::PLAY());
    let states = [
        PlayState::Ready,
        PlayState::Launching(strings::card::phase(LaunchStateTag::Preparing)),
        PlayState::Running,
        PlayState::Stopping,
        PlayState::Unavailable,
        PlayState::NeedsRuntime,
        PlayState::Installer,
    ];
    for size in [PillSize::Page, PillSize::Compact, PillSize::Row] {
        line(ui, |ui| {
            for s in &states {
                play_pill(ui, s, g::SAMPLE_GAME(), size, 1.0);
            }
            play_pill_blocked(ui, g::SAMPLE_GAME(), g::BLOCKED(), size, 1.0);
        });
    }
    // The discs, on a cover's corner colour.
    let p = theme::theme(ui.ctx()).palette.clone();
    let n = states.len() + 1;
    let big = control::DISC_BIG;
    let (strip, _) = ui.allocate_exact_size(
        vec2(n as f32 * (big + space::S3), big + space::S3 * 2.0),
        Sense::hover(),
    );
    ui.painter().rect_filled(
        strip,
        egui::CornerRadius::same(theme::tokens::radius::MD),
        p.o2,
    );
    for k in 0..n {
        let size = if k % 2 == 0 { control::DISC } else { big };
        let c = pos2(
            strip.left() + space::S3 + k as f32 * (big + space::S3) + big / 2.0,
            strip.center().y,
        );
        let rect = Rect::from_center_size(c, vec2(size, size));
        let id = egui::Id::unique(("gallery.disc", k));
        match states.get(k) {
            Some(s) => {
                play_disc(ui, rect, id, s, g::SAMPLE_GAME(), 1.0);
            }
            None => {
                play_disc_blocked(ui, rect, id, g::SAMPLE_GAME(), g::BLOCKED(), 1.0);
            }
        }
    }
    ui.add_space(space::S2);
}

/// The filter line's words: scopes with counts, the rule, a menu word, a filter that is on,
/// the count, the sort; then the selection line's.
fn words(ui: &mut Ui, s: &mut GalleryState) {
    section_head(ui, g::WORDS());
    let p = theme::theme(ui.ctx()).palette.clone();
    let scopes = [
        (strings::collection::ALL(), 17),
        (strings::collection::FAVORITES(), 3),
        (strings::collection::RECENT(), 11),
    ];
    line(ui, |ui| {
        for (i, (label, n)) in scopes.iter().enumerate() {
            if ui
                .add(Word::new(label).count(*n).pressed(s.scope == i))
                .clicked()
            {
                s.scope = i;
            }
        }
        word_rule(ui);
        menu_word(ui);
        word_on(ui, g::TAG(), Some(p.tag_dot(g::TAG())), g::REMOVE_TAG());
        word_on(ui, g::MENU_CHECKS()[0], None, g::REMOVE_TAG());
        word_text(ui, g::COUNT());
        ui.add(Word::new(g::SORT()).caret());
    });
    line(ui, |ui| {
        ui.add(
            Word::new(g::CLEAR())
                .icon(icons::CLOSE)
                .icon_only()
                .tip(g::CLEAR(), Some(strings::library::KEY_ESC())),
        );
        word_strong(ui, g::SELECTED());
        ui.add(Word::new(strings::library::SEL_FAVOURITE()));
        ui.add(Word::new(strings::library::SEL_TAGS()).open(true));
        ui.add(Word::new(g::TAG()).dot(p.tag_dot(g::TAG())));
        ui.add(Word::new(g::REMOVE()));
        ui.add(Word::new(g::SORT()).strong());
    });
}

/// A word that opens a menu of every kind of item.
fn menu_word(ui: &mut Ui) {
    let resp = ui.add(Word::new(strings::library::FILTER()));
    let popup = Popup::menu(&resp)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_START);
    context_menu::popup(popup, |ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.set_min_width(layout::MENU_W[0] - layout::MENU_PAD * 2.0);
        let p = theme::theme(ui.ctx()).palette.clone();
        section(ui, g::MENU_SECTION());
        for (i, o) in g::MENU_OPTIONS().iter().enumerate() {
            let _ = option_item(ui, o, i == 0);
        }
        separator(ui);
        for (i, o) in g::MENU_CHECKS().iter().enumerate() {
            let _ = check_item_with(ui, o, i == 0, Some(&(i + 3).to_string()));
        }
        let _ = tag_item(ui, g::TAG(), p.tag_dot(g::TAG()), true);
        separator(ui);
        let _ = sub_item(ui, "gallery.sub", g::MENU_SUB());
        unavailable_item(ui, g::MENU_OFF(), g::MENU_OFF_WHY());
        let _ = danger_item(ui, None, g::REMOVE());
    });
}

/// Fields, switches, checks, picks, swatches, the slider, segmented controls, selects and
/// tabs.
fn inputs(ui: &mut Ui, s: &mut GalleryState) {
    section_head(ui, g::INPUTS());
    ui.scope(|ui| {
        ui.set_max_width(layout::COL_MAX);
        ui.spacing_mut().item_spacing.y = space::S3;
        TextField::new(&mut s.name, "gallery.name")
            .label(g::FIELD_LABEL())
            .help(g::FIELD_HELP())
            .show(ui);
        TextField::new(&mut s.empty_name, "gallery.err")
            .label(g::FIELD_LABEL())
            .error(Some(g::FIELD_ERROR()))
            .show(ui);
        TextField::new(&mut s.search, "gallery.search")
            .search()
            .hint(strings::library::SEARCH_HINT())
            .accessible_name(strings::library::SEARCH())
            .shortcut_hint(strings::library::KEY_SEARCH)
            .show(ui);
        TextField::new(&mut s.mono, "gallery.mono")
            .label(g::FIELD_MONO())
            .mono()
            .show(ui);
        ui.spacing_mut().item_spacing.y = 0.0;
        toggle(ui, &mut s.toggle, g::TOGGLE_LABEL(), Some(g::TOGGLE_HINT()));
        checkbox(ui, &mut s.check, g::CHECK_LABEL());
        for (i, (label, note)) in [(g::PICK_A(), Some(g::PICK_A_NOTE())), (g::PICK_B(), None)]
            .into_iter()
            .enumerate()
        {
            let p = theme::theme(ui.ctx()).palette.clone();
            let (r, ()) = pick(ui, PickKind::Radio, s.pick == i, label, |ui| {
                ui.label(Type::Body.rich(label).color(p.t1));
                if let Some(n) = note {
                    ui.label(Type::Caption.rich(n).color(p.t3));
                }
            });
            if r.clicked() {
                s.pick = i;
            }
        }
    });
    line(ui, |ui| {
        switch(ui, &mut s.switch, g::TOGGLE_LABEL());
        for (i, a) in Accent::ALL.iter().enumerate() {
            if swatch(ui, a.swatch(), s.swatch == i, g::SWATCH()).clicked() {
                s.swatch = i;
            }
        }
        slider(
            ui,
            &mut s.slider,
            0.0..=1.0,
            0.25,
            control::SLIDER_W,
            g::SLIDER(),
            g::SLIDER_VALUE(),
        );
    });
    line(ui, |ui| {
        let levels = [Preset::Offline, Preset::Online, Preset::Standard].map(|p| Segment {
            icon: None,
            label: strings::shield::preset_name(p),
        });
        segmented(ui, g::SEG_ACCESS(), &mut s.segment, &levels, false);
        let views = [
            Segment {
                icon: Some(icons::GRID),
                label: strings::library::VIEW_GRID(),
            },
            Segment {
                icon: Some(icons::LIST),
                label: strings::library::VIEW_LIST(),
            },
        ];
        segmented(ui, strings::library::VIEW(), &mut s.view, &views, true);
        select(
            ui,
            g::SELECT_LABEL(),
            None,
            None,
            &mut s.select,
            &g::SELECT_OPTIONS(),
            false,
        );
        select(
            ui,
            g::SELECT_LABEL(),
            None,
            None,
            &mut s.select,
            &g::SELECT_OPTIONS(),
            true,
        );
        select_inline(ui, g::INLINE_LABEL(), &mut s.inline, &g::INLINE_OPTIONS());
    });
    let _ = Tabs::new(
        g::TABS(),
        &[g::TAB_OVERVIEW(), g::TAB_PROTECTION(), g::TAB_COMPAT()],
    )
    .show_with(ui, &mut s.tab, |ui| {
        play_pill(
            ui,
            &PlayState::Ready,
            g::SAMPLE_GAME(),
            PillSize::Compact,
            1.0,
        )
    });
}

/// Headings, setting rows, facts, doors, the reach lines and a table.
fn rows(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    section_head(ui, g::ROWS());
    ui.scope(|ui| {
        ui.set_max_width(layout::COL_MAX);
        heading(ui, g::HEADING(), true);
        let mut on = true;
        SettingRow::new(g::ROW_LABEL())
            .help(g::ROW_HELP())
            .show(ui, |ui| switch(ui, &mut on, g::ROW_LABEL()));
        let mut off = false;
        SettingRow::new(g::ROW_OFF())
            .enabled(false)
            .show(ui, |ui| switch(ui, &mut off, g::ROW_OFF()));
        SettingRow::new(g::ROW_MONO())
            .mono()
            .show(ui, |ui| ui.add(Button::new(g::ROW_RUN()).size(Size::Sm)));
        heading_note(
            ui,
            g::HEADING_NOTED(),
            Some(g::HEADING_NOTE()),
            false,
            false,
        );
        fact_text(ui, g::FACT_LABEL(), g::FACT_VALUE());
        door(ui, g::DOOR_LABEL(), g::DOOR_NAME(), Leads::Tab, |ui| {
            ui.label(Type::Body.rich(g::DOOR_VALUE()).color(p.t1));
        });
        // The shield beside a protection level, inline on a line of text.
        ui.horizontal(|ui| {
            inline_icon(ui, icons::SHIELD_STANDARD, p.t1);
            ui.label(Type::Body.rich(g::DOOR_VALUE()).color(p.t1));
        });
        door(
            ui,
            g::DOOR_FOLDER(),
            g::DOOR_FOLDER_NAME(),
            Leads::Outside,
            |ui| {
                ui.label(Type::Mono.rich(g::DOOR_PATH()).color(p.t1));
            },
        );
        ui.add_space(space::S3);
        reach(ui, g::CAN(), g::CAN_LIST(), g::CANT(), g::CANT_LIST());
        ui.add_space(space::S3);
        let [_, last_w, time_w, _] = layout::LIST_COLS;
        let cols = [Col::Flex(1.0), Col::Fixed(time_w), Col::Fixed(last_w)];
        table_row(ui, &cols, true, |ui, i| {
            table_text(ui, g::TABLE_HEAD()[i], i, true, false);
        });
        for r in g::TABLE_ROWS() {
            table_row(ui, &cols, false, |ui, i| {
                table_text(ui, r[i], i, false, i > 0);
            });
        }
    });
}

/// Banners and the game page's notes.
fn notes(ui: &mut Ui) {
    section_head(ui, g::NOTES());
    ui.spacing_mut().item_spacing.y = space::S2;
    banner(
        ui,
        Severity::Info,
        g::BANNER_INFO(),
        Some(g::BANNER_ACTION()),
        true,
    );
    banner(ui, Severity::Warning, g::BANNER_WARNING(), None, true);
    ui.scope(|ui| {
        ui.set_max_width(layout::COL_MAX);
        let _ = Note::problem(g::NOTE_PROBLEM(), g::NOTE_PROBLEM_TEXT())
            .code(g::NOTE_CODE())
            .actions(&g::NOTE_ACTIONS())
            .show(ui);
        let _ = Note::missing(g::NOTE_MISSING(), g::NOTE_MISSING_TEXT()).show(ui);
        let _ = Note::lingering(g::NOTE_LINGERING(), g::NOTE_LINGERING_TEXT()).show(ui);
    });
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// Progress bars, spinners and loading blocks (pulsing column by column).
fn loading(ui: &mut Ui) {
    let p = theme::theme(ui.ctx()).palette.clone();
    section_head(ui, g::LOADING());
    line(ui, |ui| {
        ui.vertical(|ui| {
            ui.set_width(control::SLIDER_W * 2.0);
            ui.spacing_mut().item_spacing.y = space::S3;
            progress::linear(ui, Some(0.6), g::PROGRESS());
            progress::linear(ui, None, g::PROGRESS());
        });
        progress::spinner_widget(ui, p.t2);
        progress::live(ui, p.accent_mark);
    });
    let w = layout::COVER_SPARSE / 2.0;
    let (area, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), w * layout::COVER_ASPECT + space::S6),
        Sense::hover(),
    );
    for k in 0..6 {
        let rect = Rect::from_min_size(
            pos2(area.left() + k as f32 * (w + space::S4), area.top()),
            vec2(w, w * layout::COVER_ASPECT),
        );
        skeleton::cover(ui, rect, k);
    }
    let x = area.left() + 6.0 * (w + space::S4);
    skeleton::line(ui, pos2(x, area.top()), w * 2.0, space::S3);
    skeleton::line(ui, pos2(x, area.top() + space::S6), w * 1.4, space::S3);
}

/// Covers: posters at several widths (initials under 72), artwork stand-ins, missing; and
/// the wall's card in its states.
fn covers(ui: &mut Ui, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    section_head(ui, g::COVERS());
    let widths = [
        layout::LIST_THUMB,
        layout::POSTER_MIN_W - space::S6,
        layout::POSTER_MIN_W,
        Density::Compact.cover_width(),
        Density::Comfortable.cover_width(),
        layout::COVER_SPARSE,
    ];
    let tallest = widths[widths.len() - 1] * layout::COVER_ASPECT;
    let (area, _) = ui.allocate_exact_size(vec2(ui.available_width(), tallest), Sense::hover());
    let mut x = area.left();
    for (i, w) in widths.iter().enumerate() {
        let rect = Rect::from_min_size(pos2(x, area.top()), vec2(*w, w * layout::COVER_ASPECT));
        let (source, missing) = match i {
            4 => (CoverSource::Pending(theme.palette.tags[2]), false),
            5 => (CoverSource::Placeholder, true),
            _ => (CoverSource::Placeholder, false),
        };
        let title = if i % 2 == 0 {
            g::SAMPLE_GAME
        } else {
            g::LONG_NAME
        };
        cover::paint(
            ui.painter(),
            rect,
            source,
            title(),
            CoverLook {
                missing,
                ..CoverLook::default()
            },
            &theme,
        );
        x += w + space::S4;
    }
    ui.add_space(space::S6);

    // The wall's card: at rest, a favourite, selected, in controller mode, running, a small
    // library's (Play always shown, the caption under it), missing.
    let lib = crate::demo::library(env.now);
    let find = |name: &str| lib.games.iter().find(|x| &*x.name == name).cloned();
    let mut samples = Vec::new();
    if let Some(mut a) = lib.games.first().cloned() {
        a.name = g::SHORT_NAME().into();
        a.activity = Activity::Idle;
        a.availability = Availability::Present;
        a.favorite = false;
        samples.push(a);
    }
    for name in [
        "Lantern Keep",
        "Moss & Iron",
        "Low Tide Motel",
        "Starfall Tactics",
        "Harbor Lights",
        "Paper Kingdoms",
    ] {
        if let Some(s) = find(name) {
            samples.push(s);
        }
    }
    let w = Density::Comfortable.cover_width();
    let gap = layout::WALL_GAP_X;
    let caption = layout::CAPTION_GAP + layout::CAPTION_LINE * 3.0;
    let per_row = (((ui.available_width() + gap) / (w + gap)).floor() as usize).max(1);
    let row_h = w * layout::COVER_ASPECT + caption + space::S6;
    let rows = samples.len().div_ceil(per_row);
    let (area, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), rows as f32 * row_h),
        Sense::hover(),
    );
    for (i, game) in samples.iter().enumerate() {
        let (row, col) = (i / per_row, i % per_row);
        let rect = Rect::from_min_size(
            pos2(
                area.left() + col as f32 * (w + gap),
                area.top() + space::S2 + row as f32 * row_h,
            ),
            vec2(w, w * layout::COVER_ASPECT),
        );
        let ctx = CardCtx {
            texture: None,
            placeholder: None,
            now: env.now,
            utc_offset_s: env.utc_offset_s,
            selected: false,
            multi_selected: i == 2,
            controller: i == 3,
            focusable: false,
            show_playtime: true,
            density: Density::Comfortable,
            opacity: 1.0,
        };
        let look = WallLook {
            big: i == 1,
            sparse: i == 1 || i == 2,
            ..WallLook::default()
        };
        let _ = wall_card(
            ui,
            rect,
            egui::Id::unique(("gallery.card", i)),
            game,
            &ctx,
            &look,
        );
    }
}

/// The mark at its sizes, a week's chart (and its table) and a strip.
fn marks(ui: &mut Ui, s: &mut GalleryState) {
    let p = theme::theme(ui.ctx()).palette.clone();
    section_head(ui, g::MARKS());
    line(ui, |ui| {
        for size in [icon_size::INLINE, control::BUTTON, control::PLAY] {
            let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
            crate::widgets::brand::paint_mark(ui.painter(), rect, p.accent_mark, p.bg);
        }
    });
    let minutes = [0.0, 130.0, 0.0, 45.0, 0.0, 0.0, 45.0];
    let bars: Vec<Bar> = g::CHART_DAYS()
        .iter()
        .zip(g::CHART_VALUES())
        .zip(minutes)
        .map(|((day, value), m)| Bar {
            label: (*day).to_owned(),
            spoken: format!("{day}: {value}"),
            value_text: value.to_owned(),
            value: m,
        })
        .collect();
    ui.scope(|ui| {
        ui.set_max_width(layout::COL_MAX);
        bar_chart::chart(
            ui,
            g::CHART_NAME(),
            &bars,
            &mut s.chart_table,
            &mut s.chart_focus,
        );
    });
}

/// Buttons that open the overlays: a sheet, a dialog, a toast.
fn overlays(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    section_head(ui, g::OVERLAYS());
    line(ui, |ui| {
        if ui.add(Button::new(g::OPEN_SHEET())).clicked() {
            state.nav.open(Overlay::Shortcuts);
        }
        if ui.add(Button::new(g::OPEN_DIALOG())).clicked() {
            state.show_dialog(crate::vm::dialogs::Dialog::Confirm(
                crate::vm::dialogs::Confirm::ClearStatistics,
            ));
        }
        if ui.add(Button::new(g::SHOW_TOAST())).clicked() {
            state.toasts.push(
                g::TOAST_TEXT(),
                ToastKind::Success,
                Some(ToastAction {
                    label: strings::feedback::UNDO().into(),
                    key: 0,
                }),
                env.mono,
            );
        }
    });
}

//! Widget gallery: a debug-only route showing every widget in every state (UI-004/UI-005
//! acceptance), with live theme, accent and reduced-motion switches (UI-002 acceptance).
//! Reached with `F12` in debug builds; sample text comes from `strings::gallery`.

use easyplay_core::model::{Activity, Availability, BasePreset, LaunchStateTag, Preset};
use egui::{Rect, ScrollArea, Ui, pos2, vec2};

use super::Env;
use crate::app::UiState;
use crate::route::Overlay;
use crate::strings;
use crate::theme::tokens::{icon as icon_size, space};
use crate::theme::{self, Accent, Appearance, Density, ThemeChoice, Type, icons};
use crate::vm::toasts::{ToastAction, ToastKind};
use crate::widgets::banner::{Severity, banner};
use crate::widgets::bar_chart::Bar;
use crate::widgets::button::{Button, PillSize, PlayState, Size, Variant, play_pill};
use crate::widgets::card::{CardCtx, card};
use crate::widgets::chip::{ChipKind, chip};
use crate::widgets::empty_state::empty_state;
use crate::widgets::icon_button::IconButton;
use crate::widgets::key_hint::key_hint;
use crate::widgets::link_row::{LinkRow, Point, Points, Tone, control_row, link_rows};
use crate::widgets::path_row::path_row;
use crate::widgets::progress;
use crate::widgets::segmented::{Segment, segmented};
use crate::widgets::select::select;
use crate::widgets::shield;
use crate::widgets::skeleton;
use crate::widgets::stat_tile::{Fact, stat_tile};
use crate::widgets::tabs::tabs;
use crate::widgets::text_field::TextField;
use crate::widgets::toggle::toggle;

/// Gallery-local state.
#[derive(Clone, Debug, Default)]
pub struct GalleryState {
    toggle: bool,
    name: String,
    empty_name: String,
    search: String,
    segment: usize,
    select: usize,
    tab: usize,
    chip: bool,
    access: usize,
}

fn section(ui: &mut Ui, title: &str) {
    let theme = theme::theme(ui.ctx());
    ui.add_space(space::S6);
    ui.label(Type::Heading.rich(title).color(theme.palette.text_primary));
    ui.add_space(space::S2);
}

/// Theme, accent and reduced-motion controls (also used by the Settings placeholder).
pub fn appearance_panel(ui: &mut Ui, g: &mut GalleryState) {
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
    ui.horizontal(|ui| {
        ui.label(Type::Body.rich(strings::gallery::THEME));
        if segmented(ui, strings::gallery::THEME, &mut t, &topts, false).changed() {
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
    ui.horizontal(|ui| {
        ui.label(Type::Body.rich(strings::gallery::ACCENT));
        if segmented(ui, strings::gallery::ACCENT, &mut ac, &aopts, false).changed() {
            a.accent = Accent::ALL[ac];
        }
    });
    let mut rm = a.reduce_motion;
    if toggle(
        ui,
        &mut rm,
        strings::gallery::REDUCE_MOTION,
        Some(strings::gallery::REDUCE_MOTION_HINT),
    )
    .changed()
    {
        a.reduce_motion = rm;
    }
    if a != before {
        theme::apply(&ctx, &a);
    }
    let _ = g;
}

/// Show the gallery.
pub fn show(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let m = super::library::margin(ui);
    ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(m as i8, space::S6 as i8))
                .show(ui, |ui| {
                    ui.label(
                        Type::Title
                            .rich(strings::nav::GALLERY)
                            .color(p.text_primary),
                    );
                    appearance_panel(ui, &mut state.gallery);
                    let g = &mut state.gallery;

                    section(ui, strings::gallery::TYPE);
                    for t in Type::ALL {
                        ui.label(
                            t.rich(format!("{} · {} / {}", t.name(), t.size(), t.line_height()))
                                .color(p.text_primary),
                        );
                    }
                    ui.label(
                        Type::Display
                            .numeric(strings::gallery::CLOCK_SAMPLE)
                            .color(p.text_primary),
                    );

                    section(ui, strings::gallery::ICONS);
                    ui.horizontal_wrapped(|ui| {
                        for i in icons::ALL {
                            ui.label(icons::icon(*i, icon_size::NAV).color(p.text_secondary))
                                .on_hover_text(i.name);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for i in icons::ALL {
                            ui.label(
                                icons::icon_filled(*i, icon_size::NAV).color(p.text_secondary),
                            )
                            .on_hover_text(i.name);
                        }
                    });

                    section(ui, strings::gallery::BUTTONS);
                    for size in [Size::Sm, Size::Md, Size::Lg] {
                        ui.horizontal(|ui| {
                            ui.add(
                                Button::primary(strings::gallery::PRIMARY)
                                    .icon(icons::PLUS)
                                    .size(size),
                            );
                            ui.add(Button::new(strings::gallery::SECONDARY).size(size));
                            ui.add(
                                Button::ghost(strings::gallery::GHOST)
                                    .icon(icons::FILTER)
                                    .size(size),
                            );
                            ui.add(
                                Button::new(strings::gallery::DANGER)
                                    .variant(Variant::Danger)
                                    .size(size),
                            );
                            ui.add(
                                Button::new(strings::gallery::LINK)
                                    .variant(Variant::Link)
                                    .size(size),
                            );
                            ui.add(Button::new(strings::gallery::BUSY).busy(true).size(size));
                            ui.add(
                                Button::primary(strings::gallery::DISABLED)
                                    .disabled(Some(strings::gallery::DISABLED_REASON))
                                    .size(size),
                            );
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.add(IconButton::new(icons::GEAR, strings::nav::SETTINGS));
                        ui.add(
                            IconButton::new(icons::STAR_FILL, strings::card::FAVORITE).toggle(true),
                        );
                        ui.add(
                            IconButton::new(icons::SIDEBAR, strings::nav::COLLAPSE)
                                .shortcut(strings::nav::SIDEBAR_SHORTCUT),
                        );
                        key_hint(ui, "Ctrl+Enter");
                        ui.label(
                            Type::Caption
                                .rich(strings::gallery::SHORTCUT_PLAY)
                                .color(p.text_muted),
                        );
                    });

                    section(ui, strings::gallery::PLAY);
                    let states = [
                        PlayState::Ready,
                        PlayState::Launching(strings::card::phase(LaunchStateTag::Preparing)),
                        PlayState::Running,
                        PlayState::Stopping,
                        PlayState::Unavailable,
                        PlayState::NeedsRuntime,
                        PlayState::Installer,
                    ];
                    ui.horizontal_wrapped(|ui| {
                        for s in &states {
                            play_pill(ui, s, strings::gallery::SAMPLE_GAME, PillSize::Page, 1.0);
                        }
                    });
                    ui.horizontal_wrapped(|ui| {
                        for s in &states {
                            play_pill(ui, s, strings::gallery::SAMPLE_GAME, PillSize::Row, 1.0);
                        }
                        for s in &states {
                            play_pill(ui, s, strings::gallery::SAMPLE_GAME, PillSize::Card, 1.0);
                        }
                    });

                    section(ui, strings::gallery::INPUTS);
                    ui.scope(|ui| {
                        ui.set_max_width(420.0);
                        TextField::new(&mut g.name, "gallery.name")
                            .label(strings::gallery::FIELD_LABEL)
                            .help(strings::gallery::FIELD_HELP)
                            .show(ui);
                        TextField::new(&mut g.empty_name, "gallery.err")
                            .label(strings::gallery::FIELD_LABEL)
                            .error(Some(strings::gallery::FIELD_ERROR))
                            .show(ui);
                        TextField::new(&mut g.search, "gallery.search")
                            .search()
                            .hint(strings::library::SEARCH_HINT)
                            .accessible_name(strings::library::SEARCH)
                            .show(ui);
                        toggle(
                            ui,
                            &mut g.toggle,
                            strings::gallery::TOGGLE_LABEL,
                            Some(strings::gallery::TOGGLE_HINT),
                        );
                    });
                    ui.horizontal(|ui| {
                        let opts = [
                            Segment {
                                icon: Some(icons::SHIELD_OFFLINE),
                                label: strings::shield::preset_name(Preset::Offline),
                            },
                            Segment {
                                icon: Some(icons::SHIELD_ONLINE),
                                label: strings::shield::preset_name(Preset::Online),
                            },
                            Segment {
                                icon: Some(icons::SHIELD_STANDARD),
                                label: strings::shield::preset_name(Preset::Standard),
                            },
                        ];
                        segmented(
                            ui,
                            strings::gallery::SEG_ACCESS,
                            &mut g.segment,
                            &opts,
                            false,
                        );
                        let labels = [
                            strings::library::SORT_NAME,
                            strings::library::SORT_RECENT,
                            strings::library::SORT_PLAYTIME,
                        ];
                        select(
                            ui,
                            strings::gallery::SELECT_LABEL,
                            None,
                            None,
                            &mut g.select,
                            &labels,
                            false,
                        );
                    });
                    ui.horizontal(|ui| {
                        let on = g.chip;
                        if chip(
                            ui,
                            ChipKind::Filter { selected: on },
                            strings::gallery::CHIP_OFFLINE,
                            Some(icons::WIFI_OFF),
                            false,
                        )
                        .response
                        .clicked()
                        {
                            g.chip = !on;
                        }
                        chip(
                            ui,
                            ChipKind::Tag {
                                color: 1,
                                removable: true,
                            },
                            strings::gallery::CHIP_RPG,
                            None,
                            false,
                        );
                        chip(
                            ui,
                            ChipKind::Tag {
                                color: 0,
                                removable: false,
                            },
                            strings::gallery::CHIP_COOP,
                            None,
                            true,
                        );
                        chip(ui, ChipKind::Status, strings::card::INSTALLER, None, true);
                    });
                    tabs(
                        ui,
                        strings::gallery::TABS,
                        &mut g.tab,
                        &[
                            strings::gallery::TAB_OVERVIEW,
                            strings::gallery::TAB_PROTECTION,
                            strings::gallery::TAB_COMPAT,
                        ],
                    );
                    path_row(
                        ui,
                        icons::FOLDER,
                        strings::gallery::PATH_NAME,
                        strings::gallery::PATH_LOCATION,
                        strings::gallery::PATH_FULL,
                        strings::gallery::PATH_NAME,
                        &mut g.access,
                        [strings::gallery::SEG_READ, strings::gallery::SEG_WRITE],
                        strings::gallery::REMOVE,
                    );

                    section(ui, strings::gallery::DISPLAY);
                    ui.horizontal(|ui| {
                        for preset in [
                            Preset::Offline,
                            Preset::Online,
                            Preset::Standard,
                            Preset::Custom {
                                base: BasePreset::Offline,
                            },
                        ] {
                            shield::badge(ui, preset, false, icon_size::NAV);
                        }
                        shield::badge(ui, Preset::Online, true, icon_size::NAV);
                    });
                    ui.horizontal(|ui| {
                        ui.scope(|ui| {
                            ui.set_width(220.0);
                            stat_tile(
                                ui,
                                strings::gallery::STAT_LABEL,
                                strings::gallery::STAT_VALUE,
                                strings::gallery::STAT_SPOKEN,
                                Some(strings::gallery::STAT_CAPTION),
                            );
                        });
                        ui.vertical(|ui| {
                            ui.set_width(240.0);
                            progress::linear(ui, Some(0.6), strings::gallery::PROGRESS);
                            ui.add_space(space::S3);
                            progress::linear(ui, None, strings::gallery::PROGRESS);
                            ui.add_space(space::S3);
                            let (r, _) =
                                ui.allocate_exact_size(vec2(240.0, 40.0), egui::Sense::hover());
                            skeleton::line(ui, r.min, 200.0, 12.0);
                            skeleton::line(ui, r.min + vec2(0.0, 20.0), 140.0, 12.0);
                        });
                    });
                    facts_and_rows(ui);
                    banner(
                        ui,
                        Severity::Info,
                        strings::gallery::BANNER_INFO,
                        Some(strings::gallery::BANNER_ACTION),
                        true,
                    );
                    ui.add_space(space::S2);
                    banner(
                        ui,
                        Severity::Warning,
                        strings::gallery::BANNER_WARNING,
                        None,
                        true,
                    );
                    ui.add_space(space::S4);
                    empty_state(
                        ui,
                        icons::FILE_EXE,
                        strings::library::EMPTY_TITLE,
                        strings::library::EMPTY_BODY,
                        Some(Button::primary(strings::library::ADD_GAME).icon(icons::PLUS)),
                    );

                    section(ui, strings::gallery::CARDS);
                    card_matrix(ui, env);

                    section(ui, strings::gallery::OVERLAYS);
                    ui.horizontal(|ui| {
                        if ui.add(Button::new(strings::gallery::OPEN_SHEET)).clicked() {
                            state.nav.open(Overlay::Shortcuts);
                        }
                        if ui.add(Button::new(strings::gallery::OPEN_DIALOG)).clicked() {
                            state.show_dialog(crate::vm::dialogs::Dialog::Confirm(
                                crate::vm::dialogs::Confirm::ClearStatistics,
                            ));
                        }
                        if ui.add(Button::new(strings::gallery::SHOW_TOAST)).clicked() {
                            state.toasts.push(
                                strings::gallery::TOAST_TEXT,
                                ToastKind::Success,
                                Some(ToastAction {
                                    label: strings::feedback::UNDO.into(),
                                    key: 0,
                                }),
                                env.mono,
                            );
                        }
                    });
                });
        });
}

/// Every card state at the current density, short and long names (UI-005 acceptance).
/// A line of facts (one with a strip) and a list of link rows: with points, with a detail
/// that needs attention, opening outside, and a row of controls.
fn facts_and_rows(ui: &mut Ui) {
    use strings::gallery as g;
    let p = theme::theme(ui.ctx()).palette.clone();
    let week: Vec<Bar> = [0.0, 130.0, 0.0, 45.0, 0.0, 0.0, 45.0]
        .iter()
        .map(|v| Bar {
            label: String::new(),
            spoken: g::WEEK_DAY.to_owned(),
            value_text: String::new(),
            value: *v,
        })
        .collect();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = space::S8;
        Fact {
            label: g::FACT_LABEL,
            value: g::FACT_VALUE,
            spoken: &strings::details::fact_name(g::FACT_LABEL, g::FACT_VALUE),
            strip: None,
            fg: p.text_primary,
            fg2: p.text_secondary,
        }
        .show(ui);
        Fact {
            label: g::WEEK_LABEL,
            value: g::WEEK_VALUE,
            spoken: &strings::details::fact_name(g::WEEK_LABEL, g::WEEK_VALUE),
            strip: Some(&week),
            fg: p.text_primary,
            fg2: p.text_secondary,
        }
        .show(ui);
    });
    ui.add_space(space::S3);
    let points = [
        Point {
            icon: icons::WIFI_OFF,
            text: g::ROW_POINTS[0],
            tone: Tone::Normal,
        },
        Point {
            icon: icons::FOLDER_LOCK,
            text: g::ROW_POINTS[1],
            tone: Tone::Normal,
        },
        Point {
            icon: icons::CAMERA,
            text: g::ROW_POINTS[2],
            tone: Tone::Warning,
        },
    ];
    let rows = [
        LinkRow::new(
            icons::SHIELD_OFFLINE,
            g::ROW_PROTECTION,
            g::ROW_PRESET,
            g::ROW_PROTECTION,
        )
        .points(Points::Listed {
            name: g::ROW_REACH,
            items: &points,
            more: None,
        }),
        LinkRow::new(
            icons::SHIELD_ALERT,
            g::TAB_COMPAT,
            g::CHIP_OFFLINE,
            g::TAB_COMPAT,
        )
        .icon_tone(Tone::Danger)
        .detail(Some(g::ROW_ATTENTION), Tone::Danger),
        LinkRow::new(icons::FOLDER, g::ROW_FOLDER, g::PATH_NAME, g::ROW_FOLDER)
            .detail(Some(g::PATH_LOCATION), Tone::Normal)
            .external(g::ROW_FOLDER_HINT, None),
    ];
    link_rows(ui, egui::Id::unique("gallery.rows"), &rows);
    control_row(ui, icons::FILE_EXE, g::ROW_PROGRAMS, false, |ui| {
        ui.add(Button::new(g::ROW_TOOL).size(Size::Sm).icon(icons::PLAY));
    });
    ui.add_space(space::S3);
}

fn card_matrix(ui: &mut Ui, env: &mut Env<'_>) {
    let theme = theme::theme(ui.ctx());
    let lib = crate::demo::library(env.now);
    let mut samples = lib.games.to_vec();
    if let Some(first) = samples.first().cloned() {
        let mut a = first.clone();
        a.name = strings::gallery::SHORT_NAME.into();
        a.activity = Activity::Idle;
        a.availability = Availability::Present;
        samples.insert(0, a);
        let mut b = first;
        b.name = strings::gallery::LONG_NAME.into();
        samples.insert(1, b);
    }
    for density in [Density::Compact, Density::Comfortable, Density::Large] {
        let w = density.card_min_width();
        let h = w * 1.5 + crate::widgets::card::caption_height(density);
        let per_row = ((ui.available_width() + space::S4) / (w + space::S4))
            .floor()
            .max(1.0) as usize;
        let shown = samples.len().min(per_row * 2);
        let rows = shown.div_ceil(per_row);
        let (area, _) = ui.allocate_exact_size(
            vec2(ui.available_width(), rows as f32 * (h + space::S4)),
            egui::Sense::hover(),
        );
        for (i, gsum) in samples.iter().take(shown).enumerate() {
            let rect = Rect::from_min_size(
                pos2(
                    area.left() + (i % per_row) as f32 * (w + space::S4),
                    area.top() + (i / per_row) as f32 * (h + space::S4),
                ),
                vec2(w, h),
            );
            let ctx = CardCtx {
                texture: None,
                placeholder: None,
                now: env.now,
                utc_offset_s: env.utc_offset_s,
                selected: false,
                multi_selected: i == 3,
                controller: i == 2,
                focusable: true,
                show_playtime: true,
                density,
                opacity: 1.0,
            };
            card(
                ui,
                rect,
                egui::Id::unique(("gallery.card", density as u8, i)),
                gsum,
                &ctx,
            );
        }
    }
    let _ = theme;
}

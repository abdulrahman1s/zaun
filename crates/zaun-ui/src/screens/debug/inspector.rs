//! The launch inspector (DEVELOPER_MODE §3, DEV-002): Summary, Access, Command, Sandbox,
//! Environment, Processes, Events, Raw. Every value shown comes from the structures the
//! launcher executes (`InspectorData`).
//!
//! The design does not draw it; it is built from the design's parts: the wide dialog, tabs
//! 14 apart (the dialog's column is narrow), facts for the summary, 15 pt headings, table
//! rows (34 high over a hairline, a 28 header in 12 pt quiet text) whose cells are
//! selectable mono text, quiet text buttons for the copy actions, and rows that open like
//! doors for the access statements. Nothing is boxed. Only the rows of a table that are in
//! view are drawn.

use egui::{Align2, Color32, Context, CornerRadius, Id, Rect, Sense, Ui, pos2, vec2};
use zaun_core::launch::inspect::{self, MountRow};
use zaun_core::launch::{InspectorData, InspectorSource};

use super::super::Env;
use crate::a11y;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::theme::palette::mix;
use crate::theme::tokens::{debug as dbg, game, icon as icon_size, layout, radius};
use crate::theme::{self, Type, icons};
use crate::vm::AsyncValue;
use crate::vm::debug::{InspectorTab, InspectorVm};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::choice::checkbox;
use crate::widgets::modal::fine_print;
use crate::widgets::progress::spinner_widget;
use crate::widgets::rows::{Col, fact, heading, table_row, table_text};
use crate::widgets::sheet::sheet;
use crate::widgets::tabs::Tabs;
use crate::widgets::text_field::TextField;
use crate::widgets::{focus_ring, hover_t, press_t};

/// Selectable mono text in `color`, wrapped to the width at hand.
pub(super) fn mono(ui: &mut Ui, text: impl Into<String>, color: Color32) {
    ui.add(
        egui::Label::new(Type::Mono.rich(text.into()).color(color))
            .selectable(true)
            .wrap(),
    );
}

/// A line of quiet text in the body face: an empty tab, a count.
pub(super) fn quiet(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    Type::Body.paragraph(ui, text, p.t2);
}

fn copy(env: &Env<'_>, state: &mut UiState, text: String) {
    env.ctx.copy_text(text);
    state.toast(
        crate::strings::feedback::COPIED(),
        ToastKind::Success,
        None,
        env.mono,
    );
}

/// A table of technical values: a header row, then one table row per entry whose cells are
/// selectable mono text (the first primary, the rest secondary), cut short at the column's
/// end (the whole value shows under the pointer). Rows out of view only take their place.
pub(super) fn table(ui: &mut Ui, headers: &[&str], cols: &[Col], rows: &[Vec<String>]) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        table_row(ui, cols, true, |ui, i| {
            if let Some(h) = headers.get(i) {
                table_text(ui, h, i, true, false);
            }
        });
        let w = ui.available_width();
        for r in rows {
            let rect = Rect::from_min_size(ui.cursor().min, vec2(w, layout::TABLE_ROW));
            if !ui.is_rect_visible(rect) {
                ui.allocate_space(rect.size());
                continue;
            }
            table_row(ui, cols, false, |ui, i| {
                let Some(text) = r.get(i) else { return };
                let color = if i == 0 { p.t1 } else { p.t2 };
                ui.add(
                    egui::Label::new(Type::Mono.rich(text.as_str()).color(color))
                        .selectable(true)
                        .truncate(),
                );
            });
        }
    });
}

/// A row that opens and closes what is under it (an access statement): the door's plate
/// and press, its text in primary text, a caret at the right edge (right while closed, down
/// while open).
fn disclosure(ui: &mut Ui, text: &str, open: bool) -> egui::Response {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let w = ui.available_width();
    let pad = layout::DOOR_PAD;
    let arrow = icon_size::SMALL;
    let g = Type::Body.galley(
        ui,
        text,
        p.t1,
        (w - layout::FACT_GAP - arrow).max(1.0),
        usize::MAX,
    );
    let h = g.size().y.max(Type::Body.line_height()) + pad * 2.0;
    let plate = ui.painter().add(egui::Shape::Noop);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    let hover = hover_t(&resp, &theme);
    let pressed = press_t(&resp, &theme);
    let over = rect.expand2(vec2(layout::ROW_OVERHANG, 0.0));
    let corner = CornerRadius::same(radius::ROW);
    let fill = mix(mix(Color32::TRANSPARENT, p.o1, hover), p.o2, pressed);
    ui.painter()
        .set(plate, egui::Shape::rect_filled(over, corner, fill));
    ui.painter()
        .galley(pos2(rect.left(), rect.top() + pad), g, p.t1);
    let glyph = if open {
        icons::CARET_DOWN
    } else {
        icons::CARET_RIGHT
    };
    ui.painter().text(
        pos2(
            rect.right() - arrow / 2.0,
            rect.top() + pad + Type::Body.line_height() / 2.0,
        ),
        Align2::CENTER_CENTER,
        glyph.text(false),
        icons::font(icon_size::SMALL),
        mix(p.t3, p.t1, hover),
    );
    a11y::button(&resp, text);
    ui.ctx()
        .accesskit_node_builder(resp.id, |n| n.set_expanded(open));
    if resp.has_focus() {
        focus_ring(ui.painter(), over, corner, &theme);
    }
    resp
}

/// Quiet text buttons on one line (the copy actions); returns the index clicked.
fn actions(ui: &mut Ui, labels: &[&str]) -> Option<usize> {
    let mut out = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, l) in labels.iter().enumerate() {
            if ui.add(Button::ghost(l)).clicked() {
                out = Some(i);
            }
        }
    });
    out
}

/// Show the sheet (overlay `Inspector`).
pub fn show(ctx: &Context, id: Id, state: &mut UiState, env: &mut Env<'_>) {
    let Some(mut vm) = state.debug.inspector.take() else {
        state.nav.close(Overlay::Inspector);
        return;
    };
    let title = match vm.target {
        crate::bridge::InspectTarget::NextLaunch { .. } => s::INSPECTOR_NEXT,
        crate::bridge::InspectTarget::Session(_) => s::INSPECTOR,
    };
    let can_stop = vm.tab == InspectorTab::Processes && vm.procs.session.is_some();
    let r = sheet(
        ctx,
        id,
        title(),
        false,
        |ui| body(ui, state, env, &mut vm),
        |ui| can_stop && ui.add(Button::new(s::STOP_SESSION())).clicked(),
    );
    if r.footer
        && vm.procs.session.is_some()
        && let Some(g) = vm.game
    {
        env.bridge.send(UiCommand::Stop {
            id: g,
            force: false,
        });
    }
    if r.close_requested {
        state.nav.close(Overlay::Inspector);
    } else {
        state.debug.inspector = Some(vm);
    }
}

fn body(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, vm: &mut InspectorVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut sel = InspectorTab::ALL
        .iter()
        .position(|t| *t == vm.tab)
        .unwrap_or(0);
    if Tabs::new(s::INSPECTOR_TAB_LABEL(), &s::INSPECTOR_TABS())
        .gap(layout::TAB_GAP[1])
        .show(ui, &mut sel)
        .changed()
    {
        vm.tab = InspectorTab::ALL[sel];
    }
    ui.add_space(game::PANEL_TOP);
    let data = match &vm.data {
        AsyncValue::Ready(d) => d.clone(),
        AsyncValue::Failed(e) => {
            ui.label(Type::BodyStrong.rich(&e.title).color(p.t1));
            quiet(ui, &e.message);
            return;
        }
        _ => {
            ui.horizontal(|ui| {
                spinner_widget(ui, p.t2);
                quiet(ui, s::PLANNING());
            });
            return;
        }
    };
    match vm.tab {
        InspectorTab::Summary => summary(ui, &data),
        InspectorTab::Access => access(ui, &data, vm),
        InspectorTab::Command => command(ui, state, env, &data),
        InspectorTab::Sandbox => sandbox(ui, &data, vm),
        InspectorTab::Environment => environment(ui, state, env, &data),
        InspectorTab::Processes => {
            if vm.procs.session.is_some() {
                super::processes::show(ui, env, &mut vm.procs);
            } else {
                quiet(ui, s::PROCESSES_PAST());
            }
        }
        InspectorTab::Events => events(ui, &data),
        InspectorTab::Raw => raw(ui, state, env, &data),
    }
}

fn env_value(data: &InspectorData, name: &str) -> Option<String> {
    inspect::env_rows(data)
        .into_iter()
        .find(|r| r.name == name)
        .map(|r| r.value)
}

/// A fact whose value is technical: selectable mono text.
fn tech(ui: &mut Ui, label: &str, value: impl Into<String>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let value = value.into();
    fact(ui, label, |ui| mono(ui, value, p.t1));
}

fn summary(ui: &mut Ui, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if data.source == InspectorSource::DryRun {
        fine_print(ui, s::DRY_RUN());
        ui.add_space(dbg::CONTROLS_GAP);
    }
    summary_facts(ui, data);
    let marks: Vec<Vec<String>> = data
        .timeline
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "state" | "phase"))
        .map(|e| vec![s::offset_ms(e.t_ms), e.detail.clone()])
        .collect();
    if !marks.is_empty() {
        heading(ui, s::K_TIMELINE(), false);
        table(
            ui,
            &[s::EVENT_COLUMNS()[0], s::EVENT_COLUMNS()[2]],
            &[Col::Fixed(dbg::OFFSET_COL), Col::Flex(1.0)],
            &marks,
        );
    }
    if !data.notes.is_empty() {
        heading(ui, s::NOTES(), false);
        for n in &data.notes {
            Type::Body.paragraph(ui, n, p.t2);
        }
    }
}

fn summary_facts(ui: &mut Ui, data: &InspectorData) {
    let plan = &data.plan;
    let c = &plan.context;
    let dash = || "—".to_owned();
    tech(ui, s::K_GAME(), plan.session.game_name.clone());
    tech(ui, s::K_KIND(), plan.session.kind.as_str());
    tech(
        ui,
        s::K_PRESET(),
        c.preset
            .map(|x| format!("{x:?}").to_lowercase())
            .unwrap_or_else(dash),
    );
    tech(
        ui,
        s::K_NETWORK(),
        c.network
            .map(|x| format!("{x:?}").to_lowercase())
            .unwrap_or_else(dash),
    );
    tech(
        ui,
        s::K_LAYER(),
        format!(
            "{} ({})",
            c.runtime_name
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(dash),
            c.slr_variant
                .map(|v| v.as_str().to_owned())
                .unwrap_or_else(dash)
        ),
    );
    tech(
        ui,
        s::K_ISOLATION(),
        c.isolation
            .map(|i| i.as_str().to_owned())
            .unwrap_or_else(dash),
    );
    tech(
        ui,
        s::K_PREFIX(),
        env_value(data, "WINEPREFIX")
            .or_else(|| env_value(data, "STEAM_COMPAT_DATA_PATH"))
            .unwrap_or_else(dash),
    );
    tech(
        ui,
        s::K_HOME(),
        env_value(data, "HOME").unwrap_or_else(dash),
    );
    let chdir = plan
        .sandbox
        .args
        .iter()
        .position(|a| a == "--chdir")
        .and_then(|i| plan.sandbox.args.get(i + 1))
        .map(|a| a.to_string_lossy().into_owned());
    tech(ui, s::K_WORKDIR(), chdir.unwrap_or_else(dash));
    tech(ui, s::K_SESSION(), plan.session.id.to_string());
    if let Some(sess) = &data.session {
        let end = match (sess.end_reason, sess.exit_status) {
            (Some(r), Some(code)) => s::end_status(r.as_str(), code),
            (Some(r), None) => r.as_str().to_owned(),
            (None, _) => s::RUNNING().to_owned(),
        };
        tech(ui, s::K_END(), end);
    }
}

fn access(ui: &mut Ui, data: &InspectorData, vm: &mut InspectorVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if data.explain.is_empty() {
        quiet(ui, s::NO_STATEMENTS());
        return;
    }
    for (i, st) in data.explain.iter().enumerate() {
        let open = vm.expanded.contains(&i);
        if disclosure(ui, &st.text, open).clicked() {
            if open {
                vm.expanded.retain(|x| *x != i);
            } else {
                vm.expanded.push(i);
            }
        }
        if !open {
            continue;
        }
        let mut lines: Vec<String> = Vec::new();
        if let Some(sb) = &data.sandbox {
            lines.extend(st.mounts.iter().filter_map(|k| sb.mounts.get(*k)).map(|m| {
                format!(
                    "{} {} → {}  ({})",
                    m.kind.label(),
                    m.kind
                        .src()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    m.kind.dst().display(),
                    m.origin.reason
                )
            }));
        }
        lines.extend(st.flags.iter().cloned());
        lines.extend(st.env.iter().map(|e| format!("{e}=…")));
        ui.horizontal_top(|ui| {
            ui.add_space(dbg::DETAIL_INDENT);
            ui.vertical(|ui| {
                for l in lines {
                    mono(ui, l, p.t2);
                }
            });
        });
        ui.add_space(dbg::CONTROLS_GAP);
    }
}

fn command(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let umu = inspect::umu_command(&data.plan);
    heading(ui, s::UMU_COMMAND(), true);
    let rows: Vec<Vec<String>> = umu
        .iter()
        .enumerate()
        .map(|(i, a)| vec![i.to_string(), a.clone()])
        .collect();
    table(
        ui,
        &s::ARG_COLUMNS(),
        &[Col::Fixed(dbg::INDEX_COL), Col::Flex(1.0)],
        &rows,
    );
    heading(ui, s::FULL_COMMAND(), false);
    let full: Vec<String> = data
        .plan
        .sandbox
        .command
        .iter()
        .map(|a| zaun_core::launch::shell::quote(a))
        .collect();
    mono(ui, full.join(" "), p.t1);
    ui.add_space(dbg::CONTROLS_GAP);
    match actions(ui, &[s::COPY_LIST(), s::COPY_SHELL()]) {
        Some(0) => copy(env, state, umu.join("\n")),
        Some(_) => {
            let script = zaun_core::launch::render_shell(&data.plan, &env.now.to_rfc3339());
            copy(env, state, script);
        }
        None => {}
    }
    fine_print(ui, s::COPY_SHELL_NOTE());
}

fn sandbox(ui: &mut Ui, data: &InspectorData, vm: &mut InspectorVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, s::NAMESPACES(), true);
    let args = &data.plan.sandbox.args;
    let flags: Vec<String> = args
        .iter()
        .filter_map(|a| a.to_str())
        .filter(|a| {
            a.starts_with("--unshare")
                || matches!(*a, "--die-with-parent" | "--new-session" | "--clearenv")
        })
        .map(|a| a.trim_start_matches("--").to_owned())
        .collect();
    mono(ui, flags.join(", "), p.t1);
    let seccomp = args.iter().any(|a| a == "--seccomp");
    quiet(
        ui,
        if seccomp {
            s::SECCOMP_ON()
        } else {
            s::SECCOMP_OFF()
        },
    );
    heading(ui, s::MOUNTS(), false);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = dbg::CONTROLS_GAP;
        TextField::new(&mut vm.filter, "insp.filter")
            .search()
            .hint(s::FILTER())
            .accessible_name(s::FILTER())
            .width(dbg::FILTER_W)
            .show(ui);
        checkbox(ui, &mut vm.only_writable, s::ONLY_WRITABLE());
        checkbox(ui, &mut vm.only_choices, s::ONLY_CHOICES());
    });
    ui.add_space(dbg::CONTROLS_GAP);
    let rows: Vec<Vec<String>> = vm
        .mounts(data)
        .into_iter()
        .map(|m: MountRow| {
            vec![
                m.index.to_string(),
                m.kind,
                m.source,
                m.destination,
                m.mode.to_owned(),
                m.section.unwrap_or_default(),
                m.reason.unwrap_or_default(),
            ]
        })
        .collect();
    table(
        ui,
        &s::MOUNT_COLUMNS(),
        &[
            Col::Fixed(dbg::INDEX_COL),
            Col::Fixed(dbg::KIND_COL),
            Col::Flex(1.0),
            Col::Flex(1.0),
            Col::Fixed(dbg::MODE_COL),
            Col::Fixed(dbg::KIND_COL),
            Col::Flex(1.0),
        ],
        &rows,
    );
    heading(ui, s::DESCRIPTORS(), false);
    let fds: Vec<Vec<String>> = data
        .plan
        .sandbox
        .fds
        .iter()
        .map(|f| {
            vec![
                f.fd.to_string(),
                format!("{:?}", f.kind).to_lowercase(),
                f.target
                    .as_ref()
                    .map(|t| t.display().to_string())
                    .unwrap_or_default(),
            ]
        })
        .collect();
    table(
        ui,
        &s::FD_COLUMNS(),
        &[
            Col::Fixed(dbg::INDEX_COL),
            Col::Fixed(dbg::KIND_COL),
            Col::Flex(1.0),
        ],
        &fds,
    );
}

fn environment(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    let rows = inspect::env_rows(data);
    if actions(ui, &[s::COPY_ALL()]).is_some() {
        let all: Vec<String> = rows
            .iter()
            .map(|r| format!("{}={}", r.name, r.value))
            .collect();
        copy(env, state, all.join("\n"));
    }
    ui.add_space(dbg::CONTROLS_GAP);
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| vec![r.name.clone(), r.value.clone(), r.origin.clone()])
        .collect();
    table(
        ui,
        &s::ENV_COLUMNS(),
        &[Col::Flex(1.0), Col::Flex(1.5), Col::Fixed(dbg::ORIGIN_COL)],
        &cells,
    );
    ui.add_space(dbg::CONTROLS_GAP);
    fine_print(ui, s::UMU_ADDS());
}

fn events(ui: &mut Ui, data: &InspectorData) {
    if data.timeline.is_empty() {
        quiet(ui, s::NO_EVENTS());
        return;
    }
    let rows: Vec<Vec<String>> = data
        .timeline
        .iter()
        .map(|e| vec![s::offset_ms(e.t_ms), e.kind.clone(), e.detail.clone()])
        .collect();
    table(
        ui,
        &s::EVENT_COLUMNS(),
        &[
            Col::Fixed(dbg::OFFSET_COL),
            Col::Fixed(dbg::KIND_COL),
            Col::Flex(1.0),
        ],
        &rows,
    );
}

fn raw(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    match actions(ui, &[s::COPY_NUL(), s::COPY_JSON()]) {
        Some(0) => {
            let mut t = String::new();
            for a in &data.plan.sandbox.args {
                t.push_str(&a.to_string_lossy());
                t.push('\0');
            }
            copy(env, state, t);
        }
        Some(_) => {
            let json = zaun_core::launch::plan_file::to_json(&data.plan)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            copy(env, state, json);
        }
        None => {}
    }
    heading(ui, s::RAW_ARGS(), false);
    let rows: Vec<Vec<String>> = inspect::raw_args(&data.plan)
        .into_iter()
        .map(|(i, a)| vec![i.to_string(), a])
        .collect();
    table(
        ui,
        &s::ARG_COLUMNS(),
        &[Col::Fixed(dbg::INDEX_COL), Col::Flex(1.0)],
        &rows,
    );
}

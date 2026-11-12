//! The launch inspector sheet (DEVELOPER_MODE §3, DEV-002): Summary, Access, Command,
//! Sandbox, Environment, Processes, Events, Raw. Tables are virtualized
//! (`egui_extras::TableBuilder`), cells are selectable `mono` text. Every value shown comes
//! from the structures the launcher executes (`InspectorData`).

use easyplay_core::launch::inspect::{self, MountRow};
use easyplay_core::launch::{InspectorData, InspectorSource};
use egui::{Context, Id, RichText, Ui};
use egui_extras::{Column, TableBuilder};

use super::super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::route::Overlay;
use crate::strings::debug as s;
use crate::theme::tokens::space;
use crate::theme::{self, Type};
use crate::vm::AsyncValue;
use crate::vm::debug::{InspectorTab, InspectorVm};
use crate::vm::toasts::ToastKind;
use crate::widgets::button::Button;
use crate::widgets::sheet::sheet;
use crate::widgets::tabs::tabs;

/// Row height of the tables.
const ROW_H: f32 = 22.0;

fn mono(ui: &mut Ui, text: impl Into<String>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add(egui::Label::new(Type::Mono.rich(text).color(p.text_primary)).selectable(true));
}

/// One summary row inside the summary grid.
fn key_value(ui: &mut Ui, k: &str, v: impl Into<String>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.label(Type::Caption.rich(k).color(p.text_secondary));
    mono(ui, v);
    ui.end_row();
}

fn heading(ui: &mut Ui, text: &str) {
    let p = theme::theme(ui.ctx()).palette.clone();
    ui.add_space(space::S3);
    ui.label(Type::Heading.rich(text).color(p.text_primary));
    ui.add_space(space::S1);
}

fn copy(env: &Env<'_>, state: &mut UiState, text: String) {
    env.ctx.copy_text(text);
    state.toast(
        crate::strings::feedback::COPIED,
        ToastKind::Success,
        None,
        env.mono,
    );
}

/// A virtualized table with headers and `cells(row) -> Vec<String>`.
fn table(ui: &mut Ui, salt: &str, headers: &[&str], widths: &[f32], rows: &[Vec<String>]) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let mut b = TableBuilder::new(ui)
        .id_salt(salt)
        .striped(true)
        .resizable(true)
        .max_scroll_height(360.0)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    for (i, w) in widths.iter().enumerate() {
        b = if i + 1 == widths.len() {
            b.column(Column::remainder().at_least(*w).clip(true))
        } else {
            b.column(Column::initial(*w).at_least(24.0).clip(true))
        };
    }
    b.header(ROW_H, |mut h| {
        for t in headers {
            h.col(|ui| {
                ui.label(Type::Micro.rich(*t).color(p.text_secondary));
            });
        }
    })
    .body(|body| {
        body.rows(ROW_H, rows.len(), |mut row| {
            let r = &rows[row.index()];
            for c in r {
                row.col(|ui| {
                    ui.add(
                        egui::Label::new(Type::Mono.rich(c.as_str()).color(p.text_primary))
                            .selectable(true)
                            .truncate(),
                    );
                });
            }
        });
    });
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
        title,
        false,
        |ui| body(ui, state, env, &mut vm),
        |ui| can_stop && ui.add(Button::new(s::STOP_SESSION)).clicked(),
    );
    if r.footer
        && let Some(sid) = vm.procs.session
        && let Some(g) = vm.game
    {
        let _ = sid;
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
    if tabs(ui, s::INSPECTOR_TAB_LABEL, &mut sel, &s::INSPECTOR_TABS).changed() {
        vm.tab = InspectorTab::ALL[sel];
    }
    ui.add_space(space::S3);
    let data = match &vm.data {
        AsyncValue::Ready(d) => d.clone(),
        AsyncValue::Failed(e) => {
            ui.label(Type::BodyStrong.rich(&e.title).color(p.text_primary));
            ui.label(Type::Body.rich(&e.message).color(p.text_secondary));
            return;
        }
        _ => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(Type::Body.rich(s::PLANNING).color(p.text_secondary));
            });
            return;
        }
    };
    match vm.tab {
        InspectorTab::Summary => summary(ui, &data, env),
        InspectorTab::Access => access(ui, &data, vm),
        InspectorTab::Command => command(ui, state, env, &data),
        InspectorTab::Sandbox => sandbox(ui, &data, vm),
        InspectorTab::Environment => environment(ui, state, env, &data),
        InspectorTab::Processes => {
            if vm.procs.session.is_some() {
                super::processes::show(ui, env, &mut vm.procs);
            } else {
                ui.label(Type::Body.rich(s::PROCESSES_PAST).color(p.text_secondary));
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

fn summary(ui: &mut Ui, data: &InspectorData, env: &Env<'_>) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if data.source == InspectorSource::DryRun {
        ui.label(Type::Caption.rich(s::DRY_RUN).color(p.text_secondary));
        ui.add_space(space::S2);
    }
    let dash = || "—".to_owned();
    egui::Grid::new("insp.summary")
        .num_columns(2)
        .spacing([space::S4, space::S1])
        .show(ui, |ui| summary_rows(ui, data));
    if !data.timeline.is_empty() {
        heading(ui, s::K_TIMELINE);
        timeline_bar(ui, data);
    }
    if !data.notes.is_empty() {
        heading(ui, s::NOTES);
        for n in &data.notes {
            ui.label(Type::Body.rich(format!("• {n}")).color(p.text_secondary));
        }
    }
    let _ = (env, dash);
}

fn summary_rows(ui: &mut Ui, data: &InspectorData) {
    let plan = &data.plan;
    let c = &plan.context;
    let dash = || "—".to_owned();
    key_value(ui, s::K_GAME, plan.session.game_name.clone());
    key_value(ui, s::K_KIND, plan.session.kind.as_str());
    key_value(
        ui,
        s::K_PRESET,
        c.preset
            .map(|x| format!("{x:?}").to_lowercase())
            .unwrap_or_else(dash),
    );
    key_value(
        ui,
        s::K_NETWORK,
        c.network
            .map(|x| format!("{x:?}").to_lowercase())
            .unwrap_or_else(dash),
    );
    key_value(
        ui,
        s::K_LAYER,
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
    key_value(
        ui,
        s::K_ISOLATION,
        c.isolation
            .map(|i| i.as_str().to_owned())
            .unwrap_or_else(dash),
    );
    key_value(
        ui,
        s::K_PREFIX,
        env_value(data, "WINEPREFIX")
            .or_else(|| env_value(data, "STEAM_COMPAT_DATA_PATH"))
            .unwrap_or_else(dash),
    );
    key_value(ui, s::K_HOME, env_value(data, "HOME").unwrap_or_else(dash));
    let chdir = plan
        .sandbox
        .args
        .iter()
        .position(|a| a == "--chdir")
        .and_then(|i| plan.sandbox.args.get(i + 1))
        .map(|a| a.to_string_lossy().into_owned());
    key_value(ui, s::K_WORKDIR, chdir.unwrap_or_else(dash));
    key_value(ui, s::K_SESSION, plan.session.id.to_string());
    if let Some(sess) = &data.session {
        let end = match (sess.end_reason, sess.exit_status) {
            (Some(r), Some(code)) => s::end_status(r.as_str(), code),
            (Some(r), None) => r.as_str().to_owned(),
            (None, _) => s::RUNNING.to_owned(),
        };
        key_value(ui, s::K_END, end);
    }
}

/// The launch timeline as a segmented bar (state changes, LOGGING §5).
fn timeline_bar(ui: &mut Ui, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let marks: Vec<(u64, String)> = data
        .timeline
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "state" | "phase"))
        .map(|e| (e.t_ms, e.detail.clone()))
        .collect();
    let total = marks.last().map_or(1, |m| m.0.max(1));
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), egui::Sense::hover());
    let colors = [p.accent, p.info, p.success, p.warning];
    for (i, w) in marks.windows(2).enumerate() {
        let x0 = rect.left() + rect.width() * (w[0].0 as f32 / total as f32);
        let x1 = rect.left() + rect.width() * (w[1].0 as f32 / total as f32);
        ui.painter().rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(x0, rect.top()),
                egui::pos2(x1.max(x0 + 1.0), rect.bottom()),
            ),
            2.0,
            colors[i % colors.len()],
        );
    }
    for (t, d) in &marks {
        ui.label(
            Type::Mono
                .rich(format!("{}  {d}", s::offset_ms(*t)))
                .color(p.text_secondary),
        );
    }
}

fn access(ui: &mut Ui, data: &InspectorData, vm: &mut InspectorVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if data.explain.is_empty() {
        ui.label(Type::Body.rich(s::NO_STATEMENTS).color(p.text_secondary));
        return;
    }
    for (i, st) in data.explain.iter().enumerate() {
        let open = vm.expanded.contains(&i);
        let r = egui::CollapsingHeader::new(Type::Body.rich(&st.text).color(p.text_primary))
            .id_salt(("access", i))
            .open(Some(open))
            .show(ui, |ui| {
                if let Some(sb) = &data.sandbox {
                    for m in st.mounts.iter().filter_map(|k| sb.mounts.get(*k)) {
                        mono(
                            ui,
                            format!(
                                "{} {} → {}  ({})",
                                m.kind.label(),
                                m.kind
                                    .src()
                                    .map(|p| p.display().to_string())
                                    .unwrap_or_default(),
                                m.kind.dst().display(),
                                m.origin.reason
                            ),
                        );
                    }
                }
                for f in &st.flags {
                    mono(ui, f.clone());
                }
                for e in &st.env {
                    mono(ui, format!("{e}=…"));
                }
            });
        if r.header_response.clicked() {
            if open {
                vm.expanded.retain(|x| *x != i);
            } else {
                vm.expanded.push(i);
            }
        }
    }
}

fn command(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let umu = inspect::umu_command(&data.plan);
    heading(ui, s::UMU_COMMAND);
    let rows: Vec<Vec<String>> = umu
        .iter()
        .enumerate()
        .map(|(i, a)| vec![i.to_string(), a.clone()])
        .collect();
    table(ui, "insp.umu", &["#", "Argument"], &[32.0, 300.0], &rows);
    heading(ui, s::FULL_COMMAND);
    let full: Vec<String> = data
        .plan
        .sandbox
        .command
        .iter()
        .map(|a| easyplay_core::launch::shell::quote(a))
        .collect();
    mono(ui, full.join(" "));
    ui.add_space(space::S3);
    ui.horizontal(|ui| {
        if ui.add(Button::new(s::COPY_LIST)).clicked() {
            copy(env, state, umu.join("\n"));
        }
        if ui.add(Button::new(s::COPY_SHELL)).clicked() {
            let script = easyplay_core::launch::render_shell(&data.plan, &env.now.to_rfc3339());
            copy(env, state, script);
        }
    });
    ui.label(
        Type::Caption
            .rich(s::COPY_SHELL_NOTE)
            .color(p.text_secondary),
    );
}

fn sandbox(ui: &mut Ui, data: &InspectorData, vm: &mut InspectorVm) {
    let p = theme::theme(ui.ctx()).palette.clone();
    heading(ui, s::NAMESPACES);
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
    ui.horizontal_wrapped(|ui| {
        for f in &flags {
            crate::widgets::chip::chip(ui, crate::widgets::chip::ChipKind::Status, f, None, true);
        }
    });
    let seccomp = args.iter().any(|a| a == "--seccomp");
    ui.label(
        Type::Caption
            .rich(if seccomp {
                s::SECCOMP_ON
            } else {
                s::SECCOMP_OFF
            })
            .color(p.text_secondary),
    );
    heading(ui, s::MOUNTS);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut vm.filter)
                .hint_text(s::FILTER)
                .desired_width(220.0),
        );
        ui.checkbox(&mut vm.only_writable, s::ONLY_WRITABLE);
        ui.checkbox(&mut vm.only_choices, s::ONLY_CHOICES);
    });
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
        "insp.mounts",
        &s::MOUNT_COLUMNS,
        &[32.0, 64.0, 150.0, 150.0, 36.0, 70.0, 120.0],
        &rows,
    );
    heading(ui, s::DESCRIPTORS);
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
        "insp.fds",
        &["#", "Kind", "Purpose"],
        &[32.0, 80.0, 200.0],
        &fds,
    );
}

fn environment(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let rows = inspect::env_rows(data);
    if ui.add(Button::new(s::COPY_ALL)).clicked() {
        let all: Vec<String> = rows
            .iter()
            .map(|r| format!("{}={}", r.name, r.value))
            .collect();
        copy(env, state, all.join("\n"));
    }
    let cells: Vec<Vec<String>> = rows
        .iter()
        .map(|r| vec![r.name.clone(), r.value.clone(), r.origin.clone()])
        .collect();
    table(
        ui,
        "insp.env",
        &s::ENV_COLUMNS,
        &[180.0, 260.0, 90.0],
        &cells,
    );
    ui.add_space(space::S2);
    ui.label(Type::Caption.rich(s::UMU_ADDS).color(p.text_secondary));
}

fn events(ui: &mut Ui, data: &InspectorData) {
    let p = theme::theme(ui.ctx()).palette.clone();
    if data.timeline.is_empty() {
        ui.label(Type::Body.rich(s::NO_EVENTS).color(p.text_secondary));
        return;
    }
    let rows: Vec<Vec<String>> = data
        .timeline
        .iter()
        .map(|e| vec![s::offset_ms(e.t_ms), e.kind.clone(), e.detail.clone()])
        .collect();
    table(
        ui,
        "insp.events",
        &s::EVENT_COLUMNS,
        &[90.0, 70.0, 300.0],
        &rows,
    );
}

fn raw(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>, data: &InspectorData) {
    heading(ui, s::RAW_ARGS);
    let rows: Vec<Vec<String>> = inspect::raw_args(&data.plan)
        .into_iter()
        .map(|(i, a)| vec![i.to_string(), a])
        .collect();
    table(ui, "insp.raw", &["#", "Argument"], &[40.0, 400.0], &rows);
    ui.horizontal(|ui| {
        if ui.add(Button::new(s::COPY_NUL)).clicked() {
            let mut t = String::new();
            for a in &data.plan.sandbox.args {
                t.push_str(&a.to_string_lossy());
                t.push('\0');
            }
            copy(env, state, t);
        }
        if ui.add(Button::new(s::COPY_JSON)).clicked() {
            let json = easyplay_core::launch::plan_file::to_json(&data.plan)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            copy(env, state, json);
        }
    });
    let _ = RichText::new("");
}

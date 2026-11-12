//! The process tree (DEVELOPER_MODE §4, DEV-004): host PID, name, role guess, state, CPU
//! time and resident memory, with totals. It asks for a fresh list once a second **only
//! while it is drawn**: hiding the view (another tab, the sheet closed) stops the refreshes.

use egui::Ui;
use egui_extras::{Column, TableBuilder};

use super::super::Env;
use crate::bridge::UiCommand;
use crate::strings::debug as s;
use crate::theme::{self, Type};
use crate::vm::debug::{PROCESS_REFRESH, ProcessTreeVm, role_name};

/// Ask for a refresh when due (call only while the view is visible).
pub fn tick(vm: &mut ProcessTreeVm, env: &mut Env<'_>) {
    if let Some(session) = vm.session
        && vm.due(env.mono)
    {
        let q = env.bridge.query_id();
        vm.requested(q, env.mono);
        env.bridge.send(UiCommand::ListProcesses {
            session,
            reply_to: q,
        });
    }
    env.ctx.request_repaint_after(PROCESS_REFRESH);
}

/// Draw the tree.
pub fn show(ui: &mut Ui, env: &mut Env<'_>, vm: &mut ProcessTreeVm) {
    tick(vm, env);
    let p = theme::theme(ui.ctx()).palette.clone();
    let (n, cpu, rss) = vm.totals();
    ui.label(
        Type::Caption
            .rich(s::process_totals(n, cpu, rss))
            .color(p.text_secondary),
    );
    let rows = vm.tree();
    TableBuilder::new(ui)
        .id_salt("processes")
        .striped(true)
        .max_scroll_height(360.0)
        .column(Column::initial(64.0))
        .column(Column::initial(200.0).clip(true))
        .column(Column::initial(90.0))
        .column(Column::initial(70.0))
        .column(Column::initial(80.0))
        .column(Column::remainder().at_least(70.0))
        .header(20.0, |mut h| {
            for t in s::PROCESS_COLUMNS {
                h.col(|ui| {
                    ui.label(Type::Micro.rich(t).color(p.text_secondary));
                });
            }
        })
        .body(|body| {
            body.rows(22.0, rows.len(), |mut row| {
                let r = &rows[row.index()];
                let cells = [
                    r.info.pid.to_string(),
                    format!("{}{}", "  ".repeat(r.depth), r.info.comm),
                    role_name(r.info.role).to_owned(),
                    format!("{:?}", r.info.state).to_lowercase(),
                    r.info
                        .usage
                        .map(|u| s::cpu_time(u.cpu_ms))
                        .unwrap_or_default(),
                    r.info
                        .usage
                        .map(|u| s::bytes(u.rss_bytes))
                        .unwrap_or_default(),
                ];
                for c in cells {
                    row.col(|ui| {
                        ui.add(
                            egui::Label::new(Type::Mono.rich(c).color(p.text_primary))
                                .selectable(true)
                                .truncate(),
                        );
                    });
                }
            });
        });
}

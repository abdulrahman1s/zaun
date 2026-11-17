//! The process tree (DEVELOPER_MODE §4, DEV-004): host PID, name, role guess, state, CPU
//! time and resident memory, with totals. It asks for a fresh list once a second **only
//! while it is drawn**: hiding the view (another tab, the sheet closed) stops the refreshes.
//!
//! Built from the design's parts: the totals as one quiet line, then table rows of mono
//! cells, a child process standing in under its parent.

use egui::Ui;

use super::super::Env;
use super::inspector::{quiet, table};
use crate::bridge::UiCommand;
use crate::strings::debug as s;
use crate::theme::tokens::debug as dbg;
use crate::vm::debug::{PROCESS_REFRESH, ProcessTreeVm, role_name};
use crate::widgets::rows::Col;

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
    let (n, cpu, rss) = vm.totals();
    quiet(ui, &s::process_totals(n, cpu, rss));
    ui.add_space(dbg::CONTROLS_GAP);
    let rows: Vec<Vec<String>> = vm
        .tree()
        .into_iter()
        .map(|r| {
            vec![
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
            ]
        })
        .collect();
    let [pid, role, state, cpu_w, mem] = dbg::PROCESS_COLS;
    table(
        ui,
        &s::PROCESS_COLUMNS(),
        &[
            Col::Fixed(pid),
            Col::Flex(1.0),
            Col::Fixed(role),
            Col::Fixed(state),
            Col::Fixed(cpu_w),
            Col::Fixed(mem),
        ],
        &rows,
    );
}

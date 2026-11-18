//! Settings › About › Updates (ADR-0040): the switch, the version with the last check and
//! "Check now", and a newer version with its notes and — for an install Zaun owns —
//! "Download", then "Restart now". Other installs are told how to update instead.

use egui::Ui;
use zaun_core::model::{Activity, InstallChannel, UpdateDownload, UpdateSnapshot};
use zaun_core::service::UpdateCommand;

use super::{Env, band, button_row, quiet, switch_row};
use crate::app::UiState;
use crate::bridge::Command;
use crate::strings;
use crate::strings::settings as s;
use crate::strings::updates as t;
use crate::theme::Type;
use crate::theme::tokens::control;
use crate::widgets::button::{Button, Variant};
use crate::widgets::rows::{self, SettingRow};

pub(super) fn group(ui: &mut Ui, state: &mut UiState, env: &mut Env<'_>) {
    let u = env.bridge.updates().clone();
    rows::heading(ui, t::GROUP(), false);
    if !u.configured {
        quiet(ui, t::NOT_CONFIGURED(), Type::Caption);
    }
    switch_row(
        ui,
        state,
        env,
        "updates.check",
        t::CHECK(),
        t::CHECK_HELP(),
        u.configured,
        |p| p.updates.check,
        |p, v| p.updates.check = v,
    );
    let on = env.bridge.prefs().updates.check;
    let status = status(&u, on, env);
    if button_row(
        ui,
        &s::version(&u.current),
        &status,
        u.configured && on && !u.checking,
        Button::new(t::CHECK_NOW()).variant(Variant::Secondary),
    ) {
        env.bridge.send(Command::Updates(UpdateCommand::Check));
    }
    if let Some(v) = &u.rolled_back {
        quiet(ui, &t::rolled_back(v), Type::Caption);
    }
    offer(ui, &u, env);
}

/// The version row's help: checking, never checked, up to date, or why the check failed.
fn status(u: &UpdateSnapshot, on: bool, env: &Env<'_>) -> String {
    if u.checking {
        return t::CHECKING().to_owned();
    }
    if let Some(code) = u.failure {
        return t::failed(zaun_core::error::catalog::entry(code).title);
    }
    match (u.last_checked, on) {
        (_, false) => t::CHECKS_OFF().to_owned(),
        (None, true) => t::NEVER_CHECKED().to_owned(),
        (Some(at), true) => t::up_to_date(&strings::fmt::relative_inline(
            at,
            env.now,
            env.utc_offset_s,
        )),
    }
}

/// A newer version: its notes and what to do about it.
fn offer(ui: &mut Ui, u: &UpdateSnapshot, env: &mut Env<'_>) {
    let staged = match &u.download {
        UpdateDownload::Staged { version } => Some(version.clone()),
        UpdateDownload::Idle | UpdateDownload::Downloading => None,
    };
    let Some(version) = staged
        .clone()
        .or_else(|| u.available.as_ref().map(|a| a.version.clone()))
    else {
        return;
    };
    let busy = env
        .bridge
        .library()
        .games
        .iter()
        .any(|g| !matches!(g.activity, Activity::Idle));
    let (label, help) = match (&staged, u.channel) {
        (Some(v), _) => (
            t::ready(v),
            if busy {
                t::READY_WAITS()
            } else {
                t::READY_HELP()
            }
            .to_owned(),
        ),
        (None, InstallChannel::Tarball { owned: true }) => {
            (t::available(&version), t::DOWNLOAD_HELP().to_owned())
        }
        (None, c) => (t::available(&version), t::how_to_update(c).to_owned()),
    };
    let notes = u
        .available
        .as_ref()
        .map(|a| a.notes.as_str())
        .filter(|n| !n.is_empty());
    let help = match notes {
        Some(n) => format!("{n}\n{help}"),
        None => help,
    };
    let has_notes = u.available.as_ref().is_some_and(|a| a.has_notes);
    let (notes_clicked, action) = SettingRow::new(&label).help(&help).show(ui, |ui| {
        band(ui, control::BUTTON, |ui| {
            let notes = has_notes && ui.add(Button::ghost(t::NOTES())).clicked();
            let action = match (&u.download, u.channel.self_updates()) {
                (UpdateDownload::Staged { .. }, _) => ui
                    .add_enabled(!busy, Button::primary(t::RESTART()))
                    .clicked()
                    .then_some(UpdateCommand::Restart),
                (UpdateDownload::Downloading, true) => {
                    ui.add_enabled(false, Button::new(t::DOWNLOADING()));
                    None
                }
                (UpdateDownload::Idle, true) => ui
                    .add(Button::new(t::DOWNLOAD()))
                    .clicked()
                    .then_some(UpdateCommand::Download),
                (UpdateDownload::Idle | UpdateDownload::Downloading, false) => None,
            };
            (notes, action)
        })
    });
    if notes_clicked {
        env.bridge.send(Command::Updates(UpdateCommand::OpenNotes));
    }
    if let Some(c) = action {
        env.bridge.send(Command::Updates(c));
    }
}

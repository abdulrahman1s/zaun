//! Statistics (UI-022, UF-19), settings (UI-023) and onboarding (UI-024, UF-1) tests and
//! snapshots.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::{Rig, now, rig_on, rig_with, settle, settled_library};
use egui::accesskit::Role;
use egui::{Key, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};
use zaun_core::config::schema::Theme;
use zaun_ui::bridge::{Command, FakeBackend, UiCommand, library_from};
use zaun_ui::demo;
use zaun_ui::route::{Route, SettingsTab};
use zaun_ui::theme::ThemeChoice;

fn at(route: Route, size: Vec2, choice: ThemeChoice) -> Rig {
    let mut r = rig_with(settled_library(), route, size, choice);
    settle(&mut r);
    r
}

fn big() -> Vec2 {
    Vec2::new(1280.0, 900.0)
}

// ---- Statistics --------------------------------------------------------------------------

/// The chart's name for a range ("in the last 7 days").
fn chart(range: &str) -> String {
    format!("Playtime, {range}. Show as table for the numbers.")
}

#[test]
fn statistics_show_the_headline_chart_and_lists() {
    let r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.get_by_role_and_label(Role::Heading, "9 h 48 min in the last 7 days");
    r.h.get_by_role_and_label(Role::Figure, &chart("in the last 7 days"));
    r.h.get_all_by_label("Most played").next().unwrap();
    r.h.get_all_by_label("Recent sessions").next().unwrap();
}

/// The headline says the period's time, and one sentence under it the games, the sessions
/// and the streak, for every period.
#[test]
fn the_headline_and_its_sentence_follow_the_period() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.get_by_role_and_label(Role::Heading, "9 h 48 min in the last 7 days");
    r.h.get_by_label_contains("9 games and 12 sessions.");
    for (word, says) in [
        ("30 days", "in the last 30 days"),
        ("Year", "in the last year"),
        ("All time", "in all"),
    ] {
        r.h.get_by_role_and_label(Role::RadioButton, word).click();
        settle(&mut r);
        let heading =
            r.h.query_all_by_role(Role::Heading)
                .filter_map(|n| n.accesskit_node().label())
                .find(|l| l.ends_with(says));
        assert!(heading.is_some(), "a headline {says}");
        r.h.get_by_role_and_label(Role::Figure, &chart(says));
        r.h.get_by_label_contains(" sessions.");
    }
}

#[test]
fn returning_to_statistics_shows_cached_results_in_the_first_frame() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.state_mut().state.nav.go_to(Route::Library);
    r.h.run_steps(10);
    r.fake.clear_sent();
    r.h.state_mut().state.nav.go_to(Route::Statistics);
    // (The page takes 80 ms to change.)
    r.h.run_steps(10);
    r.h.get_by_role_and_label(Role::Heading, "9 h 48 min in the last 7 days");
    assert!(!r.fake.sent().iter().any(|c| matches!(
        c,
        UiCommand::Core(Command::QueryStats {
            request: zaun_core::stats::StatsRequest::LibraryOverview { .. },
            ..
        })
    )));
}

/// A failing overview is asked for once; *Retry* asks again.
#[test]
fn failed_statistics_wait_for_retry() {
    use zaun_core::stats::StatsRequest;
    use zaun_ui::bridge::{QueryPayload, UiEvent};
    let lib = settled_library();
    let game = lib.games[0].id;
    let mut r = rig_with(lib, Route::Statistics, big(), ThemeChoice::Dark);
    // Answer every query with a failure; count the overview requests.
    let fail_all = |r: &mut Rig| -> usize {
        let sent = r.fake.sent();
        r.fake.clear_sent();
        let mut overviews = 0;
        for c in sent {
            let UiCommand::Core(Command::QueryStats { request, reply_to }) = c else {
                continue;
            };
            if matches!(request, StatsRequest::LibraryOverview { .. }) {
                overviews += 1;
            }
            r.fake.push_event(UiEvent::QueryResult {
                id: reply_to,
                payload: QueryPayload::Failed(demo::launch_failure(game, "X")),
            });
        }
        overviews
    };
    let mut asked = 0;
    for _ in 0..6 {
        r.h.run_steps(2);
        asked += fail_all(&mut r);
    }
    assert_eq!(asked, 1);
    r.h.get_by_label("Couldn't load statistics");
    r.h.get_by_role_and_label(Role::Button, "Try again").click();
    r.h.run_steps(2);
    assert_eq!(fail_all(&mut r), 1);
}

#[test]
fn chart_bars_move_with_arrow_keys_and_the_table_toggles() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.get_by_role_and_label(Role::Figure, &chart("in the last 7 days"))
        .focus();
    r.h.run();
    r.h.key_press(Key::End);
    r.h.run();
    assert_eq!(r.h.state().state.stats.bar, Some(6));
    r.h.key_press(Key::ArrowLeft);
    r.h.run();
    assert_eq!(r.h.state().state.stats.bar, Some(5));
    // "Show as table" swaps the plot for the numbers, and back.
    r.h.get_by_label("Show as table").click();
    r.h.run();
    assert!(r.h.state().state.stats.table);
    assert!(
        r.h.query_by_role_and_label(Role::Figure, &chart("in the last 7 days"))
            .is_none()
    );
    let table = r.h.get_by_role(Role::Table);
    assert!(
        table
            .accesskit_node()
            .label()
            .unwrap_or_default()
            .starts_with("Playtime")
    );
    assert!(r.h.query_all_by_role(Role::Row).count() >= 1);
    r.h.get_by_label("Show as chart").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Figure, &chart("in the last 7 days"));
}

#[test]
fn changing_the_range_queries_again() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.fake.clear_sent();
    r.h.get_by_label("30 days").click();
    settle(&mut r);
    assert_eq!(r.h.state().state.stats.range, 1);
    r.h.get_by_role_and_label(Role::Figure, &chart("in the last 30 days"));
}

#[test]
fn statistics_empty_state() {
    let lib = library_from(Vec::new(), Vec::new(), 1, now());
    let mut r = rig_with(lib, Route::Statistics, big(), ThemeChoice::Dark);
    settle(&mut r);
    r.h.get_by_label("Nothing to show yet");
    r.h.snapshot("statistics_empty_dark_1280x900");
}

#[test]
fn snapshot_statistics_dark() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Dark);
    r.h.run_steps(4);
    r.h.snapshot("statistics_dark_1280x900");
}

#[test]
fn snapshot_statistics_light() {
    let mut r = at(Route::Statistics, big(), ThemeChoice::Light);
    r.h.run_steps(4);
    r.h.snapshot("statistics_light_1280x900");
}

#[test]
fn snapshot_statistics_compact() {
    let mut r = at(
        Route::Statistics,
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(4);
    r.h.snapshot("statistics_dark_640x480");
}

// ---- Settings ----------------------------------------------------------------------------

/// Every key of the preference schema has a control (UI-023 acceptance).
#[test]
fn every_preference_has_a_control() {
    // Rows whose Chat feature is off are not drawn (ADR-0033): render with every feature,
    // as the preview does.
    let chat = common::ChatBackend::new(
        FakeBackend::new(settled_library()),
        zaun_core::model::ChatSnapshot {
            features: zaun_core::model::ChatFeatures::all(),
            ..Default::default()
        },
    );
    let mut r = common::rig_chat(
        &chat,
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 3000.0),
        zaun_ui::theme::Appearance::default(),
    );
    settle(&mut r);
    for tab in zaun_ui::screens::settings::TABS {
        r.h.state_mut().state.nav.replace(Route::Settings(tab));
        r.h.run_steps(3);
    }
    let rendered = zaun_ui::screens::settings::rendered_keys(&r.h.ctx);
    let table =
        zaun_core::config::load::to_table(&zaun_core::config::Preferences::default()).unwrap();
    // The list's column widths were set by dragging its headers; the list's columns are
    // fixed now, so nothing sets them and no row offers to reset them. The stored value is
    // still read (and ignored), so older files load.
    let without_control = ["appearance.list_column_widths"];
    let mut missing = Vec::new();
    for (section, v) in table.iter() {
        let Some(t) = v.as_table() else { continue };
        for key in t.keys() {
            let full = format!("{section}.{key}");
            if !rendered.contains(&full) && !without_control.contains(&full.as_str()) {
                missing.push(full);
            }
        }
    }
    assert!(missing.is_empty(), "no control for {missing:?}");
}

#[test]
fn a_toggle_applies_at_once() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 1600.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Ask before removing a game").click();
    r.h.run();
    let sent = r.fake.sent();
    let preview = sent.iter().find_map(|c| match c {
        UiCommand::SetPreference { patch, preview } => Some((
            patch.keys().map(str::to_owned).collect::<Vec<_>>(),
            preview.clone(),
        )),
        _ => None,
    });
    let (keys, preview) = preview.expect("SetPreference sent");
    assert_eq!(keys, vec!["general.confirm_remove".to_owned()]);
    assert_eq!(
        preview.general.confirm_remove,
        !demo::preferences().general.confirm_remove
    );
    // The switch shows the new value at once.
    r.h.run();
    let on =
        r.h.get_by_role_and_label(Role::Switch, "Ask before removing a game")
            .accesskit_node()
            .toggled();
    let want = if preview.general.confirm_remove {
        egui::accesskit::Toggled::True
    } else {
        egui::accesskit::Toggled::False
    };
    assert_eq!(on, Some(want));
}

/// A row whose condition is unmet is drawn faint and takes no input: the history switches
/// while playtime is not recorded, looking up app IDs while artwork is off.
#[test]
fn settings_rows_are_greyed_by_their_condition() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 1600.0),
        ThemeChoice::Dark,
    );
    let disabled = |r: &Rig, l: &str| {
        r.h.get_by_role_and_label(Role::Switch, l)
            .accesskit_node()
            .is_disabled()
    };
    assert!(!disabled(&r, "Keep the history of removed games"));
    assert!(
        disabled(&r, "Look up app IDs by game name"),
        "artwork is off"
    );
    r.h.get_by_role_and_label(Role::Switch, "Record playtime")
        .click();
    r.h.run();
    assert!(disabled(&r, "Keep the history of removed games"));
    assert!(disabled(&r, "Show tool and installer sessions in history"));
    r.h.get_by_role_and_label(Role::RadioButton, "From Steam's image server")
        .click();
    r.h.run();
    assert!(!disabled(&r, "Look up app IDs by game name"));
}

#[test]
fn theme_change_restyles_immediately() {
    let mut r = at(
        Route::Settings(SettingsTab::Appearance),
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Light").click();
    r.h.run();
    assert_eq!(r.h.ctx.theme(), egui::Theme::Light);
    let sent = r.fake.sent();
    assert!(sent.iter().any(|c| matches!(c, UiCommand::SetPreference { preview, .. } if preview.appearance.theme == Theme::Light)));
}

#[test]
fn settings_tabs_move_with_arrow_keys() {
    let mut r = at(
        Route::Settings(SettingsTab::General),
        big(),
        ThemeChoice::Dark,
    );
    r.h.get_by_role_and_label(Role::Tab, "General").focus();
    r.h.run();
    r.h.key_press(Key::ArrowRight);
    r.h.run();
    assert_eq!(
        r.h.state().state.nav.current(),
        Route::Settings(SettingsTab::Appearance)
    );
}

#[test]
fn developer_mode_asks_first() {
    let mut r = at(
        Route::Settings(SettingsTab::Advanced),
        Vec2::new(1280.0, 1600.0),
        ThemeChoice::Dark,
    );
    r.h.get_by_label("Use Developer Mode").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Dialog, "Turn on Developer Mode?");
    r.h.key_press(Key::Enter);
    r.h.run();
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::SetPreference { .. }))
    );
}

fn settings_snap(tab: SettingsTab, name: &str, choice: ThemeChoice) {
    let mut r = at(Route::Settings(tab), Vec2::new(1280.0, 1000.0), choice);
    r.h.run_steps(4);
    r.h.snapshot(name);
}

#[test]
fn snapshot_settings_general_dark() {
    settings_snap(
        SettingsTab::General,
        "settings_general_dark_1280x1000",
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_settings_appearance_light() {
    settings_snap(
        SettingsTab::Appearance,
        "settings_appearance_light_1280x1000",
        ThemeChoice::Light,
    );
}

#[test]
fn snapshot_settings_layers_dark() {
    settings_snap(
        SettingsTab::CompatibilityLayers,
        "settings_layers_dark_1280x1000",
        ThemeChoice::Dark,
    );
}

#[test]
fn snapshot_settings_compact() {
    let mut r = at(
        Route::Settings(SettingsTab::Protection),
        Vec2::new(640.0, 480.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(4);
    r.h.snapshot("settings_protection_dark_640x480");
}

// ---- Onboarding --------------------------------------------------------------------------

fn onboarding(host: zaun_ui::bridge::HostView, choice: ThemeChoice, size: Vec2) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.onboarding.completed = false;
    // The theme the preferences ask for is the one applied.
    if matches!(choice, ThemeChoice::Light) {
        prefs.appearance.theme = Theme::Light;
    }
    fake.publish_prefs(prefs);
    fake.publish_host(host);
    let mut r = rig_on(fake, Route::Library, size, choice);
    r.h.run();
    r
}

#[test]
fn uf1_all_checks_pass_and_onboarding_completes() {
    let mut r = onboarding(demo::host_view(), ThemeChoice::Dark, big());
    // Not completed in preferences: the window opens on onboarding.
    let fake = r.fake.clone();
    assert!(matches!(
        r.h.state().state.nav.current(),
        Route::Onboarding | Route::Library
    ));
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.run();
    r.h.get_by_label("Welcome to Zaun");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_label("Checking your system");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_label("Compatibility layer");
    r.h.get_by_role_and_label(Role::Button, "Continue").click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Switch, "Official artwork")
        .click();
    r.h.run();
    r.h.get_by_role_and_label(Role::Button, "Start using Zaun")
        .click();
    r.h.run_steps(10);
    let sent = fake.sent();
    assert!(sent.contains(&UiCommand::CompleteOnboarding { skipped: false }));
    assert!(sent.iter().any(|c| matches!(c, UiCommand::SetPreference { patch, .. } if patch.keys().any(|k| k == "artwork.online"))));
    assert_eq!(r.h.state().state.nav.current(), Route::Library);
}

#[test]
fn a_blocking_failure_cannot_be_passed_and_skip_leaves_a_banner() {
    let mut r = onboarding(
        demo::host_view_without_namespaces(),
        ThemeChoice::Dark,
        big(),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = zaun_ui::vm::screens::OnboardingStep::SystemCheck;
    r.h.run();
    r.h.get_by_label_contains(
        "Private namespaces: turned off on this system, failed; fix available",
    );
    // The step's count, and Continue held back with the reason; Skip stays.
    r.h.get_by_label("Step 2 of 4: Checking your system");
    let cont = r.h.get_by_role_and_label(Role::Button, "Continue");
    assert!(cont.accesskit_node().is_disabled());
    assert_eq!(
        cont.accesskit_node().description().as_deref(),
        Some("One check failed. Games may not start until it is fixed.")
    );
    r.h.get_by_role_and_label(Role::Button, "Skip for now")
        .click();
    r.h.run();
    assert!(
        r.h.state()
            .state
            .banners
            .top()
            .is_some_and(|b| b.key == "host")
    );
}

#[test]
fn snapshot_onboarding_check_failure_dark() {
    let mut r = onboarding(
        demo::host_view_without_namespaces(),
        ThemeChoice::Dark,
        big(),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = zaun_ui::vm::screens::OnboardingStep::SystemCheck;
    r.h.run_steps(4);
    r.h.snapshot("onboarding_check_dark_1280x900");
}

#[test]
fn snapshot_onboarding_welcome_light() {
    let mut r = onboarding(demo::host_view(), ThemeChoice::Light, big());
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.run_steps(4);
    r.h.snapshot("onboarding_welcome_light_1280x900");
}

#[test]
fn snapshot_onboarding_handler_compact() {
    let mut r = onboarding(
        demo::host_view(),
        ThemeChoice::Dark,
        Vec2::new(640.0, 480.0),
    );
    r.h.state_mut().state.nav.go_to(Route::Onboarding);
    r.h.state_mut().state.onboarding.step = zaun_ui::vm::screens::OnboardingStep::Handler;
    r.h.run_steps(4);
    r.h.snapshot("onboarding_handler_dark_640x480");
}

// ---- Settings › About › Updates (ADR-0040) -------------------------------------------------

fn update_snapshot(
    channel: zaun_core::model::InstallChannel,
    download: zaun_core::model::UpdateDownload,
) -> zaun_core::model::UpdateSnapshot {
    zaun_core::model::UpdateSnapshot {
        configured: true,
        channel,
        current: "0.1.0".into(),
        last_checked: Some(now()),
        checking: false,
        available: Some(zaun_core::model::UpdateOffer {
            version: "0.2.0".into(),
            date: "2026-12-01".into(),
            notes: "Game streams in friends' calls, a quieter library and faster starts.".into(),
            has_notes: true,
        }),
        download,
        failure: None,
        rolled_back: None,
    }
}

/// About with checks on and `u` published.
fn about_with(u: zaun_core::model::UpdateSnapshot) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = demo::preferences();
    prefs.updates.check = true;
    fake.publish_prefs(prefs);
    fake.publish_updates(u);
    let mut r = rig_on(
        fake,
        Route::Settings(SettingsTab::About),
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r
}

fn sent_update(r: &Rig) -> Vec<zaun_core::service::UpdateCommand> {
    r.fake
        .sent()
        .into_iter()
        .filter_map(|c| match c {
            UiCommand::Core(Command::Updates(u)) => Some(u),
            _ => None,
        })
        .collect()
}

#[test]
fn a_copy_that_updates_itself_downloads_the_newer_version() {
    use zaun_core::model::{InstallChannel, UpdateDownload};
    use zaun_core::service::UpdateCommand;
    let mut r = about_with(update_snapshot(
        InstallChannel::Tarball,
        UpdateDownload::Idle,
    ));
    r.h.run_steps(4);
    r.h.snapshot("settings_about_update_available_dark_1280x900");
    r.h.get_by_label("Download").click();
    r.h.run();
    r.h.get_by_label("What's new").click();
    r.h.run();
    r.h.get_by_label("Check now").click();
    r.h.run();
    assert_eq!(
        sent_update(&r),
        vec![
            UpdateCommand::Download,
            UpdateCommand::OpenNotes,
            UpdateCommand::Check
        ]
    );
}

#[test]
fn a_downloaded_version_offers_restart_now() {
    use zaun_core::model::{InstallChannel, UpdateDownload};
    use zaun_core::service::UpdateCommand;
    let mut r = about_with(update_snapshot(
        InstallChannel::Tarball,
        UpdateDownload::Staged {
            version: "0.2.0".into(),
        },
    ));
    r.h.run_steps(4);
    r.h.snapshot("settings_about_update_ready_dark_1280x900");
    r.h.get_by_label("Restart now").click();
    r.h.run();
    assert_eq!(sent_update(&r), vec![UpdateCommand::Restart]);
}

#[test]
fn a_nix_install_without_nix_ld_is_told_how_to_update_and_never_downloads() {
    use zaun_core::model::{InstallChannel, UpdateDownload};
    let mut r = about_with(update_snapshot(
        InstallChannel::Nix { bundles: false },
        UpdateDownload::Idle,
    ));
    r.h.run_steps(4);
    r.h.snapshot("settings_about_update_nix_dark_1280x900");
    assert!(r.h.query_by_label("Download").is_none());
    assert!(r.h.query_by_label("Restart now").is_none());
}

#[test]
fn with_checks_off_check_now_is_dimmed() {
    let fake = FakeBackend::new(settled_library());
    fake.publish_updates(zaun_core::model::UpdateSnapshot {
        configured: true,
        current: "0.1.0".into(),
        ..Default::default()
    });
    let mut r = rig_on(
        fake,
        Route::Settings(SettingsTab::About),
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    assert!(!demo::preferences().updates.check);
    assert!(
        r.h.get_by_role_and_label(Role::Button, "Check now")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !r.h.get_by_role_and_label(Role::Switch, "Check for updates")
            .accesskit_node()
            .is_disabled()
    );
}

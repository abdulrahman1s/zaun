//! Steam games in the window (ADR-0031): the Settings switch, the "Steam" scope, the list,
//! the game menu, the selection's actions, the page of a Steam game and its tabs, Play
//! without Stop, the Statistics line, the preview's sample games, and snapshots.
//!
//! Nothing here starts Steam or reads a Steam folder: the library is the sample one and the
//! backend a fake (or the preview's).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use easyplay_core::ids::GameId;
use easyplay_core::library::PresetFilter;
use easyplay_core::model::{Activity, GameOrigin, SteamUpdate};
use easyplay_ui::bridge::{
    Backend, FolderTarget, GameEdit, LibraryView, QueryPayload, SteamAction, UiCommand, UiEvent,
};
use easyplay_ui::demo;
use easyplay_ui::route::{Route, SettingsTab};
use easyplay_ui::strings;
use easyplay_ui::theme::ThemeChoice;
use easyplay_ui::vm::details::SteamTab;
use easyplay_ui::vm::library::{Collection, FilterChip, ViewMode};
use egui::accesskit::Role;
use egui::{Key, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

const RIDGELINE: &str = "Ridgeline Rally";
const BRASS: &str = "Brass Lighthouse";
const GARDEN: &str = "Glass Garden";

/// The settled library with the sample Steam games, all idle.
fn steam_idle() -> LibraryView {
    rebuild(&settled_with_steam(), |g| g.activity = Activity::Idle)
}

fn big() -> Vec2 {
    Vec2::new(1280.0, 900.0)
}

fn settle_steps(r: &mut Rig) {
    for _ in 0..4 {
        r.h.run_steps(10);
        serve(r);
    }
    r.h.run_steps(30);
}

/// A Steam game's page, settled (its detail answered).
fn page(lib: LibraryView, name: &str, tab: SteamTab, size: Vec2) -> (Rig, GameId) {
    let id = game_id(&lib, name);
    let mut r = rig_with(lib, Route::Game(id), size, ThemeChoice::Dark);
    settle_steps(&mut r);
    r.h.state_mut().state.details.steam_tab = tab;
    settle_steps(&mut r);
    (r, id)
}

fn sent_steam(r: &Rig) -> Vec<SteamAction> {
    r.fake
        .sent()
        .into_iter()
        .filter_map(|c| match c {
            UiCommand::Steam(a) => Some(a),
            _ => None,
        })
        .collect()
}

// ---- Settings ----------------------------------------------------------------------------

#[test]
fn the_switch_turns_steam_games_on_and_check_now_waits_for_it() {
    let mut r = rig_with(
        settled_library(),
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    // Off by default; "Check now" is greyed while it is off.
    assert!(!r.h.state().bridge().prefs().steam.show_games);
    let check = r.h.get_by_label(strings::steam::CHECK_NOW());
    assert!(check.accesskit_node().is_disabled());
    r.h.get_by_role_and_label(Role::Switch, strings::steam::SHOW_OPTION())
        .click();
    r.h.run();
    let keys: Vec<String> = r
        .fake
        .sent()
        .iter()
        .filter_map(|c| match c {
            UiCommand::SetPreference { patch, preview } => {
                assert!(preview.steam.show_games);
                Some(patch.keys().map(str::to_owned).collect::<Vec<_>>())
            }
            _ => None,
        })
        .flatten()
        .collect();
    assert_eq!(keys, ["steam.show_games"]);
    // Now on: "Check now" asks Steam's files again.
    r.fake.clear_sent();
    r.h.run_steps(4);
    r.h.get_by_label(strings::steam::CHECK_NOW()).click();
    r.h.run();
    assert_eq!(sent_steam(&r), vec![SteamAction::Rescan]);
}

// ---- The library ---------------------------------------------------------------------------

#[test]
fn the_steam_scope_appears_only_with_steam_games_and_shows_only_them() {
    let word = format!("{}, ", strings::steam::SCOPE());
    let mut r = rig(Route::Library);
    r.h.run();
    assert!(r.h.query_by_label_contains(&word).is_none());

    let lib = steam_idle();
    let mut r = rig_with(
        lib.clone(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.get_by_label_contains(&word).click();
    r.h.run();
    assert_eq!(r.h.state().state.library.collection, Collection::Steam);
    let shown = r.h.state_mut().state.library.visible(&lib);
    assert_eq!(shown.len(), 3);
    assert!(shown.iter().all(|i| lib.games[*i as usize].is_steam()));
}

#[test]
fn a_steam_row_says_run_by_steam_and_never_a_level() {
    let lib = steam_idle();
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.state_mut()
        .state
        .library
        .set_collection(Collection::Steam);
    r.h.run();
    let rows: Vec<String> =
        r.h.query_all_by_role(Role::Row)
            .filter_map(|n| n.accesskit_node().label())
            .collect();
    assert_eq!(rows.len(), 3, "{rows:?}");
    for name in [RIDGELINE, BRASS, GARDEN] {
        let label = rows
            .iter()
            .find(|l| l.starts_with(&format!("{name}. ")))
            .expect("the row");
        assert!(
            label.starts_with(&format!("{name}. Run by Steam.")),
            "{label}"
        );
        for level in ["Offline", "Online", "Standard", "Custom"] {
            assert!(!label.contains(level), "{label}");
        }
    }
}

#[test]
fn a_protection_or_internet_filter_leaves_steam_games_out() {
    let lib = steam_idle();
    let mut r = rig_with(
        lib.clone(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.state_mut()
        .state
        .library
        .toggle_filter(FilterChip::Preset(PresetFilter::Standard));
    r.h.run();
    assert!(r.h.query_by_label_contains("Run by Steam").is_none());
    let shown = r.h.state_mut().state.library.visible(&lib);
    assert!(!shown.is_empty());
    assert!(shown.iter().all(|i| !lib.games[*i as usize].is_steam()));
}

/// The labels of the open game menu's items.
fn menu_items(r: &Rig) -> Vec<String> {
    r.h.query_all_by_role(Role::MenuItem)
        .filter_map(|n| n.accesskit_node().label())
        .collect()
}

#[test]
fn a_steam_games_menu_has_only_what_applies_to_it() {
    let lib = steam_idle();
    let ridge = game_id(&lib, RIDGELINE);
    let brass = game_id(&lib, BRASS);
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.state_mut().state.library.menu = Some((ridge, egui::pos2(400.0, 300.0)));
    r.h.run();
    let items = menu_items(&r);
    let has = |l: &str| items.iter().any(|i| i == l);
    use strings::menu as m;
    use strings::steam::action as sa;
    for want in [
        m::PLAY(),
        m::OPEN_PAGE(),
        sa::OPEN_IN_STEAM(),
        sa::UPDATE_IN_STEAM(),
        m::OPEN_FOLDER(),
        sa::OPEN_STORAGE(),
        m::HIDE(),
    ] {
        assert!(has(want), "{want} missing from {items:?}");
    }
    for never in [
        m::STOP(),
        m::REMOVE(),
        m::PROTECTION(),
        m::OPEN_SAVES(),
        m::ADD_SHORTCUT(),
        m::RUN_OTHER(),
        m::ARTWORK(),
    ] {
        assert!(!has(never), "{never} in {items:?}");
    }
    // "Update in Steam" sends Steam's downloads request.
    r.h.get_by_role_and_label(Role::MenuItem, sa::UPDATE_IN_STEAM())
        .click();
    r.h.run();
    assert_eq!(sent_steam(&r), vec![SteamAction::Update(ridge)]);

    // A native game without an update: no "Update in Steam", no Windows storage.
    r.h.state_mut().state.library.menu = Some((brass, egui::pos2(400.0, 300.0)));
    r.h.run();
    let items = menu_items(&r);
    assert!(
        !items.iter().any(|i| i == sa::UPDATE_IN_STEAM()),
        "{items:?}"
    );
    assert!(!items.iter().any(|i| i == sa::OPEN_STORAGE()), "{items:?}");
}

#[test]
fn a_selection_of_steam_games_offers_no_protection_and_no_removal() {
    let lib = steam_idle();
    let ids: Vec<GameId> = lib
        .games
        .iter()
        .filter(|g| g.is_steam())
        .map(|g| g.id)
        .collect();
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.state_mut().state.library.selection = ids;
    r.h.run();
    assert!(
        r.h.query_by_label(strings::library::SEL_PROTECTION())
            .is_none()
    );
    assert!(
        r.h.query_by_label(strings::library::REMOVE_SELECTED())
            .is_none()
    );
    // What applies to them stays.
    r.h.get_by_label(strings::library::SEL_FAVOURITE());
    r.h.get_by_label(strings::library::SEL_HIDE());
}

#[test]
fn delete_on_steam_games_removes_nothing_and_says_how() {
    let lib = steam_idle();
    let ridge = game_id(&lib, RIDGELINE);
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.get_by_label_contains(&format!("{RIDGELINE}. Run by Steam."))
        .focus();
    r.h.run();
    r.h.key_press(Key::Delete);
    r.h.run();
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_none());
    // The toast (and its announcement).
    assert!(
        r.h.query_all_by_label_contains(strings::steam::selection::NOT_REMOVED())
            .next()
            .is_some()
    );
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { .. }))
    );
    // The same with Steam games selected: nothing to remove, no question asked.
    let brass = game_id(r.h.state().bridge().library(), BRASS);
    r.h.state_mut().state.library.selection = vec![ridge, brass];
    r.h.run();
    r.h.get_by_label_contains(&format!("{RIDGELINE}. Run by Steam."))
        .focus();
    r.h.run();
    r.h.key_press(Key::Delete);
    r.h.run();
    assert!(r.h.query_all_by_role(Role::Dialog).next().is_none());
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RemoveGame { .. }))
    );
}

#[test]
fn a_mixed_selection_changes_protection_of_the_easyplay_games_only() {
    let lib = steam_idle();
    let lantern = game_id(&lib, "Lantern Keep");
    let ridge = game_id(&lib, RIDGELINE);
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run();
    r.h.state_mut().state.library.selection = vec![lantern, ridge];
    r.h.run();
    r.h.get_by_label(strings::library::SEL_PROTECTION()).click();
    r.h.run();
    r.h.get_by_role_and_label(Role::MenuItem, "Offline").click();
    r.h.run();
    let applied: Vec<Vec<GameId>> = r
        .fake
        .sent()
        .into_iter()
        .filter_map(|c| match c {
            UiCommand::ApplyPreset { ids, .. } => Some(ids),
            _ => None,
        })
        .collect();
    assert_eq!(applied, vec![vec![lantern]]);
    assert!(
        r.h.query_all_by_label_contains(&strings::steam::selection::skipped(1))
            .next()
            .is_some()
    );
}

#[test]
fn play_on_a_cover_hands_the_game_to_steam_and_a_running_one_has_no_stop() {
    let lib = settled_with_steam();
    let ridge = game_id(&lib, RIDGELINE);
    let mut r = rig_with(
        lib,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(8);
    r.h.get_by_label(&strings::play::play_name(RIDGELINE))
        .click();
    r.h.run_steps(4);
    assert!(r.fake.sent().contains(&UiCommand::Launch {
        id: ridge,
        exe: None
    }));
    r.fake.clear_sent();
    // The running one: Play is quiet, says why, and sends nothing; there is no Stop.
    assert!(
        r.h.query_by_label(&strings::play::stop_name(GARDEN))
            .is_none()
    );
    let garden = strings::play::play_name(GARDEN);
    let play = r.h.get_by_label(&garden);
    assert_eq!(
        play.accesskit_node().description().as_deref(),
        Some(strings::steam::action::NO_STOP())
    );
    play.click();
    r.h.run_steps(4);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { .. } | UiCommand::Stop { .. }))
    );
}

// ---- The page of a Steam game --------------------------------------------------------------

#[test]
fn the_page_asks_only_for_steam_s_detail() {
    let (r, id) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    let sent = r.fake.sent();
    for c in &sent {
        assert!(
            !matches!(
                c,
                UiCommand::LoadGame { .. }
                    | UiCommand::AboutGame { .. }
                    | UiCommand::QueryStats { .. }
            ),
            "{c:?}"
        );
    }
    assert!(r.h.state().state.details.steam.ready().is_some());
    assert_eq!(r.h.state().state.details.game, Some(id));
}

#[test]
fn the_page_has_overview_and_windows_storage_and_no_protection() {
    let (r, _) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    let tabs: Vec<String> =
        r.h.query_all_by_role(Role::Tab)
            .filter_map(|n| n.accesskit_node().label())
            .collect();
    assert_eq!(tabs, ["Overview", strings::steam::page::STORAGE_TAB()]);
    // No protection level, no switch, no shield word anywhere on the page.
    for level in ["Offline", "Online", "Standard", "Custom"] {
        assert!(r.h.query_by_label(level).is_none(), "{level}");
    }
    // The one thing that switches is the favourite star.
    let switches: Vec<String> =
        r.h.query_all_by_role(Role::Switch)
            .filter_map(|n| n.accesskit_node().label())
            .collect();
    assert!(
        switches
            .iter()
            .all(|l| l == strings::details::FAVOURITE() || l == strings::details::UNFAVOURITE()),
        "{switches:?}"
    );
    r.h.get_by_label(strings::steam::RUN_BY_STEAM());
    r.h.get_by_label_contains(strings::steam::RUN_BY_STEAM_DETAIL());

    // A game Steam runs natively: Overview only.
    let (r, _) = page(steam_idle(), BRASS, SteamTab::Overview, big());
    let tabs: Vec<String> =
        r.h.query_all_by_role(Role::Tab)
            .filter_map(|n| n.accesskit_node().label())
            .collect();
    assert_eq!(tabs, ["Overview"]);
    r.h.get_by_label_contains(strings::steam::page::NATIVE());
}

#[test]
fn the_update_note_opens_steam_s_downloads() {
    let (mut r, id) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    r.h.get_by_label_contains(strings::steam::note::WAITING_TITLE());
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::Button, strings::steam::action::UPDATE_IN_STEAM())
        .click();
    r.h.run_steps(4);
    assert_eq!(sent_steam(&r), vec![SteamAction::Update(id)]);
    // No note for a game that is up to date.
    let (r, _) = page(steam_idle(), BRASS, SteamTab::Overview, big());
    assert!(
        r.h.query_by_label_contains(strings::steam::note::WAITING_TITLE())
            .is_none()
    );
}

#[test]
fn an_updating_game_says_how_far_it_got() {
    let lib = rebuild(&steam_idle(), |g| {
        if &*g.name == RIDGELINE
            && let GameOrigin::Steam(f) = &g.origin
        {
            let mut f = (**f).clone();
            f.update = SteamUpdate::Updating;
            f.download = Some(easyplay_core::model::SteamDownload {
                done: 700_000_000,
                total: 1_400_000_000,
            });
            g.origin = GameOrigin::Steam(std::sync::Arc::new(f));
        }
    });
    let (r, _) = page(lib, RIDGELINE, SteamTab::Overview, big());
    let text = strings::steam::note::progress(
        &strings::fmt::bytes(700_000_000),
        &strings::fmt::bytes(1_400_000_000),
    );
    r.h.get_by_label_contains(&text);
    r.h.get_by_label_contains(strings::steam::note::UPDATING_TITLE());
}

#[test]
fn play_on_the_page_hands_off_and_while_steam_runs_it_sends_nothing() {
    let (mut r, id) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    r.fake.clear_sent();
    r.h.get_by_label(&strings::play::play_name(RIDGELINE))
        .click();
    r.h.run_steps(4);
    assert_eq!(
        r.fake
            .sent()
            .into_iter()
            .filter(|c| matches!(c, UiCommand::Launch { .. } | UiCommand::Stop { .. }))
            .collect::<Vec<_>>(),
        vec![UiCommand::Launch { id, exe: None }]
    );

    let (mut r, _) = page(settled_with_steam(), GARDEN, SteamTab::Overview, big());
    r.fake.clear_sent();
    assert!(
        r.h.query_by_label(&strings::play::stop_name(GARDEN))
            .is_none()
    );
    r.h.get_by_label(&strings::play::play_name(GARDEN)).click();
    r.h.run_steps(4);
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::Launch { .. } | UiCommand::Stop { .. }))
    );
}

#[test]
fn windows_storage_waits_for_the_game_to_close() {
    let (mut r, id) = page(settled_with_steam(), GARDEN, SteamTab::Storage, big());
    // "Open folder" needs only the storage; the two programs wait.
    let opens: Vec<bool> =
        r.h.query_all_by_role_and_label(Role::Button, strings::steam::page::OPEN())
            .map(|n| n.accesskit_node().is_disabled())
            .collect();
    assert_eq!(opens, [false, true]);
    assert!(
        r.h.get_by_role_and_label(Role::Button, strings::steam::page::CHOOSE())
            .accesskit_node()
            .is_disabled()
    );
    r.fake.clear_sent();
    r.h.query_all_by_role_and_label(Role::Button, strings::steam::page::OPEN())
        .next()
        .unwrap()
        .click();
    r.h.run_steps(4);
    assert_eq!(sent_steam(&r), vec![SteamAction::OpenStorageFolder(id)]);
}

#[test]
fn windows_settings_runs_in_the_storage_of_a_closed_game() {
    let (mut r, id) = page(steam_idle(), RIDGELINE, SteamTab::Storage, big());
    r.fake.clear_sent();
    let buttons: Vec<_> =
        r.h.query_all_by_role_and_label(Role::Button, strings::steam::page::OPEN())
            .collect();
    assert!(buttons.iter().all(|b| !b.accesskit_node().is_disabled()));
    buttons[1].click();
    r.h.run_steps(4);
    assert_eq!(sent_steam(&r), vec![SteamAction::WindowsSettings(id)]);
    // "Choose…" picks a program, which then runs in the game's Windows storage.
    let p = std::path::PathBuf::from("/home/sam/Downloads/ModInstaller.exe");
    r.picker.answer(vec![p.clone()]);
    r.h.get_by_role_and_label(Role::Button, strings::steam::page::CHOOSE())
        .click();
    r.h.run_steps(8);
    assert!(sent_steam(&r).contains(&SteamAction::RunProgram {
        id,
        program: p,
        args: Vec::new(),
    }));
    assert!(
        !r.fake
            .sent()
            .iter()
            .any(|c| matches!(c, UiCommand::RunTool { .. }))
    );
}

#[test]
fn the_overview_doors_lead_to_steam_and_the_folders() {
    let (mut r, id) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    r.fake.clear_sent();
    r.h.get_by_label_contains(&format!("{}: ", strings::steam::page::PROTON()))
        .click();
    r.h.run_steps(4);
    r.h.get_by_label_contains(&format!("{}: ", strings::details::GAME_FOLDER()))
        .click();
    r.h.run_steps(4);
    r.h.get_by_label_contains(&format!("{}: ", strings::steam::page::STORAGE()))
        .click();
    r.h.run_steps(4);
    let sent = r.fake.sent();
    assert!(sent.contains(&UiCommand::Steam(SteamAction::OpenInSteam(id))));
    assert!(sent.contains(&UiCommand::OpenFolder(FolderTarget::GameFolder(id))));
    assert!(sent.contains(&UiCommand::Steam(SteamAction::OpenStorageFolder(id))));
}

#[test]
fn notes_of_a_steam_game_are_saved_like_any_other() {
    let (mut r, id) = page(
        steam_idle(),
        RIDGELINE,
        SteamTab::Overview,
        Vec2::new(1280.0, 1600.0),
    );
    r.fake.clear_sent();
    r.h.get_by_role_and_label(Role::MultilineTextInput, strings::details::NOTES())
        .focus();
    r.h.run();
    r.h.get_by_role_and_label(Role::MultilineTextInput, strings::details::NOTES())
        .type_text("Stage 4 done");
    r.h.run();
    r.clock.advance(std::time::Duration::from_millis(600));
    r.h.run_steps(2);
    assert!(r.fake.sent().contains(&UiCommand::UpdateGame {
        id,
        edits: vec![GameEdit::Notes("Stage 4 done".into())],
    }));
}

#[test]
fn the_record_says_the_time_comes_from_steam() {
    let (r, _) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    let rec = r.h.get_by_label_contains("Played 74 h");
    assert_eq!(
        rec.accesskit_node().description().as_deref(),
        Some(strings::steam::RECORD_FROM_STEAM())
    );
    assert!(r.h.query_by_label_contains("Last 30 days").is_none());
}

#[test]
fn a_running_steam_game_in_the_top_bar_has_no_stop() {
    let mut r = rig_with(
        settled_with_steam(),
        Route::Statistics,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(8);
    r.h.get_by_label(&strings::nav::open_page_of(GARDEN));
    assert!(
        r.h.query_by_label(&strings::play::stop_name(GARDEN))
            .is_none()
    );
}

// ---- Statistics ----------------------------------------------------------------------------

#[test]
fn statistics_say_steam_s_total_apart() {
    let lib = steam_idle();
    let total = strings::fmt::play_time(lib.totals.steam_playtime_ms);
    assert!(lib.totals.steam_playtime_ms > 0);
    let line = strings::steam::statistics_line(&total);
    let mut r = rig_with(
        lib,
        Route::Statistics,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    r.h.get_by_label(&line);
    // Without Steam games, no line.
    let mut r = rig_with(
        settled_library(),
        Route::Statistics,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    settle(&mut r);
    assert!(
        r.h.query_by_label_contains("Steam games are not in these charts")
            .is_none()
    );
}

// ---- The preview -------------------------------------------------------------------------

fn drain(b: &demo::DemoBackend) -> Vec<UiEvent> {
    std::iter::from_fn(|| b.try_event()).collect()
}

#[test]
fn the_preview_shows_the_sample_steam_games_and_answers_steam() {
    let b = demo::DemoBackend::new(now());
    assert!(b.preferences().steam.show_games);
    let lib = b.library();
    assert_eq!(lib.games.iter().filter(|g| g.is_steam()).count(), 3);
    // The sample library itself stays as the other tests know it.
    assert_eq!(demo::library(now()).games.len(), 24);
    assert!(!demo::preferences().steam.show_games);

    let id = game_id(&lib, RIDGELINE);
    let q = easyplay_core::ids::QueryId::from_uuid(uuid::Uuid::from_u128(77));
    b.send(UiCommand::Steam(SteamAction::Details { id, reply_to: q }));
    let ev = drain(&b);
    let detail = ev.iter().find_map(|e| match e {
        UiEvent::QueryResult {
            id,
            payload: QueryPayload::Steam(d),
        } if *id == q => Some(d.clone()),
        _ => None,
    });
    let detail = detail.expect("the detail");
    assert!(detail.client_found && detail.prefix_tools && !detail.running);

    // Play: handed to Steam (a toast), no launch phases of EasyPlay's.
    b.send(UiCommand::Launch { id, exe: None });
    let ev = drain(&b);
    assert!(ev.iter().any(
        |e| matches!(e, UiEvent::Toast(t) if t.text == strings::steam::asked_to_play(RIDGELINE))
    ));
    assert!(
        !ev.iter()
            .any(|e| matches!(e, UiEvent::LaunchProgress { .. }))
    );

    // Off: the Steam games leave; on: they return.
    let mut prefs = (*b.preferences()).clone();
    prefs.steam.show_games = false;
    b.send(UiCommand::SetPreference {
        patch: easyplay_core::config::PreferencePatch::new().set("steam.show_games", false),
        preview: Box::new(prefs.clone()),
    });
    assert!(!b.library().games.iter().any(|g| g.is_steam()));
    b.send(UiCommand::Steam(SteamAction::SetEnabled(true)));
    assert_eq!(b.library().games.iter().filter(|g| g.is_steam()).count(), 3);
    let _ = drain(&b);
    b.send(UiCommand::Steam(SteamAction::Rescan));
    assert!(
        drain(&b)
            .iter()
            .any(|e| matches!(e, UiEvent::Toast(t) if t.text == strings::steam::found(3)))
    );
}

#[test]
fn the_preview_keeps_the_notes_of_a_steam_game() {
    let b = demo::DemoBackend::new(now());
    let id = game_id(&b.library(), BRASS);
    b.send(UiCommand::UpdateGame {
        id,
        edits: vec![GameEdit::Notes("Lighthouse lit".into())],
    });
    let q = easyplay_core::ids::QueryId::from_uuid(uuid::Uuid::from_u128(78));
    b.send(UiCommand::Steam(SteamAction::Details { id, reply_to: q }));
    let notes = drain(&b).into_iter().find_map(|e| match e {
        UiEvent::QueryResult {
            payload: QueryPayload::Steam(d),
            ..
        } => Some(d.notes),
        _ => None,
    });
    assert_eq!(notes.as_deref(), Some("Lighthouse lit"));
}

// ---- Snapshots -----------------------------------------------------------------------------

#[test]
fn snapshot_library_with_steam_dark_1280() {
    let mut r = rig_with(
        steam_idle(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(8);
    r.h.snapshot("library_steam_dark_1280x800");
}

#[test]
fn snapshot_library_list_with_steam_dark_1280() {
    let mut r = rig_with(
        steam_idle(),
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    r.h.state_mut().state.library.view = ViewMode::List;
    r.h.state_mut()
        .state
        .library
        .set_collection(Collection::Steam);
    r.h.run_steps(8);
    r.h.snapshot("library_steam_list_dark_1280x800");
}

#[test]
fn snapshot_steam_game_overview_dark() {
    let (mut r, _) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    r.h.run_steps(4);
    r.h.snapshot("game_steam_overview_dark_1280x900");
}

#[test]
fn snapshot_steam_game_storage_light() {
    let id = game_id(&steam_idle(), RIDGELINE);
    let mut r = rig_with(steam_idle(), Route::Game(id), big(), ThemeChoice::Light);
    settle_steps(&mut r);
    r.h.state_mut().state.details.steam_tab = SteamTab::Storage;
    settle_steps(&mut r);
    r.h.snapshot("game_steam_storage_light_1280x900");
}

#[test]
fn ctrl_tab_moves_between_the_tabs_of_a_steam_game() {
    let (mut r, _) = page(steam_idle(), RIDGELINE, SteamTab::Overview, big());
    r.h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Tab);
    r.h.run();
    assert_eq!(r.h.state().state.details.steam_tab, SteamTab::Storage);
    r.h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Tab);
    r.h.run();
    assert_eq!(r.h.state().state.details.steam_tab, SteamTab::Overview);
    // A game Steam runs natively has one tab: it stays.
    let (mut r, _) = page(steam_idle(), BRASS, SteamTab::Overview, big());
    r.h.key_press_modifiers(egui::Modifiers::COMMAND, Key::Tab);
    r.h.run();
    assert_eq!(r.h.state().state.details.steam_tab, SteamTab::Overview);
}

#[test]
fn the_steam_pages_settle() {
    // Nothing on either tab keeps asking for frames.
    for tab in [SteamTab::Overview, SteamTab::Storage] {
        let (mut r, _) = page(steam_idle(), RIDGELINE, tab, big());
        r.h.run();
        let steps = r.h.run();
        assert!(steps <= 1, "{tab:?}: {steps} steps after settling");
    }
}

#[test]
fn the_preview_window_shows_steam_games_until_the_option_is_turned_off() {
    let backend = demo::DemoBackend::new(now());
    let mut h = rig_backend(
        Box::new(backend.clone()),
        Route::Library,
        Vec2::new(1280.0, 1400.0),
        ThemeChoice::Dark,
    );
    h.run_steps(10);
    let word = format!("{}, 3", strings::steam::SCOPE());
    h.get_by_label_contains(&word);
    h.get_by_label_contains(&format!("{RIDGELINE}. Run by Steam."));
    // Settings → General → the switch off: the Steam games and their scope leave the wall.
    h.state_mut()
        .state
        .nav
        .replace(Route::Settings(SettingsTab::General));
    h.run_steps(6);
    h.get_by_role_and_label(Role::Switch, strings::steam::SHOW_OPTION())
        .click();
    h.run_steps(6);
    h.state_mut().state.nav.replace(Route::Library);
    h.run_steps(10);
    assert!(!backend.preferences().steam.show_games);
    assert!(h.query_by_label_contains(&word).is_none());
    assert!(
        h.query_by_label_contains(&format!("{RIDGELINE}. "))
            .is_none()
    );
}

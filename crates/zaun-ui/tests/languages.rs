//! The interface's language is process-wide, so every test here takes one lock and puts
//! English back before it ends. Other test files run in their own processes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::Mutex;

use common::*;
use egui::Vec2;
use egui_kittest::kittest::Queryable;
use zaun_ui::bridge::FakeBackend;
use zaun_ui::i18n::{self, Language, LanguageChoice, plural};
use zaun_ui::route::Route;
use zaun_ui::theme::{SystemPrefs, ThemeChoice};

static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn switching_the_language_changes_the_text_and_back() {
    let _g = lock();
    i18n::set(Language::English);
    assert_eq!(i18n::text("language.name"), "English");
    assert_eq!(i18n::dir(), i18n::Dir::Ltr);
    let before = i18n::generation();
    i18n::set(Language::Arabic);
    assert_eq!(i18n::language(), Language::Arabic);
    assert_eq!(i18n::text("language.name"), "العربية");
    assert_eq!(i18n::text("language.system"), "لغة النظام");
    assert_eq!(i18n::dir(), i18n::Dir::Rtl);
    assert!(i18n::generation() > before);
    i18n::set(Language::English);
    assert_eq!(i18n::text("language.name"), "English");
    assert_eq!(i18n::text("language.system"), "Same as system");
}

#[test]
fn counted_follows_each_languages_plural_rules() {
    let _g = lock();
    i18n::set(Language::English);
    assert_eq!(plural::counted("sample.games", 1), "1 game");
    assert_eq!(plural::counted("sample.games", 5), "5 games");
    i18n::set(Language::Arabic);
    assert_eq!(plural::counted("sample.games", 2), "لعبتان");
    assert_eq!(plural::counted("sample.games", 7), "7 ألعاب");
    assert_eq!(plural::counted("sample.games", 102), "102 لعبة");
    i18n::set(Language::English);
}

fn rig_in(language: &str, system: Option<SystemPrefs>) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = zaun_ui::demo::preferences();
    prefs.general.language = language.into();
    fake.publish_prefs(prefs);
    let mut r = rig_on(
        fake,
        Route::Library,
        Vec2::new(1280.0, 800.0),
        ThemeChoice::Dark,
    );
    if let Some(s) = system {
        let ctx = r.h.ctx.clone();
        r.h.state_mut().set_system_prefs(&ctx, s);
    }
    r.h.run_steps(3);
    r
}

#[test]
fn a_rig_started_with_the_arabic_preference_is_in_arabic() {
    let _g = lock();
    i18n::set(Language::English);
    let _r = rig_in("ar", None);
    assert_eq!(i18n::language(), Language::Arabic);
    i18n::set(Language::English);
}

#[test]
fn system_resolves_through_the_desktop_locale_and_defaults_to_english() {
    let _g = lock();
    i18n::set(Language::English);
    let _r = rig_in("system", None);
    assert_eq!(i18n::language(), Language::English);
    let desktop = SystemPrefs {
        locale: Some("ar_EG.UTF-8".into()),
        ..SystemPrefs::default()
    };
    let _r = rig_in("system", Some(desktop));
    assert_eq!(i18n::language(), Language::Arabic);
    assert_eq!(
        i18n::resolve(LanguageChoice::System, Some("en_GB")),
        Language::English
    );
    i18n::set(Language::English);
}

// ---- the interface in Arabic ---------------------------------------------------------

use zaun_ui::route::SettingsTab;
use zaun_ui::vm::details::DetailsTab;

fn arabic_rig(route: Route, size: Vec2) -> Rig {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = zaun_ui::demo::preferences();
    prefs.general.language = "ar".into();
    fake.publish_prefs(prefs);
    let mut r = rig_on(fake, route, size, ThemeChoice::Dark);
    settle(&mut r);
    r
}

#[test]
fn the_library_renders_in_arabic_and_controls_have_arabic_names() {
    let _g = lock();
    i18n::set(Language::English);
    let mut r = arabic_rig(Route::Library, Vec2::new(1280.0, 800.0));
    r.h.run_steps(8);
    assert_eq!(i18n::language(), Language::Arabic);
    // The top bar's destinations and the Add game button, by their Arabic names.
    r.h.get_by_label("الإحصاءات");
    r.h.get_by_label("الإعدادات");
    r.h.get_by_label_contains("إضافة لعبة");
    i18n::set(Language::English);
}

#[test]
fn a_game_page_settings_and_the_add_game_dialog_render_in_arabic() {
    let _g = lock();
    i18n::set(Language::English);
    let lib = settled_library();
    let id = game_id(&lib, "Lantern Keep");
    for (tab, label) in [
        (DetailsTab::Overview, "نظرة عامة"),
        (DetailsTab::Protection, "الحماية"),
        (DetailsTab::Compatibility, "التوافق"),
    ] {
        let fake = FakeBackend::new(lib.clone());
        let mut prefs = zaun_ui::demo::preferences();
        prefs.general.language = "ar".into();
        fake.publish_prefs(prefs);
        let mut r = rig_on(
            fake,
            Route::Game(id),
            Vec2::new(1280.0, 900.0),
            ThemeChoice::Dark,
        );
        r.h.state_mut().state.details.tab = tab;
        settle(&mut r);
        r.h.run_steps(6);
        r.h.get_by_label_contains("تشغيل Lantern Keep");
        r.h.get_by_label(label);
    }
    let mut r = arabic_rig(
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 900.0),
    );
    r.h.run_steps(6);
    r.h.get_by_label("اللغة");
    let mut r = arabic_rig(Route::Library, Vec2::new(1280.0, 800.0));
    zaun_ui::screens::add_game::open(&mut r.h.state_mut().state);
    r.h.run_steps(6);
    r.h.get_by_role_and_label(egui::accesskit::Role::Dialog, "إضافة لعبة");
    i18n::set(Language::English);
}

#[test]
fn choosing_a_language_in_settings_changes_the_interface_at_once() {
    let _g = lock();
    i18n::set(Language::English);
    let fake = FakeBackend::new(settled_library());
    let mut prefs = zaun_ui::demo::preferences();
    prefs.general.language = "system".into();
    fake.publish_prefs(prefs);
    let mut r = rig_on(
        fake,
        Route::Settings(SettingsTab::General),
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    r.h.run_steps(6);
    r.h.get_by_label("Language");
    // The segmented control names each language in itself.
    r.h.get_by_label("English");
    r.h.get_by_label("العربية").click();
    r.h.run_steps(6);
    assert_eq!(i18n::language(), Language::Arabic);
    r.h.get_by_label("اللغة");
    r.h.get_by_label("English").click();
    r.h.run_steps(6);
    assert_eq!(i18n::language(), Language::English);
    r.h.get_by_label("Language");
    i18n::set(Language::English);
}

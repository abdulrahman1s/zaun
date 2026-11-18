//! What the interface keeps in memory stays bounded as the app runs (AGENTS.md §6: every
//! growing collection has an eviction rule).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use crate::common;

use common::{rig_with, settle, settled_library};
use egui::Vec2;
use zaun_ui::route::Route;
use zaun_ui::theme::ThemeChoice;

/// Every library change asks for the overview (and the long "Most played" list) again; the
/// answers replace the range's earlier ones instead of piling up, one per revision.
#[test]
fn statistics_keep_one_overview_per_range_as_the_library_changes() {
    let mut r = rig_with(
        settled_library(),
        Route::Statistics,
        Vec2::new(1280.0, 900.0),
        ThemeChoice::Dark,
    );
    // "Show all" under Most played: the longer list is asked for too.
    r.h.state_mut().state.stats.all_games = true;
    settle(&mut r);
    let first = r.h.state().bridge().library().revision;
    for _ in 0..5 {
        let lib = (**r.h.state().bridge().library()).clone();
        r.fake.publish_library(lib);
        settle(&mut r);
    }
    assert_eq!(r.h.state().bridge().library().revision, first + 5);
    assert_eq!(r.h.state().state.stats.cache.len(), 1);
    assert_eq!(r.h.state().state.stats.top.len(), 1);
}

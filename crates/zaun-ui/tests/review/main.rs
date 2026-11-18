//! Review renders (throwaway, opt-in): PNGs in `target/review/` to compare with the
//! redesign prototype, not assertions. Built only with the `review` feature:
//! `cargo test -p zaun-ui --features review --test review -- --ignored --test-threads=1`.

#[path = "../common/mod.rs"]
mod common;

mod review_controls;
mod review_game;
mod review_i18n;
mod review_library;
mod review_overlays;
mod review_pages;
mod review_shell;
mod review_steam;

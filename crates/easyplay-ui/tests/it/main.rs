//! All integration tests of this crate in one binary: linking one test binary instead of
//! one per file is most of the build time. Files that need a process of their own stay
//! next to this directory as separate binaries.
//!
//! Run one file: `cargo test -p easyplay-ui --test it <module>::`.

#[path = "../common/mod.rs"]
mod common;

mod catalogue;
mod controller;
mod debug_views;
mod details;
mod flows;
mod idle;
mod keyboard_flows;
mod memory;
mod modal_focus;
mod real_service;
mod scaling;
mod screen_reader;
mod screens;
mod security_wording;
mod shell;
mod steam;
mod themes;
mod widgets;

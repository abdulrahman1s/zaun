//! All integration tests of this crate in one binary: linking one test binary instead of
//! one per file is most of the build time. Files that need a process of their own stay
//! next to this directory as separate binaries.
//!
//! Run one file: `cargo test -p zaun --test it <module>::`.

mod cli;
mod steam_cli;

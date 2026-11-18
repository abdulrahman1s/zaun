//! All integration tests of this crate in one binary: linking one test binary instead of
//! one per file is most of the build time. Files that need a process of their own stay
//! next to this directory as separate binaries.
//!
//! Run one file: `cargo test -p zaun-core --test it <module>::`.

mod artwork;
mod chat_calls;
mod chat_overlay;
mod chat_overlay_scene;
mod chat_streams;
mod config_prefs;
mod desktop;
mod failure_matrix;
mod foundation;
mod host_probes;
mod hostile_prefix;
mod identity;
mod launch_m5;
mod launch_orchestrator;
mod library;
mod runtime_adopt;
mod runtime_catalog;
mod runtime_catalog_sqlite;
mod runtime_integrity;
mod runtime_trusted;
mod sandbox_explain_policy;
mod sandbox_golden;
mod sandbox_props;
mod sandbox_sections;
mod sandbox_validate;
mod service_diagnostics;
mod service_flows;
mod service_flows_wave4;
mod stats;
mod stats_order;
mod steam_launch_options;
mod steam_library;
mod steam_prefix;
mod storage_m2;
mod storage_repos;
mod umu_contract;
mod umu_log_corpus;

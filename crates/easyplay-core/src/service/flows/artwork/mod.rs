//! Artwork flows (LOGIC_SPECS §14, ADR-0018): fetch, import, clear, find a Steam app.

mod clear;
mod fetch;
mod fetched;
mod find_steam_app;
mod import;
mod program_icon;

pub(crate) use fetched::ArtFetched;

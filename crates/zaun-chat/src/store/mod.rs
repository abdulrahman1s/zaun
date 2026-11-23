//! The engine's sealed SQLite store behind a synchronous API (ADR-0042, ADR-0017).

mod db;
mod error;
mod files;
mod handle;
mod items;
mod kv;
mod migrate;
mod op;
mod seal;
mod seq;
#[cfg(test)]
mod tests;

pub use error::StoreError;
pub use handle::Store;
pub use op::Op;

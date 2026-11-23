//! The engine's records in the store, namespace "chat", versioned JSON (ADR-0042).

pub mod conn;
pub mod format;
pub mod friend;
pub mod item;
pub mod keys;
pub mod settings;
pub mod user;

pub use conn::{ConnKind, ConnRecord};
pub use format::{NS, VERSION, decode, encode};
pub use friend::FriendRecord;
pub use item::ItemRecord;
pub use settings::SettingsRecord;
pub use user::UserRecord;

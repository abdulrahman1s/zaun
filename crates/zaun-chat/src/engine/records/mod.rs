//! The engine's records in the store, namespace "chat", versioned JSON (ADR-0042).

pub mod address;
pub mod conn;
pub mod file;
pub mod format;
pub mod friend;
pub mod group;
pub mod item;
pub mod keys;
pub mod member;
pub mod settings;
pub mod user;

pub use address::{AddressRecord, RequestRecord};
pub use conn::{ConnKind, ConnRecord};
pub use format::{NS, VERSION, decode, encode};
pub use friend::FriendRecord;
pub use group::{GroupRecord, MemberStatus};
pub use item::ItemRecord;
pub use member::{MemberCategory, MemberRecord, MemberRelation};
pub use settings::SettingsRecord;
pub use user::UserRecord;

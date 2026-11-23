//! Wire copies of core's `model::chat` types; times are milliseconds since the epoch.

pub mod body;
pub mod bytes;
pub mod call;
pub mod chat_features;
pub mod chat_ref;
pub mod friend;
pub mod group;
pub mod message;
pub mod picture;
pub mod preview;
pub mod profile;
pub mod stream;

pub use body::{CallKind, CallRecord, ChatNotice, FileState, Media, MessageBody, StreamRecord};
pub use call::{CallKey, CallRequest, CallSignal};
pub use chat_features::ChatFeatures;
pub use chat_ref::ChatRef;
pub use friend::{Friend, FriendRequest};
pub use group::{ChatGroup, GroupMember, Membership};
pub use message::{ChatMessage, Delivery, Mention, MessageAuthor, Quote, Reaction};
pub use picture::Picture;
pub use preview::ChatPreview;
pub use profile::{ChatProfile, MyStatus, Playing, Presence};
pub use stream::{StreamRequest, StreamSignal};

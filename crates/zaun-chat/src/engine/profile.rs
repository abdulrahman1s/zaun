//! Profiles between the user's record, `x.info` and the page (Types.hs `Profile`, `userProfileDirect`).

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use zaun_chat_wire::model::Picture;

use super::records::UserRecord;
use crate::chat::compress::compress_to_limit;
use crate::chat::image::ImageData;
use crate::chat::limits::MAX_CONN_INFO_LENGTH_PQ;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, Result, parse_chat_messages};

/// The user's profile as sent: the full default preferences, the line about them, the picture.
#[must_use]
pub fn sent_profile(user: &UserRecord) -> Profile {
    let mut p = Profile::new(&user.name, "");
    p.short_descr = Some(user.about.clone()).filter(|a| !a.is_empty());
    p.image = user.picture.as_ref().map(data_uri);
    p
}

/// The conn-info of a handshake: `x.info` without a message id, compressed to the PQ limit
/// (`compressConnInfo`), since every connection this engine makes is PQ.
pub fn conn_info(user: &UserRecord) -> Result<Vec<u8>> {
    let msg = ChatMessage::without_id(ChatEvent::Info {
        profile: sent_profile(user),
        member_key: None,
    });
    compress_to_limit(&msg.to_json()?, MAX_CONN_INFO_LENGTH_PQ)
}

/// The profile in a peer's conn-info (`x.info`, or `x.contact` through an address).
#[must_use]
pub fn received_profile(conn_info: &[u8]) -> Option<Profile> {
    parse_chat_messages(conn_info)
        .ok()?
        .into_iter()
        .find_map(|m| {
            if let ChatEvent::Info { profile, .. } | ChatEvent::Contact { profile, .. } = m.event {
                Some(profile)
            } else {
                None
            }
        })
}

fn data_uri(p: &Picture) -> ImageData {
    let kind = if p.mime == "image/png" { "png" } else { "jpg" };
    ImageData(format!(
        "data:image/{kind};base64,{}",
        STANDARD.encode(&p.bytes)
    ))
}

/// A received `data:` URI as a picture; anything but a JPEG or PNG is dropped.
#[must_use]
pub fn picture_of(uri: &str) -> Option<Picture> {
    let image = ImageData(uri.to_owned());
    let safe = image.safe();
    let (head, b64) = safe.split_once(";base64,")?;
    let mime = if head.ends_with("png") {
        "image/png"
    } else {
        "image/jpeg"
    };
    Some(Picture {
        bytes: STANDARD.decode(b64.trim()).ok()?,
        mime: mime.to_owned(),
    })
}

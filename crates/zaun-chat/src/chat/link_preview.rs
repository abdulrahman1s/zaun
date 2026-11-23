//! Link previews inside `link` content (Protocol.hs `LinkPreview`).
use super::image::ImageData;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LinkPreview {
    pub uri: String,
    pub title: String,
    pub description: String,
    pub image: ImageData,
    /// `LinkContent` (page, image, video, or an unknown tag): kept as the JSON that was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
}

//! `ImageData`: a `data:` URI that is cleared on encode unless it is a real JPEG or PNG (Types.hs `safeImageData`).
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Serialize, Serializer};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct ImageData(pub String);

const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF];
const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";

impl ImageData {
    /// The text that is sent: the image itself, or `""` for anything but jpg/png with a valid header.
    pub fn safe(&self) -> &str {
        let t = self.0.as_str();
        let b64 = t
            .strip_prefix("data:image/jpg;base64,")
            .or_else(|| t.strip_prefix("data:image/png;base64,"));
        let header = b64.and_then(|b| STANDARD.decode(b.chars().take(12).collect::<String>()).ok());
        match header {
            Some(h) if h.starts_with(JPEG) || h.starts_with(PNG) => t,
            _ => "",
        }
    }
}

impl Serialize for ImageData {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.safe())
    }
}

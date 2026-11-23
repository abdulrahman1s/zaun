//! The engine's own settings record.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsRecord {
    /// `sendRcptsContacts`, on for new users (Store/Profiles.hs).
    pub send_receipts: bool,
}

impl Default for SettingsRecord {
    fn default() -> Self {
        Self {
            send_receipts: true,
        }
    }
}

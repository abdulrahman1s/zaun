//! File invitations and XFTP descriptions inside messages (Types.hs `FileInvitation`, `FileDescr`).
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDescr {
    pub file_descr_text: String,
    pub file_descr_part_no: i64,
    pub file_descr_complete: bool,
}

/// `file_conn_req`, `file_digest` and `file_inline` belong to the legacy direct-file protocol and stay opaque.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInvitation {
    pub file_name: String,
    pub file_size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_conn_req: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_inline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_descr: Option<FileDescr>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_badge: Option<Value>,
}

impl FileInvitation {
    /// The placeholder an XFTP send announces: the real description follows in `x.msg.file.descr`.
    pub fn xftp(file_name: &str, file_size: u64) -> Self {
        Self {
            file_name: file_name.to_owned(),
            file_size,
            file_digest: None,
            file_conn_req: None,
            file_inline: None,
            file_descr: Some(FileDescr {
                file_descr_text: String::new(),
                file_descr_part_no: 0,
                file_descr_complete: false,
            }),
            file_badge: None,
        }
    }
}

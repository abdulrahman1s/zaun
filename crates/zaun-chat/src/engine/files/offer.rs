//! Whether a file may go and to whom its offer goes: the user's own file, a connected friend, or a group that allows files (Commands.hs `checkSndFile`; Internal.hs `prohibitedGroupContent` when sending).

use std::fs;
use std::path::{Path, PathBuf};

use zaun_chat_wire::model::ChatRef;

use super::send::PQ;
use crate::chat::group::GroupMemberRole;
use crate::chat::{ChatMessage, encode_chat_message};
use crate::engine::agent_port::{AgentMsgId, AgentPort, ConnId};
use crate::engine::error::{Code, Result, fail};
use crate::engine::groups::policy;
use crate::engine::machine::Engine;
use crate::xftp::MAX_FILE_SIZE;

/// The file as the engine may send it: inside its own data folder (a path from outside is
/// never read), a regular file, not empty, within the size limit (`checkSndFile`).
pub(super) fn checked(dir: &Path, path: &Path) -> Result<(PathBuf, u64)> {
    let own = fs::canonicalize(dir).map_err(|_| fail(Code::RequestFailed, "no data folder"))?;
    let file = fs::canonicalize(path).map_err(|_| fail(Code::RequestFailed, "file not found"))?;
    if !file.starts_with(&own) {
        return Err(fail(Code::RequestFailed, "file is outside the data folder"));
    }
    let meta = fs::metadata(&file).map_err(|_| fail(Code::RequestFailed, "file not found"))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > MAX_FILE_SIZE {
        return Err(fail(Code::RequestFailed, "file size is not allowed"));
    }
    Ok((file, meta.len()))
}

impl<A: AgentPort> Engine<A> {
    /// A friend must be connected; in a group the user must be an author and files allowed
    /// for their role (Internal.hs `prohibitedGroupContent` when sending).
    pub(super) fn may_send_file(&self, chat: ChatRef) -> Result<()> {
        match chat {
            ChatRef::Direct(friend) => {
                if !self.friend(friend)?.reachable() {
                    return Err(fail(Code::RequestFailed, "not connected"));
                }
            }
            ChatRef::Group(group) => {
                let g = self.group(group)?;
                Self::assert_my_role(&g, &GroupMemberRole::Author)?;
                if !policy::allowed(&g, policy::Feature::Files, &g.my_role) {
                    return Err(fail(Code::RequestFailed, "feature not allowed: files"));
                }
            }
        }
        Ok(())
    }

    /// The message with the file's invitation: to the friend, or to each member reachable
    /// now. What was sent: (member, connection, agent message id); the member is 0 for a friend.
    pub(super) fn send_offer(
        &mut self,
        chat: ChatRef,
        msg: &ChatMessage,
    ) -> Result<Vec<(i64, ConnId, AgentMsgId)>> {
        match chat {
            ChatRef::Direct(friend) => {
                let conn = self.friend(friend)?.conn;
                let body = encode_chat_message(msg, PQ)?;
                let id = self.agent.send(&mut self.store, &conn, body)?;
                Ok(vec![(0, conn, id)])
            }
            ChatRef::Group(group) => self.send_to_group(group, msg, None),
        }
    }
}

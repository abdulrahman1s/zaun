//! SetGroupPicture: the owner changes the group profile's image and sends `x.grp.info` (Commands.hs `APIUpdateGroupProfile`, `runUpdateGroupProfile`).

use zaun_chat_wire::model::Picture;

use super::rank::assert_user_role;
use crate::chat::group::GroupMemberRole;
use crate::chat::{ChatEvent, ChatMessage};
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::{MAX_PICTURE_URI, checked_picture, sent_profile};
use crate::engine::records::UserRecord;

impl<A: AgentPort> Engine<A> {
    /// Only the image changes; the rest of the group's profile stays as the group has it.
    pub(crate) fn set_group_picture(&mut self, group: i64, picture: Option<Picture>) -> Result<()> {
        let mut g = self.group(group)?;
        assert_user_role(&g, &GroupMemberRole::Owner)?;
        if let Some(p) = &picture {
            checked_picture(p).map_err(|why| fail(Code::RequestFailed, why))?;
        }
        // The same `data:` URI the user's own picture is sent as.
        let image = sent_profile(&UserRecord {
            picture,
            ..self.state.user.clone()
        })
        .image;
        if let Some(i) = &image
            && i.0.len() > MAX_PICTURE_URI
        {
            return Err(fail(
                Code::RequestFailed,
                format!("Profile image is too large {}", i.0.len()),
            ));
        }
        if g.profile.image == image {
            self.state.chats_dirty = true;
            return Ok(());
        }
        g.profile.image = image;
        let msg = ChatMessage::new(ChatEvent::GrpInfo(g.profile.clone()))?;
        self.save_group(g)?;
        self.send_to_group(group, &msg, None)?;
        self.state.chats_dirty = true;
        Ok(())
    }
}

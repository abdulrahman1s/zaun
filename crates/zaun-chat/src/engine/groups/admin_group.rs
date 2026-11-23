//! The group itself changed by an owner: deleted, new profile, new preferences (Subscriber.hs `xGrpDel`, `xGrpInfo`, `xGrpPrefs`).

use zaun_chat_wire::model::ChatNotice;

use super::admin::insufficient;
use super::inbound::GroupCtx;
use super::rank::at_least;
use crate::chat::group::GroupMemberRole;
use crate::chat::group_prefs::GroupPreferences;
use crate::chat::group_profile::GroupProfile;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::MemberStatus;

impl<A: AgentPort> Engine<A> {
    fn sender_is_owner(&self, ctx: GroupCtx, event: &str) -> Result<()> {
        let sender = self.member(ctx.group, ctx.author)?;
        if at_least(&sender.role, &GroupMemberRole::Owner) {
            return Ok(());
        }
        Err(insufficient(event))
    }

    /// Subscriber.hs 3719: owner only.
    pub(crate) fn on_group_del(&mut self, ctx: GroupCtx) -> Result<()> {
        self.sender_is_owner(ctx, "x.grp.del")?;
        let g = self.group(ctx.group)?;
        if g.status == MemberStatus::GroupDeleted {
            return Ok(());
        }
        self.delete_group_link_if_exists(ctx.group)?;
        self.drop_group_conns(ctx.group)?;
        let mut g = self.group(ctx.group)?;
        g.status = MemberStatus::GroupDeleted;
        self.save_group(g)?;
        self.group_notice(ctx.group, Some(ctx.author), ChatNotice::GroupDeleted)
    }

    /// Subscriber.hs 3731: owner only, and never a channel's profile.
    pub(crate) fn on_group_info(&mut self, ctx: GroupCtx, p: GroupProfile) -> Result<()> {
        self.sender_is_owner(ctx, "x.grp.info")?;
        if p.public_group.is_some() {
            return Err(fail(
                Code::RequestFailed,
                "x.grp.info: publicGroup not allowed in p2p groups",
            ));
        }
        let mut g = self.group(ctx.group)?;
        if g.profile == p {
            return Ok(());
        }
        let renamed = g.profile.display_name != p.display_name;
        let name = p.display_name.clone();
        g.profile = p;
        self.save_group(g)?;
        self.state.chats_dirty = true;
        if renamed {
            self.group_notice(
                ctx.group,
                Some(ctx.author),
                ChatNotice::GroupNameChanged(name),
            )?;
        }
        Ok(())
    }

    /// Subscriber.hs 3817: owner only.
    pub(crate) fn on_group_prefs(&mut self, ctx: GroupCtx, p: GroupPreferences) -> Result<()> {
        self.sender_is_owner(ctx, "x.grp.prefs")?;
        let mut g = self.group(ctx.group)?;
        if g.profile.group_preferences.as_ref() == Some(&p) {
            return Ok(());
        }
        g.profile.group_preferences = Some(p);
        self.save_group(g)?;
        self.state.chats_dirty = true;
        Ok(())
    }
}

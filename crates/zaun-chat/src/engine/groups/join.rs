//! JoinGroup: accept an invitation, joining its link with `x.grp.acpt` (Commands.hs `APIJoinGroup`; Internal.hs `encodeXGrpAcpt`).

use super::conn_info::group_conn_info;
use crate::agent::links::ConnectionLink;
use crate::chat::ChatEvent;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::records::{ConnKind, ConnRecord, MemberStatus};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn join_group(&mut self, group: i64) -> Result<()> {
        let mut g = self.group(group)?;
        let (Some(inv), Some(host), MemberStatus::Invited) = (&g.invitation, g.host, g.status)
        else {
            return Err(fail(Code::RequestFailed, "no invitation to join"));
        };
        let link = ConnectionLink::parse(&inv.conn_request)
            .map_err(|e| fail(Code::BadLink, e.to_string()))?;
        // The member key is optional in classic groups (`opt "memberKey"`): none is sent.
        let info = group_conn_info(ChatEvent::GrpAcpt {
            member_id: g.my_member_id.clone(),
            member_key: None,
            profile: Some(self.handshake_profile()),
        })?;
        // Both statuses change only once the join is under way, so a failure leaves them Invited.
        let conn = self.agent.join(&mut self.store, &link, info, false)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Member {
                group,
                member: host,
            },
        })?;
        let mut h = self.member(group, host)?;
        h.conn = Some(conn);
        h.status = MemberStatus::Accepted;
        self.save_member(h)?;
        g.status = MemberStatus::Accepted;
        self.save_group(g)?;
        self.state.chats_dirty = true;
        Ok(())
    }
}

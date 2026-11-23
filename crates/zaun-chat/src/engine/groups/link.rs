//! A group's link: a contact address whose `clientData` names the group, made, read back and deleted (Commands.hs `APICreateGroupLink`, `APIDeleteGroupLink`, Internal.hs `deleteGroupLinkIfExists`).

use zaun_chat_wire::{Event, RequestKind};

use super::link_data::{client_data, link_data};
use super::rank::assert_user_role;
use crate::chat::group::GroupMemberRole;
use crate::engine::agent_port::{AgentEvent, AgentFailure, AgentPort, ConnId};
use crate::engine::error::{EngineError, Result};
use crate::engine::machine::Engine;
use crate::engine::records::{ConnKind, ConnRecord};

impl<A: AgentPort> Engine<A> {
    pub(crate) fn create_group_link(&mut self, group: i64) -> Result<()> {
        let mut g = self.group(group)?;
        assert_user_role(&g, &GroupMemberRole::Admin)?;
        // A link the group has is read back, not made again (`SEDuplicateGroupLink`).
        if g.link_conn.is_some() {
            if g.link.is_some() {
                self.emit(Event::GroupLink {
                    group,
                    link: g.link,
                });
            }
            return Ok(());
        }
        let data = link_data(&g.profile)?;
        let conn = self
            .agent
            .create_group_address(&mut self.store, data, client_data()?)?;
        // The record is what routes the agent's `Address` here.
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::GroupLink { group },
        })?;
        g.link_conn = Some(conn);
        self.save_group(g)?;
        Ok(())
    }

    /// A group without a link has nothing to delete, and the page is told so.
    pub(crate) fn delete_group_link(&mut self, group: i64) -> Result<()> {
        self.group(group)?;
        self.delete_group_link_if_exists(group)?;
        self.emit(Event::GroupLink { group, link: None });
        Ok(())
    }

    pub(crate) fn delete_group_link_if_exists(&mut self, group: i64) -> Result<()> {
        let mut g = self.group(group)?;
        let Some(conn) = g.link_conn.take() else {
            return Ok(());
        };
        g.link = None;
        self.save_group(g)?;
        self.forget_conn(&conn)?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// `Address` and `Req` on the link's connection.
    pub(crate) fn on_group_link_agent_event(&mut self, group: i64, e: AgentEvent) -> Result<()> {
        match e {
            AgentEvent::Address { conn, full, short } => {
                self.on_group_link_made(group, conn, full, short)
            }
            AgentEvent::Req {
                invitation,
                conn_info,
                ..
            } => self.on_link_request(group, invitation, &conn_info),
            AgentEvent::Invitation { .. }
            | AgentEvent::Conf { .. }
            | AgentEvent::Info { .. }
            | AgentEvent::Con { .. }
            | AgentEvent::Msg { .. }
            | AgentEvent::Sent { .. }
            | AgentEvent::Merr { .. }
            | AgentEvent::Rcvd { .. }
            | AgentEvent::Up { .. }
            | AgentEvent::Down { .. }
            | AgentEvent::Err { .. }
            | AgentEvent::RatchetSync { .. } => Ok(()),
        }
    }

    /// The short link when the relay keeps one, as the page shows it.
    fn on_group_link_made(
        &mut self,
        group: i64,
        conn: ConnId,
        full: String,
        short: Option<String>,
    ) -> Result<()> {
        let mut g = self.group(group)?;
        if g.link_conn != Some(conn) {
            self.log
                .line(format_args!("a group link nobody asked for was dropped"));
            return self.forget_conn(&conn);
        }
        let link = short.unwrap_or(full);
        g.link = Some(link.clone());
        self.save_group(g)?;
        self.emit(Event::GroupLink {
            group,
            link: Some(link),
        });
        self.state.chats_dirty = true;
        Ok(())
    }

    /// An error while the link is being made ends the attempt; one on a made link is only logged.
    pub(crate) fn on_group_link_error(&mut self, group: i64, err: &AgentFailure) -> Result<()> {
        let mut g = self.group(group)?;
        let (None, Some(conn)) = (&g.link, g.link_conn) else {
            self.log
                .line(format_args!("a group link failed: {}", err.detail()));
            return Ok(());
        };
        g.link_conn = None;
        self.save_group(g)?;
        self.forget_conn(&conn)?;
        let e = EngineError::from(err.clone());
        self.emit(Event::Failed {
            request: RequestKind::GroupLink,
            code: e.code.as_str().to_owned(),
            detail: e.detail,
        });
        Ok(())
    }
}

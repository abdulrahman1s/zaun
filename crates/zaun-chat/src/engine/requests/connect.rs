//! Connect: join a friend's invitation link. A bad link never reaches the network; the user's own
//! link connects nothing, and a link already joined only reads the list again (codec `on_plan`).

use crate::agent::links::{ConnShortLink, ConnectionLink, ConnectionRequestUri, ContactConnType};
use crate::engine::agent_port::{AgentFailure, AgentPort};
use crate::engine::error::{Code, EngineError, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::conn_info;
use crate::engine::records::{ConnKind, ConnRecord, FriendRecord, keys};

use super::invite::PQ;

/// The `simplex:` form, so the same link from a page or a QR code compares equal.
fn canonical(link: ConnectionLink) -> ConnectionLink {
    match link {
        ConnectionLink::Full(f) => ConnectionLink::Full(f.into_simplex()),
        ConnectionLink::Short(s) => ConnectionLink::Short(s.into_simplex()),
    }
}

fn parse(text: &str) -> Option<ConnectionLink> {
    ConnectionLink::parse(text).ok().map(canonical)
}

/// Only invitations for now; addresses and group links answer as a refused link does.
fn check_kind(link: &ConnectionLink) -> Result<()> {
    let what = match link {
        ConnectionLink::Full(ConnectionRequestUri::Invitation { .. })
        | ConnectionLink::Short(ConnShortLink::Invitation { .. }) => return Ok(()),
        ConnectionLink::Full(ConnectionRequestUri::Contact { .. }) => "an address or group link",
        ConnectionLink::Short(ConnShortLink::Contact { contact_type, .. }) => match contact_type {
            ContactConnType::Contact => "an address",
            ContactConnType::Group | ContactConnType::Channel | ContactConnType::Relay => {
                "a group link"
            }
        },
    };
    Err(fail(Code::BadLink, format!("not yet: {what}")))
}

fn connect_error(e: AgentFailure) -> EngineError {
    match e {
        AgentFailure::Network(d) => fail(Code::NoConnection, d),
        AgentFailure::BadLink(d) | AgentFailure::Other(d) => fail(Code::BadLink, d),
    }
}

impl<A: AgentPort> Engine<A> {
    pub(crate) fn connect(&mut self, text: &str) -> Result<()> {
        let text = text.trim();
        if text.is_empty() || text.chars().any(char::is_whitespace) {
            return Err(fail(Code::BadLink, "empty or contains spaces"));
        }
        let link = parse(text).ok_or_else(|| fail(Code::BadLink, "not a link"))?;
        check_kind(&link)?;
        if self.is_own(&link) {
            return Err(fail(Code::BadLink, "the user's own link"));
        }
        let uri = link.to_uri();
        if self
            .state
            .friends
            .values()
            .any(|f| f.link.as_deref() == Some(&uri))
        {
            self.state.chats_dirty = true;
            return Ok(());
        }
        let info = conn_info(&self.state.user)?;
        let conn = self
            .agent
            .join(&mut self.store, &link, info, PQ)
            .map_err(connect_error)?;
        let id = self.store.next_id(keys::FRIEND_SEQ)?;
        let mut friend = FriendRecord::new(id, conn, self.clock.wall_ms());
        friend.link = Some(uri);
        self.save_friend(friend)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Friend { id },
        })?;
        self.state.chats_dirty = true;
        Ok(())
    }

    /// One of our invitations, answered or not.
    fn is_own(&self, link: &ConnectionLink) -> bool {
        let same = |own: &String| parse(own).as_ref() == Some(link);
        let open = self.state.conns.values().any(|c| match &c.kind {
            ConnKind::Invitation { full, short, .. } => {
                [full, short].into_iter().flatten().any(same)
            }
            ConnKind::Friend { .. } => false,
        });
        open || self
            .state
            .friends
            .values()
            .any(|f| f.invited_with.iter().any(same))
    }
}

//! A request through the address: listed with the profile it brought, once per person (Subscriber.hs `processContactConnMessage` REQ, `profileContactRequest`).

use super::MAX_REQUESTS;
use super::info::{hex, parse_request};
use crate::engine::agent_port::{AgentPort, ConnId};
use crate::engine::error::Result;
use crate::engine::machine::Engine;
use crate::engine::records::{NS, RequestRecord, keys};

impl<A: AgentPort> Engine<A> {
    /// Anything but an `x.contact` (or a legacy `x.info`) on our address is dropped, as is a person already accepted.
    pub(crate) fn on_contact_request(
        &mut self,
        conn: ConnId,
        invitation: Vec<u8>,
        conn_info: &[u8],
    ) -> Result<()> {
        let ours = self
            .state
            .address
            .record
            .as_ref()
            .is_some_and(|a| a.conn == conn);
        let Some((profile, req_id)) = parse_request(conn_info).filter(|_| ours) else {
            return self.drop_request(&invitation);
        };
        let req_id = req_id.as_deref().map(hex);
        let accepted = self.state.address.record.as_ref().is_some_and(|a| {
            let friend = req_id.as_ref().and_then(|id| a.accepted_friend(id));
            friend.is_some_and(|f| self.state.friends.contains_key(&f))
        });
        if accepted {
            return self.drop_request(&invitation);
        }
        // The same person asking again replaces the earlier request and keeps its place.
        let earlier = req_id.as_ref().and_then(|id| {
            self.state
                .address
                .requests
                .values()
                .find(|r| r.req_id.as_ref() == Some(id))
                .cloned()
        });
        let id = match earlier {
            Some(r) => {
                self.drop_request(&r.invitation)?;
                r.id
            }
            None => {
                self.make_room()?;
                self.store.next_id(keys::REQUEST_SEQ)?
            }
        };
        let rec = RequestRecord {
            id,
            invitation,
            name: profile.display_name,
            about: profile.short_descr.unwrap_or_default(),
            image: profile.image.map(|i| i.0),
            req_id,
            since: self.clock.wall_ms(),
        };
        self.put(&keys::request(id), &rec)?;
        self.state.address.requests.insert(id, rec);
        self.state.chats_dirty = true;
        Ok(())
    }

    /// Tells the agent to forget a request; one it no longer has is only logged.
    pub(super) fn drop_request(&mut self, invitation: &[u8]) -> Result<()> {
        if let Err(e) = self.agent.reject_request(&mut self.store, invitation) {
            self.log
                .line(format_args!("request not dropped: {}", e.detail()));
        }
        Ok(())
    }

    /// Past `MAX_REQUESTS` the oldest is rejected: anyone can write to an address.
    fn make_room(&mut self) -> Result<()> {
        while self.state.address.requests.len() >= MAX_REQUESTS {
            let Some((&oldest, _)) = self.state.address.requests.first_key_value() else {
                break;
            };
            self.forget_request(oldest, true)?;
        }
        Ok(())
    }

    /// Removes a request here, and at the agent when `drop` says it is not answered by a connection.
    pub(super) fn forget_request(&mut self, id: i64, drop: bool) -> Result<()> {
        let Some(r) = self.state.address.requests.remove(&id) else {
            return Ok(());
        };
        self.store.delete(NS, &keys::request(id))?;
        self.state.chats_dirty = true;
        if drop {
            self.drop_request(&r.invitation)?;
        }
        Ok(())
    }
}

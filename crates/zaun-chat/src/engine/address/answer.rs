//! Accept and Reject: accepting joins the requester's invitation with our profile and makes them a friend that turns ready on `CON`; rejecting forgets the request and tells no one (Commands.hs `APIAcceptContact`, `APIRejectContact`).

use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, fail};
use crate::engine::machine::Engine;
use crate::engine::profile::conn_info;
use crate::engine::records::{ConnKind, ConnRecord, FriendRecord, RequestRecord, keys};

/// Every connection this engine makes is post-quantum.
const PQ: bool = true;

impl<A: AgentPort> Engine<A> {
    fn request_of(&self, id: i64) -> Result<RequestRecord> {
        let found = self.state.address.requests.get(&id);
        found
            .cloned()
            .ok_or_else(|| fail(Code::RequestFailed, "no such request"))
    }

    pub(crate) fn accept_request(&mut self, request: i64) -> Result<()> {
        let req = self.request_of(request)?;
        let ours = conn_info(&self.state.user)?;
        let conn = self
            .agent
            .accept_request(&mut self.store, &req.invitation, ours, PQ)?;
        let id = self.store.next_id(keys::FRIEND_SEQ)?;
        let mut f = FriendRecord::new(id, conn, self.clock.wall_ms());
        f.name = req.name;
        f.about = req.about;
        f.image = req.image;
        self.save_friend(f)?;
        self.save_conn(ConnRecord {
            conn,
            kind: ConnKind::Friend { id },
        })?;
        if let (Some(mut a), Some(req_id)) = (self.state.address.record.clone(), req.req_id) {
            a.accept(req_id, id, |f| self.state.friends.contains_key(&f));
            self.save_address(a)?;
        }
        self.forget_request(request, false)?;
        Ok(())
    }

    /// The requester is never told: the request is forgotten here and at the agent.
    pub(crate) fn reject_request(&mut self, request: i64) -> Result<()> {
        self.request_of(request)?;
        self.forget_request(request, true)
    }
}

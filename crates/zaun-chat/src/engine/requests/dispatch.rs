//! The request table: each request to its handler; a handler's error answers `Failed`.

use zaun_chat_wire::{Event, Request};

use super::kind::kind_of;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::{Code, Result, not_yet};
use crate::engine::machine::Engine;

impl<A: AgentPort> Engine<A> {
    pub fn on_request(&mut self, request: Request) {
        let kind = kind_of(&request);
        if let Err(e) = self.route(request) {
            self.emit(Event::Failed {
                request: kind,
                code: e.code.as_str().to_owned(),
                detail: e.detail,
            });
        }
        self.pump();
    }

    fn route(&mut self, request: Request) -> Result<()> {
        match request {
            Request::SetProfile(p) => self.set_profile(&p),
            Request::CreateInvite => self.create_invite(),
            Request::Connect(link) => self.connect(&link),
            Request::RemoveFriend(id) => self.remove_friend(id),
            Request::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => self.send_text(chat, &text, reply_to, &mentions),
            Request::MarkRead(chat) => self.mark_read(chat),
            Request::History {
                chat,
                before,
                count,
                tag,
            } => self.history(chat, before, count, tag),
            Request::Refresh => {
                self.state.chats_dirty = true;
                Ok(())
            }
            Request::SetMuted { chat, on } => self.set_muted(chat, on),
            Request::SetBlocked { friend, on } => self.set_blocked(friend, on),
            Request::SetMemberBlocked { group, member, on } => {
                self.set_member_blocked(group, member, on)
            }
            Request::Call(_) => Err(not_yet(Code::CallFailed)),
            Request::Stream(_) => Err(not_yet(Code::StreamFailed)),
            Request::SetPicture(_)
            | Request::SetGroupPicture { .. }
            | Request::Accept(_)
            | Request::Reject(_)
            | Request::CreateAddress
            | Request::DeleteAddress
            | Request::SendFile { .. }
            | Request::ReceiveFile { .. }
            | Request::CancelFile { .. }
            | Request::OpenFile { .. }
            | Request::LocateFile { .. }
            | Request::CreateGroupLink(_)
            | Request::DeleteGroupLink(_)
            | Request::Edit { .. }
            | Request::Delete { .. }
            | Request::React { .. }
            | Request::ShowCode(_)
            | Request::Verify { .. }
            | Request::Members(_)
            | Request::NewGroup(_)
            | Request::AddMember { .. }
            | Request::JoinGroup(_)
            | Request::LeaveGroup(_) => Err(not_yet(Code::RequestFailed)),
        }
    }
}

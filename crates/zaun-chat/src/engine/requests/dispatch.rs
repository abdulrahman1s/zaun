//! The request table: each request to its handler; a handler's error answers `Failed`.

use zaun_chat_wire::{Event, Request};

use super::kind::kind_of;
use crate::engine::agent_port::AgentPort;
use crate::engine::error::Result;
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
            Request::SetPicture(p) => self.set_picture(p),
            Request::Accept(id) => self.accept_request(id),
            Request::Reject(id) => self.reject_request(id),
            Request::CreateAddress => self.create_address(),
            Request::DeleteAddress => self.delete_address(),
            Request::SendFile {
                chat,
                path,
                kind,
                preview,
                duration,
                text,
            } => self.send_file(chat, &path, kind, preview.as_deref(), duration, &text),
            Request::ReceiveFile { chat, item } => self.receive_file(chat, item),
            Request::CancelFile { chat, item } => self.cancel_file(chat, item),
            Request::OpenFile { chat, item } => self.open_file(chat, item),
            Request::LocateFile { chat, item, tag } => self.locate_file(chat, item, tag),
            Request::ShowCode(friend) => self.show_code(friend),
            Request::Verify { friend, code } => self.verify(friend, &code),
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
            Request::Edit { chat, item, text } => self.edit(chat, item, &text),
            Request::Delete { chat, item } => self.delete(chat, item),
            Request::React {
                chat,
                item,
                emoji,
                on,
            } => self.react(chat, item, &emoji, on),
            Request::Call(r) => self.call(r),
            Request::Stream(r) => self.stream(r),
            Request::SetGroupPicture { group, picture } => self.set_group_picture(group, picture),
            Request::CreateGroupLink(group) => self.create_group_link(group),
            Request::DeleteGroupLink(group) => self.delete_group_link(group),
            Request::Members(group) => self.members(group),
            Request::NewGroup(name) => self.new_group(&name),
            Request::AddMember { group, friend } => self.add_member(group, friend),
            Request::JoinGroup(group) => self.join_group(group),
            Request::LeaveGroup(group) => self.leave_group(group),
        }
    }
}

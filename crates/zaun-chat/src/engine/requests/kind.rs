//! Which request a failure answers (the kinds core's codec gives each request).

use zaun_chat_wire::{Request, RequestKind};

#[must_use]
pub fn kind_of(r: &Request) -> RequestKind {
    use RequestKind as K;
    match r {
        Request::SetProfile(_) | Request::SetPicture(_) => K::SetProfile,
        Request::CreateInvite => K::CreateInvite,
        Request::Connect(_) => K::Connect,
        Request::Accept(_) | Request::Reject(_) => K::Answer,
        Request::CreateAddress | Request::DeleteAddress => K::Address,
        Request::Send { .. } => K::Send,
        Request::SendFile { .. } => K::SendFile,
        Request::ReceiveFile { .. }
        | Request::CancelFile { .. }
        | Request::OpenFile { .. }
        | Request::LocateFile { .. } => K::File,
        Request::CreateGroupLink(_) | Request::DeleteGroupLink(_) => K::GroupLink,
        Request::Edit { .. } => K::Edit,
        Request::Delete { .. } => K::Delete,
        Request::React { .. } => K::React,
        Request::SetMuted { .. } => K::Mute,
        Request::SetBlocked { .. } | Request::SetMemberBlocked { .. } => K::Block,
        Request::ShowCode(_) | Request::Verify { .. } => K::Code,
        Request::Members(_) => K::Members,
        Request::MarkRead(_) => K::MarkRead,
        Request::History { .. } => K::History,
        Request::Refresh => K::Refresh,
        Request::SetGroupPicture { .. }
        | Request::NewGroup(_)
        | Request::AddMember { .. }
        | Request::JoinGroup(_)
        | Request::LeaveGroup(_) => K::Group,
        Request::RemoveFriend(_) => K::RemoveFriend,
        Request::Call(_) | Request::Stream(_) => K::Call,
    }
}

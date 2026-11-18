//! The Chat page's commands, each handed to the file that does it (ADR-0033).
use crate::chat::Request;
use crate::service::prelude::*;

impl AppService {
    pub(crate) fn chat_command(&mut self, c: ChatCommand) -> Result<()> {
        match c {
            ChatCommand::SetEnabled { on, name } => self.chat_set_enabled(on, name),
            ChatCommand::SetName(name) => self.chat_set_name(&name),
            ChatCommand::SetProfile(p) => {
                if !p.name.trim().is_empty() {
                    self.chat_set_profile(p);
                }
            }
            ChatCommand::SetPicture { path, crop } => {
                return self.chat_set_picture(None, path, crop);
            }
            ChatCommand::ClearPicture => return self.chat_clear_picture(None),
            ChatCommand::LoadPicture { path, reply_to } => self.chat_load_picture(path, reply_to),
            ChatCommand::SetGroupPicture { group, path, crop } => {
                return self.chat_set_picture(Some(group), path, crop);
            }
            ChatCommand::ClearGroupPicture(g) => return self.chat_clear_picture(Some(g)),
            ChatCommand::CreateInvite => self.chat_request(Request::CreateInvite),
            ChatCommand::Connect(link) => self.chat_request(Request::Connect(link)),
            ChatCommand::Accept(id) => self.chat_request(Request::Accept(id)),
            ChatCommand::Reject(id) => self.chat_request(Request::Reject(id)),
            ChatCommand::CreateAddress => self.chat_request(Request::CreateAddress),
            ChatCommand::DeleteAddress => self.chat_request(Request::DeleteAddress),
            ChatCommand::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => self.chat_send(chat, text, reply_to, mentions),
            ChatCommand::SendFile { chat, path, text } => {
                return self.chat_send_path(chat, path, text);
            }
            ChatCommand::SendClipboard {
                chat,
                bytes,
                kind,
                text,
            } => return self.chat_send_clipboard(chat, bytes, kind, text),
            ChatCommand::ReceiveFile { chat, item } => {
                self.chat_request(Request::ReceiveFile { chat, item });
            }
            ChatCommand::CancelFile { chat, item } => {
                self.chat_request(Request::CancelFile { chat, item });
            }
            ChatCommand::OpenFile { chat, item } => {
                self.chat_request(Request::OpenFile { chat, item });
            }
            ChatCommand::SaveFile { chat, item, to } => {
                return self.chat_save_command(chat, item, to);
            }
            ChatCommand::CreateGroupLink(g) => self.chat_request(Request::CreateGroupLink(g)),
            ChatCommand::DeleteGroupLink(g) => self.chat_request(Request::DeleteGroupLink(g)),
            ChatCommand::Edit { chat, item, text } => self.chat_edit(chat, item, text),
            ChatCommand::Delete { chat, item } => self.chat_request(Request::Delete { chat, item }),
            ChatCommand::React {
                chat,
                item,
                emoji,
                on,
            } => self.chat_request(Request::React {
                chat,
                item,
                emoji,
                on,
            }),
            ChatCommand::SetMuted { chat, on } => self.chat_set_muted(chat, on),
            ChatCommand::SetBlocked { friend, on } => {
                self.chat_set_blocked(friend, on);
                if on {
                    self.call_drop_friend(friend);
                    self.stream_drop_friend(friend);
                }
            }
            ChatCommand::SetMemberBlocked { group, member, on } => {
                self.chat_set_member_blocked(group, member, on);
            }
            ChatCommand::ShowCode(friend) => self.chat_show_code(friend),
            ChatCommand::Verify { friend, code } => {
                self.chat_request(Request::Verify { friend, code });
            }
            ChatCommand::MarkRead(chat) => self.chat_mark_read(chat),
            ChatCommand::LoadChat {
                chat,
                before,
                reply_to,
            } => self.chat_load(chat, before, reply_to),
            ChatCommand::NewGroup(name) => self.chat_new_group(&name),
            ChatCommand::AddMember { group, friend } => {
                self.chat_request(Request::AddMember { group, friend });
            }
            ChatCommand::JoinGroup(g) => self.chat_request(Request::JoinGroup(g)),
            ChatCommand::LeaveGroup(g) => self.chat_request(Request::LeaveGroup(g)),
            ChatCommand::RemoveFriend(c) => {
                self.call_drop_friend(c);
                self.stream_drop_friend(c);
                self.chat_request(Request::RemoveFriend(c));
            }
            ChatCommand::OpenLink(url) => return self.open_chat_link(&url, true),
            ChatCommand::OpenLinkAnyway(url) => return self.open_chat_link(&url, false),
            ChatCommand::GifSearch { query, reply_to } => self.gif_search(&query, reply_to),
            ChatCommand::GifTrending { reply_to } => self.gif_trending(reply_to),
            ChatCommand::GifPreview { url, reply_to } => self.gif_preview(url, reply_to),
            ChatCommand::SendGif { chat, gif } => return self.send_gif(chat, gif),
            ChatCommand::Call(chat) => return self.call_place(chat),
            ChatCommand::Answer => return self.call_answer(),
            ChatCommand::Decline => self.call_decline(),
            ChatCommand::EndCall => self.call_hang_up(),
            ChatCommand::CallMute(on) => self.call_mute(on),
            ChatCommand::CallDeafen(on) => self.call_deafen(on),
            ChatCommand::MicTest(on) => return self.mic_test(on),
            ChatCommand::RefreshAudioDevices => self.audio_devices_refresh(),
            ChatCommand::StartShare(chat) => return self.share_start(chat),
            ChatCommand::StopShare => self.share_stop(),
            ChatCommand::Watch(chat) => self.watch_start(chat),
            ChatCommand::StopWatch => self.watch_stop(),
            ChatCommand::WatchMute(on) => self.watch_mute(on),
            ChatCommand::SetShareQuality { height, fps } => {
                return self.share_set_quality(height, fps);
            }
            // Status and voice messages have no engine yet: `ChatSnapshot::features` keeps
            // their controls off the page.
            ChatCommand::SendVoice { .. } | ChatCommand::SetStatus(_) => {
                tracing::debug!("chat command not carried by this engine yet");
            }
        }
        Ok(())
    }
}

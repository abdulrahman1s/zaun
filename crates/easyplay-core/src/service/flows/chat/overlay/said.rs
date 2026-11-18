//! What the user did in the overlay becomes the Chat commands the page would send
//! (ADR-0037 §3): a message, another conversation, the call's buttons, the answer bar,
//! closing.
use crate::chat::overlay::FromOverlay;
use crate::service::prelude::*;

impl AppService {
    pub(super) fn on_overlay_said(&mut self, m: FromOverlay) -> Result<()> {
        match m {
            // Handled with the generation check in `done`.
            FromOverlay::Ready => Ok(()),
            FromOverlay::SendMessage { chat, text } => {
                let chat = ChatRef::from(chat);
                // Only conversations the overlay was shown.
                if !self.overlay_knows(chat) {
                    tracing::info!(
                        ?chat,
                        "overlay: a message for a conversation it was not shown"
                    );
                    return Ok(());
                }
                self.chat_command(ChatCommand::Send {
                    chat,
                    text,
                    reply_to: None,
                    mentions: Vec::new(),
                })
            }
            FromOverlay::OpenConversation { chat } => {
                let chat = ChatRef::from(chat);
                if self.overlay_knows(chat) {
                    self.overlay_open_chat(chat);
                    self.overlay_push();
                }
                Ok(())
            }
            FromOverlay::CallMute { on } => self.chat_command(ChatCommand::CallMute(on)),
            FromOverlay::CallDeafen { on } => self.chat_command(ChatCommand::CallDeafen(on)),
            FromOverlay::EndCall => self.chat_command(ChatCommand::EndCall),
            FromOverlay::Answer => {
                // Answering opens that conversation in the panel (the call starts in
                // "Connecting…" there).
                if let Some(chat) = self
                    .flow
                    .chat
                    .snapshot
                    .live
                    .ringing
                    .as_ref()
                    .map(|r| r.chat)
                {
                    self.overlay_open_chat(chat);
                }
                self.chat_command(ChatCommand::Answer)
            }
            FromOverlay::Decline => self.chat_command(ChatCommand::Decline),
            FromOverlay::Share { on: true } => {
                let Some(chat) = self.flow.chat.snapshot.live.call.as_ref().map(|c| c.chat) else {
                    return Ok(());
                };
                self.chat_command(ChatCommand::StartShare(chat))
            }
            FromOverlay::Share { on: false } => self.chat_command(ChatCommand::StopShare),
            FromOverlay::SetLook {
                toasts,
                voice_widget,
                corner,
            } => self.overlay_set_look(toasts, voice_widget, corner),
            FromOverlay::Closed => {
                self.flow.overlay.open = false;
                Ok(())
            }
        }
    }

    fn overlay_knows(&self, chat: ChatRef) -> bool {
        self.flow.chat.snapshot.chats.iter().any(|p| p.chat == chat)
    }
}

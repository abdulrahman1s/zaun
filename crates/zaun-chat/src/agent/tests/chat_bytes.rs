//! The chat-layer bytes the interop tests send and read: `x.info` for the handshake, `x.msg.new` for text.
use crate::chat::container::MsgContainer;
use crate::chat::content::MsgContent;
use crate::chat::profile::Profile;
use crate::chat::{ChatEvent, ChatMessage, encode_chat_message, parse_chat_messages};

pub fn x_info(name: &str) -> Vec<u8> {
    let ev = ChatEvent::Info {
        profile: Profile::new(name, ""),
        member_key: None,
    };
    encode_chat_message(&ChatMessage::without_id(ev), false).unwrap()
}

pub fn x_msg(text: &str) -> Vec<u8> {
    let c = MsgContainer::simple(MsgContent::Text { text: text.into() });
    let m = ChatMessage::new(ChatEvent::MsgNew(c)).unwrap();
    encode_chat_message(&m, false).unwrap()
}

pub fn text_of(body: &[u8]) -> Option<String> {
    parse_chat_messages(body)
        .ok()?
        .into_iter()
        .find_map(|m| match m.event {
            ChatEvent::MsgNew(MsgContainer {
                content: MsgContent::Text { text },
                ..
            }) => Some(text),
            _ => None,
        })
}

pub fn profile_name(conn_info: &[u8]) -> Option<String> {
    parse_chat_messages(conn_info)
        .ok()?
        .into_iter()
        .find_map(|m| match m.event {
            ChatEvent::Info { profile, .. } => Some(profile.display_name),
            _ => None,
        })
}

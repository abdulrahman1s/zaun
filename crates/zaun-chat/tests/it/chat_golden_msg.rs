//! Golden JSON of x.msg.*, x.file.* and x.info from tests/ProtocolTests.hs, as strings given there.
use super::chat_support::*;
use zaun_chat::chat::ChatEvent as E;
use zaun_chat::chat::container::ForwardLink;
use zaun_chat::chat::container::{MsgContainer, MsgRef, QuotedMsg};
use zaun_chat::chat::content::{MsgContent, ReportReason};
use zaun_chat::chat::file::FileInvitation;
use zaun_chat::chat::ids::MemberId;
use zaun_chat::chat::image::ImageData;
use zaun_chat::chat::link_preview::LinkPreview;
use zaun_chat::chat::profile::{ChatPeerType, Profile};
use zaun_chat::chat::version::SUPPORTED_CHAT_VRANGE;

fn text(t: &str) -> MsgContent {
    MsgContent::Text { text: t.into() }
}

fn quoted() -> QuotedMsg {
    let msg_ref = MsgRef {
        msg_id: Some(id(&[5, 6, 7, 8])),
        sent_at: "1970-01-01T00:00:01.000000001Z".into(),
        sent: true,
        member_id: None,
    };
    QuotedMsg {
        msg_ref,
        content: text("hello there!"),
    }
}

fn plain_file(name: &str) -> FileInvitation {
    FileInvitation {
        file_name: name.into(),
        file_size: 12345,
        file_digest: None,
        file_conn_req: None,
        file_inline: None,
        file_descr: None,
        file_badge: None,
    }
}

fn new_msg(c: MsgContainer) -> zaun_chat::chat::ChatEvent {
    E::MsgNew(c)
}

const QUOTE: &str = r#""quote":{"content":{"text":"hello there!","type":"text"},"msgRef":{"msgId":"BQYHCA==","sent":true,"sentAt":"1970-01-01T00:00:01.000000001Z"}}"#;

#[test]
fn x_msg_new_simple_text_round_trips() {
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"}}}"#;
    both(s, &msg(new_msg(MsgContainer::text("hello"))));
}

#[test]
fn x_msg_new_ttl_and_live_are_kept() {
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"ttl":3600}}"#;
    both(
        s,
        &msg(new_msg(MsgContainer {
            ttl: Some(3600),
            ..MsgContainer::text("hello")
        })),
    );
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"live":true}}"#;
    both(
        s,
        &msg(new_msg(MsgContainer {
            live: Some(true),
            ..MsgContainer::text("hello")
        })),
    );
}

#[test]
fn x_msg_new_link_preview_round_trips() {
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"https://simplex.chat","type":"link","preview":{"description":"SimpleX Chat","image":"data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgA","title":"SimpleX Chat","uri":"https://simplex.chat"}}}}"#;
    let preview = LinkPreview {
        uri: "https://simplex.chat".into(),
        title: "SimpleX Chat".into(),
        description: "SimpleX Chat".into(),
        image: ImageData("data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAgA".into()),
        content: None,
    };
    both(
        s,
        &msg(new_msg(MsgContainer::simple(MsgContent::Link {
            text: "https://simplex.chat".into(),
            preview,
        }))),
    );
}

#[test]
fn x_msg_new_image_with_and_without_text() {
    for caption in ["", "here's an image"] {
        let s = format!(
            r#"{{"v":"9","event":"x.msg.new","params":{{"content":{{"text":"{caption}","type":"image","image":"{IMG}"}}}}}}"#
        );
        let c = MsgContent::Image {
            text: caption.into(),
            image: ImageData(IMG.into()),
        };
        both(&s, &msg(new_msg(MsgContainer::simple(c))));
    }
}

#[test]
fn x_msg_new_message_with_id_and_version_range() {
    let s = r#"{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"}}}"#;
    both(
        s,
        &msg_with_id(&[1, 2, 3, 4], new_msg(MsgContainer::text("hello"))),
    );
    let s = r#"{"v":"9-22","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"}}}"#;
    let mut m = msg_with_id(&[1, 2, 3, 4], new_msg(MsgContainer::text("hello")));
    m.v = Some(SUPPORTED_CHAT_VRANGE);
    both(s, &m);
}

#[test]
fn x_msg_new_quote_with_ttl_and_live() {
    let body = r#""content":{"text":"hello to you too","type":"text"}"#;
    let mk = |c: MsgContainer| msg_with_id(&[1, 2, 3, 4], new_msg(c));
    let base = MsgContainer::quote(quoted(), text("hello to you too"));
    both(
        &format!(
            r#"{{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{{{body},{QUOTE}}}}}"#
        ),
        &mk(base.clone()),
    );
    let ttl = MsgContainer {
        ttl: Some(3600),
        ..base.clone()
    };
    both(
        &format!(
            r#"{{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{{{body},{QUOTE},"ttl":3600}}}}"#
        ),
        &mk(ttl),
    );
    let live = MsgContainer {
        live: Some(true),
        ..base
    };
    both(
        &format!(
            r#"{{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{{{body},{QUOTE},"live":true}}}}"#
        ),
        &mk(live),
    );
}

#[test]
fn x_msg_new_forward_with_and_without_channel_link() {
    let mk = |c: MsgContainer| msg_with_id(&[1, 2, 3, 4], new_msg(c));
    let s = r#"{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"forward":true}}"#;
    both(s, &mk(MsgContainer::forward(None, text("hello"))));
    let s = r#"{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"forward":true,"ttl":3600}}"#;
    both(
        s,
        &mk(MsgContainer {
            ttl: Some(3600),
            ..MsgContainer::forward(None, text("hello"))
        }),
    );
    let link = ForwardLink {
        display_name: "team".into(),
        group_link: "simplex:/c#AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg?h=smp.simplex.im&p=5223&c=1234-w".into(),
        public_group_id: "AQIDBA==".into(),
        member_id: Some(MemberId(vec![1, 2, 3, 4])),
        msg_id: id(&[5, 6, 7, 8]),
    };
    let s = r#"{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"forward":true,"forwardLink":{"displayName":"team","groupLink":"simplex:/c#AQIDBAUGBwgBAgMEBQYHCAECAwQFBgcIAQIDBAUGBwg?h=smp.simplex.im&p=5223&c=1234-w","publicGroupId":"AQIDBA==","memberId":"AQIDBA==","msgId":"BQYHCA=="}}}"#;
    both(
        s,
        &mk(MsgContainer::forward(Some(link.clone()), text("hello"))),
    );
    let s = s.replace(r#""memberId":"AQIDBA==","#, "");
    let no_author = ForwardLink {
        member_id: None,
        ..link
    };
    both(
        &s,
        &mk(MsgContainer::forward(Some(no_author), text("hello"))),
    );
}

#[test]
fn x_msg_new_with_file_invitation() {
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"file":{"fileSize":12345,"fileName":"photo.jpg"}}}"#;
    both(
        s,
        &msg(new_msg(MsgContainer {
            file: Some(plain_file("photo.jpg")),
            ..MsgContainer::text("hello")
        })),
    );
    let s = r#"{"v":"9","event":"x.msg.new","params":{"content":{"text":"","type":"file"},"file":{"fileSize":12345,"fileName":"file.txt"}}}"#;
    let c = MsgContainer {
        file: Some(plain_file("file.txt")),
        ..MsgContainer::simple(MsgContent::File { text: "".into() })
    };
    both(s, &msg(new_msg(c)));
    let s = r#"{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{"content":{"text":"hello","type":"text"},"file":{"fileSize":12345,"fileName":"photo.jpg"},"forward":true}}"#;
    let c = MsgContainer {
        file: Some(plain_file("photo.jpg")),
        ..MsgContainer::forward(None, text("hello"))
    };
    both(s, &msg_with_id(&[1, 2, 3, 4], new_msg(c)));
}

#[test]
fn x_msg_new_report_quotes_the_reported_message() {
    let s = format!(
        r#"{{"v":"9","msgId":"AQIDBA==","event":"x.msg.new","params":{{"content":{{"text":"","reason":"spam","type":"report"}},{QUOTE}}}}}"#
    );
    let report = MsgContent::Report {
        text: "".into(),
        reason: ReportReason::Spam,
    };
    both(
        &s,
        &msg_with_id(
            &[1, 2, 3, 4],
            new_msg(MsgContainer::quote(quoted(), report)),
        ),
    );
}

#[test]
fn x_msg_update_del_deleted() {
    let s = r#"{"v":"9","event":"x.msg.update","params":{"msgId":"AQIDBA==", "content":{"text":"hello","type":"text"}}}"#;
    let ev = E::MsgUpdate {
        msg_id: id(&[1, 2, 3, 4]),
        content: text("hello"),
        mentions: Default::default(),
        ttl: None,
        live: None,
        scope: None,
        as_group: None,
    };
    both(s, &msg(ev));
    let s = r#"{"v":"9","event":"x.msg.del","params":{"msgId":"AQIDBA=="}}"#;
    both(
        s,
        &msg(E::MsgDel {
            msg_id: id(&[1, 2, 3, 4]),
            member_id: None,
            scope: None,
            only_history: false,
        }),
    );
    both(
        r#"{"v":"9","event":"x.msg.deleted","params":{}}"#,
        &msg(E::MsgDeleted),
    );
}

#[test]
fn x_file_events() {
    let s = format!(
        r#"{{"v":"9","event":"x.file","params":{{"file":{{"fileConnReq":"{CONN_REQ}","fileSize":12345,"fileName":"photo.jpg"}}}}}}"#
    );
    let f = FileInvitation {
        file_conn_req: Some(CONN_REQ.into()),
        ..plain_file("photo.jpg")
    };
    both(&s, &msg(E::File(f)));
    let s =
        r#"{"v":"9","event":"x.file","params":{"file":{"fileSize":12345,"fileName":"photo.jpg"}}}"#;
    both(s, &msg(E::File(plain_file("photo.jpg"))));
    let s = r#"{"v":"9","event":"x.file.acpt","params":{"fileName":"photo.jpg"}}"#;
    both(
        s,
        &msg(E::FileAcpt {
            file_name: "photo.jpg".into(),
        }),
    );
    let s = format!(
        r#"{{"v":"9","event":"x.file.acpt.inv","params":{{"msgId":"AQIDBA==","fileName":"photo.jpg","fileConnReq":"{CONN_REQ}"}}}}"#
    );
    both(
        &s,
        &msg(E::FileAcptInv {
            msg_id: id(&[1, 2, 3, 4]),
            file_conn_req: Some(CONN_REQ.into()),
            file_name: "photo.jpg".into(),
        }),
    );
    let s = r#"{"v":"9","event":"x.file.acpt.inv","params":{"msgId":"AQIDBA==","fileName":"photo.jpg"}}"#;
    both(
        s,
        &msg(E::FileAcptInv {
            msg_id: id(&[1, 2, 3, 4]),
            file_conn_req: None,
            file_name: "photo.jpg".into(),
        }),
    );
    let s = r#"{"v":"9","event":"x.file.cancel","params":{"msgId":"AQIDBA=="}}"#;
    both(
        s,
        &msg(E::FileCancel {
            msg_id: id(&[1, 2, 3, 4]),
        }),
    );
}

#[test]
fn x_info_carries_the_profile() {
    let s = format!(
        r#"{{"v":"9","event":"x.info","params":{{"profile":{}}}}}"#,
        test_profile_json()
    );
    both(
        &s,
        &msg(E::Info {
            profile: test_profile(),
            member_key: None,
        }),
    );
    let bare = Profile {
        image: None,
        ..test_profile()
    };
    let s = r#"{"v":"9","event":"x.info","params":{"profile":{"fullName":"","displayName":"alice","preferences":{"reactions":{"allow":"yes"},"voice":{"allow":"yes"}}}}}"#;
    both(
        s,
        &msg(E::Info {
            profile: Profile {
                full_name: "".into(),
                ..bare.clone()
            },
            member_key: None,
        }),
    );
    let s = r#"{"v":"9","event":"x.info","params":{"profile":{"fullName":"","displayName":"alice","peerType":"agent","preferences":{"reactions":{"allow":"yes"},"voice":{"allow":"yes"}}}}}"#;
    let agent = Profile {
        full_name: "".into(),
        peer_type: Some(ChatPeerType::Agent),
        ..bare
    };
    both(
        s,
        &msg(E::Info {
            profile: agent,
            member_key: None,
        }),
    );
}

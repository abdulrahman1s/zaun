//! The overlay design's scenes (`chat-calls.html` §G: Lantern Keep, Friday Crew in a call),
//! sent to the real overlay by the ignored real-compositor test so its screenshots can be
//! held against the design renders kept by the maintainer.

#![allow(
    clippy::unwrap_used,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

use base64::Engine as _;
use zaun_core::chat::overlay::ChatKey;
use zaun_core::chat::overlay::notice::Notice;
use zaun_core::chat::overlay::snapshot::{
    BodyKind, CallLine, CallPerson, Conversation, Corner, Link, OverlayCall, OverlayChat,
    OverlayLook, OverlayMessage, OverlayRing, OverlaySnapshot, Person, Status, Toast, WirePicture,
};
use zaun_core::clock::{Clock, SystemClock};

#[derive(Clone, Copy)]
pub enum Scene {
    /// A call (Mira speaking, Dev muted) and two message toasts, the overlay closed.
    Closed,
    /// The panel open on Friday Crew, in the call.
    Open,
    /// Dev is calling (a call toast closed, the answer bar open).
    Ringing,
    /// The open panel in Arabic.
    Arabic,
    /// Closed, high contrast, the widget bottom right (toasts top left), no key bound.
    Contrast,
}

const PEOPLE: [(&str, u8, [u8; 3]); 7] = [
    ("Mira Okafor", 2, [196, 140, 92]),
    ("Dev Patel", 5, [176, 92, 108]),
    ("Jonas", 3, [92, 156, 116]),
    ("Kit", 7, [92, 140, 196]),
    ("Aiko Tanaka", 1, [0, 0, 0]),
    ("Tomasz Nowak", 8, [0, 0, 0]),
    ("Lucía Fernández", 4, [0, 0, 0]),
];

fn person(name: &str) -> Person {
    let (full, paper, colour) = PEOPLE
        .iter()
        .find(|(n, _, _)| n.starts_with(name))
        .copied()
        .unwrap_or((name, 0, [0, 0, 0]));
    Person {
        name: if name.contains(' ') {
            full.into()
        } else {
            name.into()
        },
        paper: Some(paper),
        picture: (colour != [0, 0, 0]).then(|| key(full)),
    }
}

fn key(name: &str) -> u64 {
    name.bytes().fold(1469598103934665603u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(1099511628211)
    })
}

/// A stand-in photo: a portrait silhouette on the person's colour (PNG).
fn photo(colour: [u8; 3]) -> String {
    let img = image::RgbImage::from_fn(96, 96, |x, y| {
        let (dx, dy) = (x as f32 - 48.0, y as f32 - 40.0);
        let head = dx * dx + dy * dy < 300.0;
        let body = y > 66 && (x as f32 - 48.0).abs() < 34.0 - (96.0 - y as f32) * 0.4;
        if head || body {
            image::Rgb([235, 196, 150])
        } else {
            let t = y as f32 / 96.0;
            image::Rgb(colour.map(|c| (f32::from(c) * (1.0 - 0.3 * t)) as u8))
        }
    });
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(png.into_inner())
}

fn msg(id: i64, who: &str, me: bool, mins_ago: i64, text: &str) -> OverlayMessage {
    let now = SystemClock::new().now().as_millis();
    OverlayMessage {
        id,
        author: if me { person("Sam") } else { person(who) },
        me,
        kind: BodyKind::Text,
        text: text.into(),
        notice: None,
        call: None,
        at: now - mins_ago * 60_000,
        unread: false,
        mentions: if text.contains("@Sam") {
            vec!["Sam".into()]
        } else {
            Vec::new()
        },
    }
}

fn notice(id: i64, mins_ago: i64, n: Notice) -> OverlayMessage {
    OverlayMessage {
        kind: BodyKind::Notice,
        notice: Some(n),
        text: String::new(),
        ..msg(id, "Kit", false, mins_ago, "")
    }
}

fn chat(
    chat: ChatKey,
    who: &str,
    status: Option<Status>,
    unread: u32,
    last: OverlayMessage,
) -> OverlayChat {
    let mut who = person(who);
    if chat.is_group() {
        who = Person {
            name: "Friday Crew".into(),
            paper: Some(6),
            picture: None,
        };
    }
    OverlayChat {
        chat,
        who,
        status,
        unread,
        muted: false,
        last: Some(last),
    }
}

pub fn scene(s: Scene) -> OverlaySnapshot {
    let crew = ChatKey::Group(1);
    let mut transcript = vec![
        notice(1, 60 * 21, Notice::MemberJoined { who: "Kit".into() }),
        msg(
            2,
            "Kit",
            false,
            60 * 21 - 1,
            "hey all, thanks for the invite",
        ),
        msg(3, "Kit", false, 60 * 21 - 1, "what are we playing?"),
        notice(
            4,
            60 * 20,
            Notice::MemberAdded {
                by: Some("Mira".into()),
                who: "Lucía".into(),
            },
        ),
        notice(
            5,
            60 * 20 - 1,
            Notice::GroupNameChanged {
                name: "Friday Crew".into(),
            },
        ),
        OverlayMessage {
            kind: BodyKind::Call,
            call: Some(CallLine::Ended { minutes: 38 }),
            ..msg(6, "Dev", false, 19 * 60, "")
        },
        msg(
            7,
            "Dev",
            false,
            50,
            "for anyone on Linux, this launch option fixed the stutter for me",
        ),
        msg(
            8,
            "Jonas",
            false,
            21,
            "going live. ranked until I hit gold or cry",
        ),
        msg(
            9,
            "Mira",
            false,
            13,
            "watching. your aim is suspiciously good today",
        ),
        msg(
            10,
            "Dev",
            false,
            10,
            "@Sam you joining voice? we need a third",
        ),
    ];
    for m in &mut transcript[8..] {
        m.unread = true;
    }
    let last = transcript
        .last()
        .cloned()
        .unwrap_or_else(|| msg(0, "Dev", false, 0, ""));
    let chats = vec![
        chat(crew, "Friday Crew", None, 0, last.clone()),
        chat(
            ChatKey::Direct(2),
            "Mira Okafor",
            Some(Status::Online),
            0,
            msg(
                11,
                "Mira",
                false,
                5,
                "I'm in Lantern Keep doing the bell puzzle",
            ),
        ),
        chat(
            ChatKey::Direct(3),
            "Dev Patel",
            Some(Status::Online),
            1,
            msg(12, "Dev", false, 56, "you up for ranked later?"),
        ),
        chat(
            ChatKey::Direct(4),
            "Aiko Tanaka",
            Some(Status::Away),
            2,
            msg(
                13,
                "Aiko",
                false,
                60 * 26,
                "tried calling about the shader cache",
            ),
        ),
        chat(
            ChatKey::Direct(5),
            "Tomasz Nowak",
            Some(Status::Offline),
            0,
            msg(14, "", true, 60 * 50, "sent it over, check your downloads"),
        ),
        chat(
            ChatKey::Direct(6),
            "Lucía Fernández",
            Some(Status::Busy),
            0,
            OverlayMessage {
                kind: BodyKind::File,
                ..msg(15, "", true, 60 * 90, "save_slot2.dat")
            },
        ),
    ];
    let call = OverlayCall {
        chat: crew,
        name: "Friday Crew".into(),
        since: Some(SystemClock::new().now().as_millis() - 762_000),
        link: Link::Connected,
        people: [
            ("Mira", true, false),
            ("Jonas", false, false),
            ("Dev", false, true),
        ]
        .iter()
        .map(|(n, speaking, muted)| CallPerson {
            who: person(n),
            me: false,
            speaking: *speaking,
            muted: *muted,
            level: if *speaking { 70 } else { 0 },
        })
        .chain([CallPerson {
            who: person("Sam"),
            me: true,
            speaking: false,
            muted: false,
            level: 0,
        }])
        .collect(),
        more: 0,
        muted: false,
        deafened: false,
        sharing: false,
        can_share: true,
    };
    let look = OverlayLook {
        toasts: true,
        voice_widget: true,
        corner: Corner::TopLeft,
        key: "Shift+F1".into(),
        language: if matches!(s, Scene::Arabic) {
            "ar"
        } else {
            "en"
        }
        .into(),
        locale: "en_GB.UTF-8".into(),
        accent: "tide".into(),
        high_contrast: false,
        reduced_motion: false,
    };
    let pictures = PEOPLE
        .iter()
        .filter(|(_, _, c)| *c != [0, 0, 0])
        .map(|(n, _, c)| WirePicture {
            key: key(n),
            data: photo(*c),
        })
        .collect();
    let crew_face = Person {
        name: "Friday Crew".into(),
        paper: Some(6),
        picture: None,
    };
    let mut snap = OverlaySnapshot {
        me: Some(Person {
            name: "Sam".into(),
            paper: Some(9),
            picture: None,
        }),
        game: Some("Lantern Keep".into()),
        chats,
        open: Some(Conversation {
            chat: crew,
            who: crew_face,
            status: None,
            members: Some((6, 5)),
            messages: transcript,
        }),
        call: Some(call),
        ringing: None,
        toasts: vec![
            Toast {
                id: 1,
                chat: ChatKey::Direct(7),
                who: person("Jonas"),
                group: None,
                kind: BodyKind::Text,
                text: "gg. one more and then I really sleep".into(),
            },
            Toast {
                id: 2,
                chat: crew,
                who: person("Dev"),
                group: Some("Friday Crew".into()),
                kind: BodyKind::Text,
                text: "@Sam you joining voice? we need a third".into(),
            },
        ],
        look,
        pictures,
    };
    if matches!(s, Scene::Contrast) {
        snap.look.high_contrast = true;
        snap.look.corner = Corner::BottomRight;
        snap.look.key = String::new();
        // New toasts: the overlay shows each ID once.
        for t in &mut snap.toasts {
            t.id += 10;
        }
    }
    if matches!(s, Scene::Ringing) {
        snap.call = None;
        snap.toasts.clear();
        snap.ringing = Some(OverlayRing {
            chat: ChatKey::Direct(3),
            who: person("Dev Patel"),
            since: SystemClock::new().now().as_millis(),
        });
        snap.open = Some(Conversation {
            chat: ChatKey::Direct(3),
            who: person("Dev Patel"),
            status: Some(Status::Online),
            members: None,
            messages: Vec::new(),
        });
    }
    snap
}

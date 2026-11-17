//! Chat for the preview backend and the tests (ADR-0033): chat.html's `demoChat()`, ported.
//! Sam's friends Mira, Jonas, Dev, Aiko, Lucía, Tomasz and Noor (and Rex, blocked), a
//! request from Kit, the groups "Friday Crew" and "Mod makers", and every conversation's
//! messages (in SimpleX Chat's own formatting: `*bold*`, `_italic_`, `#secret#`, inline
//! `` `code` ``), dated relative to the demo's `now`. [`DemoChat`] plays every `ChatCommand` the
//! way the service would, and its timers stand in for friends answering, talking and tuning
//! in (chat.html section 25). Time only comes from the caller (`handle(cmd, now)`,
//! [`DemoChat::tick`]), so everything here is deterministic.
//!
//! Names and messages are invented; like the rest of `demo`, they stand in for data the
//! service supplies.

use std::collections::HashMap;
use std::sync::Arc;

use easyplay_core::clock::Timestamp;
use easyplay_core::ids::QueryId;
use easyplay_core::model::{
    Call, CallKind, CallRecord, ChatFeatures, ChatGroup, ChatMessage, ChatPreview, ChatProfile,
    ChatRef, ChatSnapshot, ChatStatus, Delivery, Friend, FriendRequest, GroupMember, Membership,
    Mention, MessageAuthor, MessageBody, MyStatus, Playing, Presence, Quote, Reaction, Ringing,
    Share, StreamRecord, Watching,
};
use easyplay_core::model::{FileState, Media};
use easyplay_core::service::{ChatCommand, ChatPage};

use crate::bridge::{QueryPayload, UiEvent};

const MIN: i64 = 60_000;
const DAY: i64 = 24 * 60 * MIN;

/// Mira Okafor (playing Lantern Keep, verified).
pub const MIRA: ChatRef = ChatRef::Direct(1);
/// Jonas Weber (playing Ninefold Arena, sharing it live).
pub const JONAS: ChatRef = ChatRef::Direct(2);
/// Dev Patel (verified).
pub const DEV: ChatRef = ChatRef::Direct(3);
/// Aiko Tanaka (away).
pub const AIKO: ChatRef = ChatRef::Direct(4);
/// Lucía Fernández (do not disturb).
pub const LUCIA: ChatRef = ChatRef::Direct(5);
/// Tomasz Nowak (offline; Sam's last message waits for him).
pub const TOMASZ: ChatRef = ChatRef::Direct(6);
/// Noor Haddad (offline).
pub const NOOR: ChatRef = ChatRef::Direct(7);
/// Rex Calloway (blocked: his conversation is not listed).
pub const REX: ChatRef = ChatRef::Direct(8);
/// "Friday Crew": Mira, Jonas (owner), Dev, Lucía and Sam; Jonas and Dev in voice.
pub const CREW: ChatRef = ChatRef::Group(10);
/// "Mod makers": Aiko, Tomasz, Noor and Sam (owner); muted.
pub const MODS: ChatRef = ChatRef::Group(11);
/// Kit Brennan's request.
pub const KIT_REQUEST: i64 = 20;
/// Kit's contact number once the request is accepted.
pub const KIT: i64 = 9;
/// The link `CreateInvite` makes in the preview.
pub const INVITE: &str = "https://easyplay.invalid/chat/invite#preview-one-time-0001";
/// The link `CreateAddress` makes in the preview.
pub const ADDRESS: &str = "https://easyplay.invalid/chat/address#preview-sam";

/// A ringing call becomes live after this long (the friend answers).
pub const ANSWER_MS: i64 = 2_500;
/// Who speaks changes this often in a live call.
pub const SPEAK_MS: i64 = 1_100;
/// A friend starts watching the game the user shares after this long.
pub const VIEWER_MS: i64 = 2_600;
/// An incoming call nobody answers becomes a missed call after this long.
pub const RING_MS: i64 = 20_000;

/// The member number of `friend` in `group` (stable, for mentions and voice).
pub const fn member_id(group: i64, friend: i64) -> i64 {
    group * 100 + friend
}

/// The user's member number in `group`.
pub const fn my_member_id(group: i64) -> i64 {
    group * 100
}

// ---- chat.html's helpers, bit for bit -------------------------------------------------------

/// chat.html `rng(seed)` (mulberry32): numbers in 0..1.
fn rng(seed: u32) -> impl FnMut() -> f64 {
    let mut a = seed;
    move || {
        a = a.wrapping_add(0x6D2B_79F5);
        let mut t = (a ^ (a >> 15)).wrapping_mul(1 | a);
        t = t.wrapping_add((t ^ (t >> 7)).wrapping_mul(61 | t)) ^ t;
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

/// chat.html `hash(str)` (FNV-1a over UTF-16 units).
fn hash(s: &str) -> u32 {
    let mut h: u32 = 2_166_136_261;
    for u in s.encode_utf16() {
        h ^= u32::from(u);
        h = h.wrapping_mul(16_777_619);
    }
    h
}

/// A voice message's waveform: chat.html `waveBars(seed, clamp(round(secs × 2.6), 20, 56))`,
/// as bar heights 0..=255.
pub fn wave(secs: u32, seed: u32) -> Vec<u8> {
    let n = ((f64::from(secs) * 2.6).round() as usize).clamp(20, 56);
    let mut r = rng(seed.wrapping_mul(977).wrapping_add(13));
    (0..n)
        .map(|i| {
            let v =
                0.16 + 0.84 * (i as f64 * 0.5 + f64::from(seed)).sin().abs() * (0.3 + 0.7 * r());
            // chat.html rounds to two decimals.
            let v = (v * 100.0).round() / 100.0;
            (v * 255.0).round().clamp(0.0, 255.0) as u8
        })
        .collect()
}

/// The security code shown for a friend named `name`: twelve groups of five digits, as
/// chat.html's `verifyDialog` makes them, separated by spaces.
pub fn security_code(name: &str) -> String {
    let mut r = rng(hash(&format!("code{name}")));
    (0..12)
        .map(|_| format!("{:05}", (r() * 100_000.0).floor() as u32))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The first day of `month` 2026, at noon UTC ("Friends since").
fn month_2026(month: u32) -> Timestamp {
    const STARTS: [i64; 12] = [
        1_767_268_800_000,
        1_769_947_200_000,
        1_772_366_400_000,
        1_775_044_800_000,
        1_777_636_800_000,
        1_780_315_200_000,
        1_782_907_200_000,
        1_785_585_600_000,
        1_788_264_000_000,
        1_790_856_000_000,
        1_793_534_400_000,
        1_796_126_400_000,
    ];
    Timestamp::from_millis(STARTS[(month as usize).clamp(1, 12) - 1])
}

/// chat.html `at(ago)`.
#[derive(Clone, Copy)]
enum Ago {
    /// Minutes ago.
    Min(i64),
    /// `days` days ago at `hh:mm` (UTC).
    Day(i64, i64, i64),
}

fn at(now: Timestamp, ago: Ago) -> Timestamp {
    let n = now.as_millis();
    Timestamp::from_millis(match ago {
        Ago::Min(m) => n - m * MIN,
        Ago::Day(d, h, m) => n.div_euclid(DAY) * DAY - d * DAY + (h * 60 + m) * MIN,
    })
}

// ---- The people -----------------------------------------------------------------------------

struct Person {
    id: i64,
    name: &'static str,
    presence: Presence,
    playing: Option<(&'static str, u32, bool)>,
    about: &'static str,
    since: u32,
    seen: &'static [&'static str],
    last_seen: Option<Ago>,
    verified: bool,
    blocked: bool,
}

const fn person(id: i64, name: &'static str, presence: Presence) -> Person {
    Person {
        id,
        name,
        presence,
        playing: None,
        about: "",
        since: 3,
        seen: &[],
        last_seen: None,
        verified: false,
        blocked: false,
    }
}

fn people() -> Vec<Person> {
    vec![
        Person {
            playing: Some(("Lantern Keep", 42, false)),
            about: "Completionist. Will explain the lore whether you ask or not.",
            since: 1,
            seen: &["Lantern Keep", "Hollow Bell", "Clockwork Garden"],
            verified: true,
            ..person(1, "Mira Okafor", Presence::Online)
        },
        Person {
            playing: Some(("Ninefold Arena", 21, true)),
            about: "Ranked grinder. Mic always on, sorry.",
            since: 2,
            seen: &["Ninefold Arena", "Starfall Tactics"],
            ..person(2, "Jonas Weber", Presence::Online)
        },
        Person {
            about: "Strategy games and spreadsheets.",
            seen: &["Starfall Tactics", "Paper Kingdoms"],
            verified: true,
            ..person(3, "Dev Patel", Presence::Online)
        },
        Person {
            about: "Modder. Ask me about shaders.",
            seen: &["Tidewater", "Amber Sky"],
            last_seen: Some(Ago::Min(20)),
            ..person(4, "Aiko Tanaka", Presence::Away)
        },
        Person {
            about: "Do not disturb means do not disturb.",
            seen: &["Amber Sky"],
            ..person(5, "Lucía Fernández", Presence::Busy)
        },
        Person {
            seen: &["Moss & Iron"],
            last_seen: Some(Ago::Min(180)),
            ..person(6, "Tomasz Nowak", Presence::Offline)
        },
        Person {
            seen: &["Harbor Lights"],
            last_seen: Some(Ago::Day(1, 18, 5)),
            ..person(7, "Noor Haddad", Presence::Offline)
        },
        Person {
            since: 5,
            blocked: true,
            ..person(8, "Rex Calloway", Presence::Offline)
        },
    ]
}

fn friends(now: Timestamp) -> Vec<Friend> {
    people()
        .into_iter()
        .map(|p| Friend {
            id: p.id,
            name: p.name.to_owned(),
            ready: true,
            presence: p.presence,
            playing: p.playing.map(|(title, minutes, live)| Playing {
                title: title.to_owned(),
                minutes,
                live,
            }),
            last_seen: p.last_seen.map(|a| at(now, a)),
            about: p.about.to_owned(),
            paper: None,
            since: Some(month_2026(p.since)),
            verified: p.verified,
            blocked: p.blocked,
            muted: false,
            seen: p.seen.iter().map(|s| (*s).to_owned()).collect(),
        })
        .collect()
}

fn me_member(group: i64, owner: bool) -> GroupMember {
    GroupMember {
        id: my_member_id(group),
        name: "Sam".into(),
        friend: None,
        me: true,
        owner,
        presence: Presence::Online,
        playing: None,
        blocked: false,
    }
}

fn group_member(group: i64, f: &Friend, owner: bool) -> GroupMember {
    GroupMember {
        id: member_id(group, f.id),
        name: f.name.clone(),
        friend: Some(f.id),
        me: false,
        owner,
        presence: f.presence,
        playing: f.playing.clone(),
        blocked: f.blocked,
    }
}

/// A group of `members` (friend numbers), the user among them.
fn group(
    friends: &[Friend],
    id: i64,
    name: &str,
    members: &[i64],
    owner: Option<i64>,
    voice: &[i64],
    muted: bool,
) -> ChatGroup {
    let mut list = vec![me_member(id, owner.is_none())];
    list.extend(
        members
            .iter()
            .filter_map(|m| friends.iter().find(|f| f.id == *m))
            .map(|f| group_member(id, f, owner == Some(f.id))),
    );
    ChatGroup {
        id,
        name: name.to_owned(),
        membership: Membership::Member,
        members: Some(list.len() as u32),
        member_list: list,
        muted,
        voice: voice.iter().map(|f| member_id(id, *f)).collect(),
        link: None,
    }
}

// ---- The conversations ----------------------------------------------------------------------

/// Who wrote a message: the user, or a friend by number.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Who {
    Me,
    F(i64),
}

/// Builds one conversation's messages, numbering them in order.
struct Convo<'a> {
    chat: ChatRef,
    now: Timestamp,
    next: &'a mut i64,
    names: &'a HashMap<i64, String>,
    msgs: Vec<ChatMessage>,
}

impl Convo<'_> {
    fn author(&self, who: Who) -> MessageAuthor {
        match (who, self.chat) {
            (Who::Me, _) => MessageAuthor::Me,
            (Who::F(_), ChatRef::Direct(_)) => MessageAuthor::Friend,
            (Who::F(f), ChatRef::Group(_)) => {
                MessageAuthor::Member(self.names.get(&f).cloned().unwrap_or_default())
            }
        }
    }

    fn name(&self, who: Who) -> String {
        match who {
            Who::Me => "Sam".into(),
            Who::F(f) => self.names.get(&f).cloned().unwrap_or_default(),
        }
    }

    /// Add a message; returns its index in the conversation.
    fn mk(&mut self, from: Who, ago: Ago, body: MessageBody) -> usize {
        *self.next += 1;
        let mine = from == Who::Me;
        let m = ChatMessage {
            id: *self.next,
            chat: self.chat,
            author: self.author(from),
            body,
            at: at(self.now, ago),
            delivery: if mine {
                Delivery::Delivered
            } else {
                Delivery::Received
            },
            unread: false,
            reply: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
        };
        self.msgs.push(m);
        self.msgs.len() - 1
    }

    fn t(&mut self, from: Who, ago: Ago, text: &str) -> usize {
        self.mk(from, ago, MessageBody::Text(text.to_owned()))
    }

    /// Reactions on message `i`: each emoji with who reacted.
    fn rx(&mut self, i: usize, rx: &[(&str, &[Who])]) {
        let reactions = rx
            .iter()
            .map(|(emoji, who)| Reaction {
                emoji: (*emoji).to_owned(),
                count: who.len() as u32,
                mine: who.contains(&Who::Me),
                who: who
                    .iter()
                    .filter(|w| **w != Who::Me)
                    .map(|w| self.name(*w))
                    .collect(),
            })
            .collect();
        self.msgs[i].reactions = reactions;
    }

    /// Message `i` answers message `to`.
    fn re(&mut self, i: usize, to: usize) {
        let q = &self.msgs[to];
        let quote = Quote {
            id: Some(q.id),
            author: q.author.clone(),
            text: quote_text(&q.body),
        };
        self.msgs[i].reply = Some(quote);
    }

    /// The last `n` messages from others are unread.
    fn unread(&mut self, n: usize) {
        for m in self
            .msgs
            .iter_mut()
            .rev()
            .filter(|m| m.author != MessageAuthor::Me)
            .take(n)
        {
            m.unread = true;
        }
    }
}

/// What a reply shows of the message it answers: its first line.
fn quote_text(body: &MessageBody) -> String {
    match body {
        MessageBody::Text(t) => t.lines().next().unwrap_or("").to_owned(),
        MessageBody::Gif(g) => format!("GIF {g}"),
        MessageBody::Stream(s) => s.title.clone(),
        _ => String::new(),
    }
}

/// A small preview picture (PNG): a sky from `top` to `bottom` with a sun, `w` × `h`.
pub fn preview_png(w: u32, h: u32, top: [u8; 3], bottom: [u8; 3]) -> Vec<u8> {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let t = y as f32 / h.max(1) as f32;
        let mix = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
        let (cx, cy, r) = (w as f32 * 0.68, h as f32 * 0.42, h as f32 * 0.16);
        let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
        if d < r {
            image::Rgba([255, 214, 150, 255])
        } else if y as f32 > h as f32 * 0.72 {
            image::Rgba([mix(top[0], 20), mix(top[1], 24), mix(top[2], 30), 255])
        } else {
            image::Rgba([
                mix(top[0], bottom[0]),
                mix(top[1], bottom[1]),
                mix(top[2], bottom[2]),
                255,
            ])
        }
    });
    let mut out = Vec::new();
    let _ = img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png);
    out
}

/// A picture, video or file offered (or sent) with `name` and `size`.
fn media(
    name: &str,
    size: u64,
    preview: Option<Vec<u8>>,
    duration: Option<u32>,
    state: FileState,
) -> Media {
    Media {
        name: name.into(),
        size,
        preview,
        duration,
        state,
    }
}

fn voice(secs: u32, seed: u32) -> MessageBody {
    MessageBody::Voice {
        secs,
        wave: wave(secs, seed),
    }
}

fn call(kind: CallKind, minutes: u32) -> MessageBody {
    MessageBody::Call(CallRecord { kind, minutes })
}

fn text_with_mention(c: &mut Convo<'_>, from: Who, ago: Ago, text: &str) -> usize {
    let i = c.t(from, ago, text);
    if let ChatRef::Group(g) = c.chat {
        c.msgs[i].mentions = vec![Mention {
            name: "Sam".into(),
            member: my_member_id(g),
            me: true,
        }];
    }
    i
}

/// Every conversation's messages (chat.html `demoChat()`), oldest first, numbered from 1 in
/// list order.
fn conversations(now: Timestamp, names: &HashMap<i64, String>) -> Vec<(ChatRef, Vec<ChatMessage>)> {
    use Ago::{Day, Min};
    use Who::{F, Me};
    let (mira, jonas, dev, aiko, lucia, tomasz, noor, rex) =
        (F(1), F(2), F(3), F(4), F(5), F(6), F(7), F(8));
    let mut next = 0;
    let mut out = Vec::new();
    let mut convo = |chat: ChatRef, build: &dyn Fn(&mut Convo<'_>)| {
        let mut c = Convo {
            chat,
            now,
            next: &mut next,
            names,
            msgs: Vec::new(),
        };
        build(&mut c);
        out.push((chat, c.msgs));
    };

    convo(MIRA, &|c| {
        c.t(
            mira,
            Day(1, 21, 2),
            "did you get past the *lighthouse* boss yet?",
        );
        let a = c.t(
            Me,
            Day(1, 21, 3),
            "nope. third try and it still flattens me 💀",
        );
        c.mk(mira, Day(1, 21, 4), voice(14, 7));
        let i = c.t(
            Me,
            Day(1, 21, 6),
            "ohh, so you _bait_ the lunge first. ok, again",
        );
        c.rx(i, &[("👍", &[mira])]);
        c.mk(Me, Day(1, 21, 52), call(CallKind::Ended, 38));
        let i = c.t(mira, Min(34), "so? did it work 👀");
        c.re(i, a);
        c.mk(Me, Min(33), MessageBody::Gif("letsgo".into()));
        c.t(
            Me,
            Min(33),
            "first try with your route:\nleft stairs, not the bridge\nskip the `lantern` puzzle\nparry twice, then run",
        );
        let i = c.t(mira, Min(31), "🎉🎉🎉");
        c.rx(i, &[("🏆", &[Me, mira]), ("🔥", &[Me])]);
        c.t(
            mira,
            Min(30),
            "told you. the ending is #not what you think# by the way",
        );
        c.t(
            mira,
            Min(4),
            "I'm in Lantern Keep doing the bell tower now. call if you get stuck",
        );
    });
    convo(CREW, &|c| {
        c.t(
            dev,
            Day(1, 18, 30),
            "patch notes are out: https://ninefold.example/patch-4-2",
        );
        c.t(lucia, Day(1, 18, 34), "they nerfed my main *again* 😭");
        let i = c.t(jonas, Day(1, 18, 35), "skill issue");
        c.rx(i, &[("😂", &[dev, mira, Me]), ("💀", &[lucia])]);
        c.t(
            dev,
            Day(1, 18, 41),
            "for anyone on Linux, this launch option fixed the stutter for me: `PROTON_ENABLE_WAYLAND=1 %command%`",
        );
        c.mk(
            jonas,
            Min(21),
            MessageBody::Stream(StreamRecord {
                title: "Ninefold Arena".into(),
                live: true,
                watching: 2,
                minutes: 0,
            }),
        );
        c.t(jonas, Min(20), "going live. ranked until I hit gold or cry");
        c.t(
            mira,
            Min(12),
            "watching. your aim is _suspiciously_ good today",
        );
        text_with_mention(c, dev, Min(9), "@Sam you joining voice? we need a third");
        c.mk(jonas, Min(2), voice(6, 3));
        c.unread(3);
    });
    convo(DEV, &|c| {
        c.t(Me, Day(2, 20, 15), "that last match was robbery");
        let i = c.t(dev, Day(2, 20, 16), "the spreadsheet says otherwise 📎");
        c.rx(i, &[("🤣", &[Me])]);
        c.t(
            dev,
            Min(55),
            "you up for ranked later? need one more for the 8 pm queue",
        );
        c.unread(1);
    });
    convo(AIKO, &|c| {
        c.t(
            Me,
            Day(3, 16, 40),
            "does the shader pack work with the GOG build?",
        );
        c.mk(aiko, Min(96), call(CallKind::Missed, 0));
        c.t(
            aiko,
            Min(95),
            "tried calling about the shader pack. no rush, the new build is in the _Mods_ folder",
        );
        c.unread(2);
    });
    convo(LUCIA, &|c| {
        c.t(lucia, Day(1, 23, 10), "gg last night 🙌");
        c.mk(Me, Day(1, 23, 11), MessageBody::Gif("gg".into()));
        // Pictures, a video and files (round 3): offered, not fetched yet.
        c.mk(
            lucia,
            Day(1, 23, 14),
            MessageBody::Image(media(
                "IMG_20261117_231402.jpg",
                2_400_000,
                Some(preview_png(96, 72, [214, 120, 92], [96, 52, 110])),
                None,
                FileState::Offered,
            )),
        );
        c.mk(
            lucia,
            Day(1, 23, 15),
            MessageBody::Video(media(
                "clutch.mp4",
                18_200_000,
                Some(preview_png(128, 72, [40, 70, 120], [16, 22, 40])),
                Some(42),
                FileState::Offered,
            )),
        );
        c.mk(
            lucia,
            Day(1, 23, 16),
            MessageBody::File(media(
                "shaders-v3.zip",
                3_100_000,
                None,
                None,
                FileState::Offered,
            )),
        );
        c.mk(
            Me,
            Day(1, 23, 18),
            MessageBody::File(media(
                "save_slot2.dat",
                840_000,
                None,
                None,
                FileState::Sending { percent: 40 },
            )),
        );
    });
    convo(TOMASZ, &|c| {
        c.t(
            tomasz,
            Day(1, 14, 2),
            "can you send me your save? mine got eaten",
        );
        let i = c.t(Me, Min(150), "sent it over, check when you are back");
        c.msgs[i].delivery = Delivery::Sending;
    });
    convo(MODS, &|c| {
        c.t(noor, Day(3, 11, 20), "texture pack v3 is up");
        c.t(
            aiko,
            Day(3, 11, 42),
            "nice. I will test it on *Tidewater* tonight",
        );
        let i = c.t(tomasz, Day(3, 12, 5), "works here ✅");
        c.rx(i, &[("🙏", &[noor])]);
    });
    convo(NOOR, &|c| {
        c.t(
            noor,
            Day(8, 19, 30),
            "thanks for the invite link, got in fine",
        );
    });
    convo(JONAS, &|c| {
        c.t(jonas, Day(4, 22, 40), "one more?");
        c.t(Me, Day(4, 22, 41), "one more. then sleep 😴");
    });
    convo(REX, &|c| {
        c.t(rex, Day(12, 2, 14), "selling accounts, cheap");
        c.t(rex, Day(12, 2, 15), "hello??");
    });
    out
}

/// Everything the preview starts with.
struct Built {
    snapshot: ChatSnapshot,
    histories: HashMap<ChatRef, Vec<ChatMessage>>,
    next: i64,
}

fn build(now: Timestamp) -> Built {
    let friends = friends(now);
    let names: HashMap<i64, String> = friends.iter().map(|f| (f.id, f.name.clone())).collect();
    let groups = vec![
        group(
            &friends,
            10,
            "Friday Crew",
            &[1, 2, 3, 5],
            Some(2),
            &[2, 3],
            false,
        ),
        group(&friends, 11, "Mod makers", &[4, 6, 7], None, &[], true),
    ];
    let convos = conversations(now, &names);
    let next = convos
        .iter()
        .flat_map(|(_, m)| m.iter().map(|m| m.id))
        .max()
        .unwrap_or(0)
        .max(1000);
    let histories: HashMap<ChatRef, Vec<ChatMessage>> = convos.into_iter().collect();
    let mut snapshot = ChatSnapshot {
        status: ChatStatus::Ready,
        profile: Some(ChatProfile {
            name: "Sam".into(),
            about: "Mostly co-op, sometimes ranked.".into(),
            paper: Some(0),
            status: MyStatus::Online,
        }),
        friends,
        groups,
        requests: vec![FriendRequest {
            id: KIT_REQUEST,
            name: "Kit Brennan".into(),
        }],
        features: ChatFeatures::all(),
        ..ChatSnapshot::default()
    };
    snapshot.chats = list(&snapshot, &histories, &HashMap::new());
    Built {
        snapshot,
        histories,
        next,
    }
}

/// The chat list: every conversation except those with blocked friends, newest first
/// (chat.html `listed()`). `made` dates conversations without messages.
fn list(
    s: &ChatSnapshot,
    histories: &HashMap<ChatRef, Vec<ChatMessage>>,
    made: &HashMap<ChatRef, Timestamp>,
) -> Vec<ChatPreview> {
    let preview = |chat: ChatRef, name: &str, muted: bool| {
        let h = histories.get(&chat).map(Vec::as_slice).unwrap_or_default();
        ChatPreview {
            chat,
            name: name.to_owned(),
            last: h.last().cloned(),
            unread: h.iter().filter(|m| m.unread).count() as u32,
            muted,
        }
    };
    let mut chats: Vec<ChatPreview> = s
        .friends
        .iter()
        .filter(|f| !f.blocked)
        .map(|f| preview(ChatRef::Direct(f.id), &f.name, f.muted))
        .chain(
            s.groups
                .iter()
                .filter(|g| g.membership != Membership::Left)
                .map(|g| preview(ChatRef::Group(g.id), &g.name, g.muted)),
        )
        .collect();
    let key = |c: &ChatPreview| {
        c.last
            .as_ref()
            .map(|m| m.at)
            .or_else(|| made.get(&c.chat).copied())
            .unwrap_or(Timestamp::EPOCH)
    };
    chats.sort_by_key(|c| std::cmp::Reverse(key(c)));
    chats
}

/// The history of `chat` as the preview starts, oldest first.
pub fn history(chat: ChatRef, now: Timestamp) -> Vec<ChatMessage> {
    build(now).histories.remove(&chat).unwrap_or_default()
}

/// A page of `messages` (oldest first): the last [`crate::vm::chat::PAGE`] older than
/// `before` (`None`: the latest).
fn page_of(chat: ChatRef, messages: &[ChatMessage], before: Option<i64>) -> ChatPage {
    let older: Vec<&ChatMessage> = messages
        .iter()
        .filter(|m| before.is_none_or(|b| m.id < b))
        .collect();
    let from = older.len().saturating_sub(crate::vm::chat::PAGE);
    ChatPage {
        chat,
        messages: older[from..].iter().map(|m| (*m).clone()).collect(),
    }
}

/// A page of the starting history (the tests' answer to `ChatCommand::LoadChat`).
pub fn page(chat: ChatRef, before: Option<i64>, now: Timestamp) -> ChatPage {
    page_of(chat, &history(chat, now), before)
}

/// The preview's snapshot as it starts: Chat running, profile "Sam".
pub fn sample(now: Timestamp) -> ChatSnapshot {
    build(now).snapshot
}

/// The page's own state as the preview starts: chat.html's `state.chat.freq` (how often
/// each emoji was used) and `savedGifs`.
pub fn seed_vm(vm: &mut crate::vm::chat::ChatVm) {
    const FREQ: [(&str, u32); 16] = [
        ("👍", 9),
        ("😂", 8),
        ("❤️", 7),
        ("🔥", 6),
        ("🎉", 5),
        ("😭", 5),
        ("👀", 4),
        ("💀", 4),
        ("🙏", 3),
        ("😅", 3),
        ("🎮", 3),
        ("✅", 2),
        ("🤔", 2),
        ("😎", 2),
        ("💯", 1),
        ("👏", 1),
    ];
    vm.freq = FREQ.iter().map(|(e, n)| ((*e).to_owned(), *n)).collect();
    vm.saved_gifs = ["gg", "letsgo", "nope"].map(str::to_owned).to_vec();
}

// ---- Playing the commands -------------------------------------------------------------------

/// Something that happens later by itself (chat.html section 25's timers).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Timer {
    /// The friend answers the user's call.
    Answer,
    /// Someone else speaks in the live call.
    Speak,
    /// A friend starts watching the game the user shares (its message).
    Viewer(i64),
    /// Nobody answered the incoming call.
    Missed,
}

/// The preview's Chat: the starting snapshot and the conversations' histories, changed by
/// the commands and timers as the service would change them.
#[derive(Debug)]
pub struct DemoChat {
    snapshot: Arc<ChatSnapshot>,
    histories: HashMap<ChatRef, Vec<ChatMessage>>,
    /// When conversations without messages were made (they sort by it).
    made: HashMap<ChatRef, Timestamp>,
    next: i64,
    timers: Vec<(Timestamp, Timer)>,
    /// Speaking turns so far (picks who speaks next).
    turns: u32,
    /// The message of the game the user shares.
    share_msg: Option<(ChatRef, i64)>,
    /// The title of the game running now (for sharing).
    running: Option<String>,
}

impl DemoChat {
    /// The preview as it starts at `now`.
    pub fn new(now: Timestamp) -> Self {
        let b = build(now);
        Self {
            snapshot: Arc::new(b.snapshot),
            histories: b.histories,
            made: HashMap::new(),
            next: b.next,
            timers: Vec::new(),
            turns: 0,
            share_msg: None,
            running: None,
        }
    }

    /// The current snapshot.
    pub fn snapshot(&self) -> Arc<ChatSnapshot> {
        self.snapshot.clone()
    }

    /// The messages of `chat`, oldest first.
    pub fn messages(&self, chat: ChatRef) -> &[ChatMessage] {
        self.histories
            .get(&chat)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    /// The game running now (`None`: nothing), which `StartShare` shares.
    pub fn set_running(&mut self, title: Option<String>) {
        self.running = title;
    }

    /// When the next timer is due, if any.
    pub fn next_due(&self) -> Option<Timestamp> {
        self.timers.iter().map(|(t, _)| *t).min()
    }

    fn s(&mut self) -> &mut ChatSnapshot {
        Arc::make_mut(&mut self.snapshot)
    }

    fn relist(&mut self) {
        let chats = list(&self.snapshot, &self.histories, &self.made);
        self.s().chats = chats;
    }

    fn schedule(&mut self, due: Timestamp, t: Timer) {
        self.timers.push((due, t));
    }

    fn cancel(&mut self, f: impl Fn(&Timer) -> bool) {
        self.timers.retain(|(_, t)| !f(t));
    }

    fn name_of(&self, who: &MessageAuthor, chat: ChatRef) -> String {
        match (who, chat) {
            (MessageAuthor::Me, _) => self
                .snapshot
                .profile
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            (MessageAuthor::Member(n), _) => n.clone(),
            (MessageAuthor::Friend, ChatRef::Direct(f)) => self
                .snapshot
                .friend(f)
                .map(|f| f.name.clone())
                .unwrap_or_default(),
            (MessageAuthor::Friend, ChatRef::Group(_)) => String::new(),
        }
    }

    /// Add a message to `chat`; returns it (to announce).
    fn post(
        &mut self,
        chat: ChatRef,
        author: MessageAuthor,
        body: MessageBody,
        now: Timestamp,
    ) -> ChatMessage {
        self.next += 1;
        let mine = author == MessageAuthor::Me;
        let m = ChatMessage {
            id: self.next,
            chat,
            author,
            body,
            at: now,
            delivery: if mine {
                Delivery::Delivered
            } else {
                Delivery::Received
            },
            unread: !mine,
            reply: None,
            reactions: Vec::new(),
            edited: false,
            mentions: Vec::new(),
        };
        self.histories.entry(chat).or_default().push(m.clone());
        m
    }

    /// Change message `item` of `chat`; returns it changed (to announce).
    fn change(
        &mut self,
        chat: ChatRef,
        item: i64,
        f: impl FnOnce(&mut ChatMessage),
    ) -> Option<ChatMessage> {
        let m = self
            .histories
            .get_mut(&chat)?
            .iter_mut()
            .find(|m| m.id == item)?;
        f(m);
        Some(m.clone())
    }

    /// Someone calls the user from `chat` (a friend's conversation): the incoming card,
    /// which becomes a missed call after [`RING_MS`] unanswered. Nothing happens while the
    /// user is in a call or someone is already calling, or when the friend is blocked.
    pub fn ring_in(&mut self, chat: ChatRef, now: Timestamp) -> Vec<UiEvent> {
        let ChatRef::Direct(f) = chat else {
            return Vec::new();
        };
        let Some(friend) = self.snapshot.friend(f).filter(|f| !f.blocked) else {
            return Vec::new();
        };
        if self.snapshot.live.call.is_some() || self.snapshot.live.ringing.is_some() {
            return Vec::new();
        }
        let name = friend.name.clone();
        self.s().live.ringing = Some(Ringing {
            chat,
            name,
            since: now,
        });
        self.schedule(
            Timestamp::from_millis(now.as_millis() + RING_MS),
            Timer::Missed,
        );
        vec![UiEvent::ChatChanged]
    }

    /// Run the timers due by `now`; returns the events the service would announce.
    pub fn tick(&mut self, now: Timestamp) -> Vec<UiEvent> {
        let mut out = Vec::new();
        let mut changed = false;
        while let Some(i) = self
            .timers
            .iter()
            .enumerate()
            .filter(|(_, (t, _))| *t <= now)
            .min_by_key(|(_, (t, _))| *t)
            .map(|(i, _)| i)
        {
            let (due, timer) = self.timers.remove(i);
            changed = true;
            match timer {
                Timer::Answer => {
                    if let Some(c) = &mut self.s().live.call
                        && c.since.is_none()
                    {
                        c.since = Some(due);
                    }
                    self.schedule(
                        Timestamp::from_millis(due.as_millis() + SPEAK_MS),
                        Timer::Speak,
                    );
                }
                Timer::Speak => {
                    self.turns = self.turns.wrapping_add(1);
                    let turn = self.turns;
                    if let Some(c) = &mut self.s().live.call {
                        // chat.html: [...with, ...with, you unless muted, nobody].
                        let mut pool: Vec<Option<&String>> =
                            c.with.iter().chain(c.with.iter()).map(Some).collect();
                        if !c.muted {
                            pool.push(None);
                        }
                        pool.push(None);
                        let pick = pool[(hash(&turn.to_string()) as usize) % pool.len()];
                        c.speaking = match pick {
                            Some(n) if !c.deafened => vec![n.clone()],
                            _ => Vec::new(),
                        };
                        let next = Timestamp::from_millis(due.as_millis() + SPEAK_MS);
                        self.schedule(next, Timer::Speak);
                    }
                }
                Timer::Viewer(item) => {
                    if self.share_msg.is_some_and(|(_, m)| m == item) {
                        let chat = self.share_msg.map(|(c, _)| c).unwrap_or(CREW);
                        if let Some(sh) = &mut self.s().live.share {
                            sh.watching = 1;
                        }
                        if let Some(m) = self.change(chat, item, |m| {
                            if let MessageBody::Stream(s) = &mut m.body {
                                s.watching = 1;
                            }
                        }) {
                            out.push(UiEvent::ChatMessages {
                                messages: vec![m],
                                updated: true,
                            });
                        }
                    }
                }
                Timer::Missed => {
                    if let Some(r) = self.s().live.ringing.take() {
                        let m = self.post(
                            r.chat,
                            MessageAuthor::Friend,
                            call(CallKind::Missed, 0),
                            due,
                        );
                        out.push(UiEvent::ChatMessages {
                            messages: vec![m],
                            updated: false,
                        });
                    }
                }
            }
        }
        if changed {
            self.relist();
            out.push(UiEvent::ChatChanged);
        }
        out
    }

    /// Leave the call (chat.html `endCall`): a direct call that was live writes "Call ended,
    /// N min".
    fn end_call(&mut self, now: Timestamp, out: &mut Vec<UiEvent>) {
        let Some(c) = self.s().live.call.take() else {
            return;
        };
        self.cancel(|t| matches!(t, Timer::Answer | Timer::Speak));
        match c.chat {
            ChatRef::Direct(_) => {
                if let Some(since) = c.since {
                    let ms = (now.as_millis() - since.as_millis()).max(0);
                    let minutes = ((ms as f64 / MIN as f64).round() as u32).max(1);
                    let m = self.post(
                        c.chat,
                        MessageAuthor::Me,
                        call(CallKind::Ended, minutes),
                        now,
                    );
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: false,
                    });
                }
            }
            ChatRef::Group(g) => {
                if let Some(gr) = self.s().groups.iter_mut().find(|x| x.id == g) {
                    gr.voice.retain(|v| *v != my_member_id(g));
                }
            }
        }
    }

    fn stop_share(&mut self, now: Timestamp, out: &mut Vec<UiEvent>) {
        if self.s().live.share.take().is_none() {
            return;
        }
        let Some((chat, item)) = self.share_msg.take() else {
            return;
        };
        self.cancel(|t| matches!(t, Timer::Viewer(_)));
        let since = self
            .messages(chat)
            .iter()
            .find(|m| m.id == item)
            .map_or(now, |m| m.at);
        let minutes = ((((now.as_millis() - since.as_millis()).max(0)) as f64 / MIN as f64).round()
            as u32)
            .max(1);
        if let Some(m) = self.change(chat, item, |m| {
            if let MessageBody::Stream(s) = &mut m.body {
                s.live = false;
                s.minutes = minutes;
            }
        }) {
            out.push(UiEvent::ChatMessages {
                messages: vec![m],
                updated: true,
            });
        }
    }

    fn stop_watch(&mut self, out: &mut Vec<UiEvent>) {
        let Some(w) = self.s().live.watch.take() else {
            return;
        };
        let Some(item) = self.live_stream(w.chat) else {
            return;
        };
        if let Some(m) = self.change(w.chat, item, |m| {
            if let MessageBody::Stream(s) = &mut m.body {
                s.watching = s.watching.saturating_sub(1);
            }
        }) {
            out.push(UiEvent::ChatMessages {
                messages: vec![m],
                updated: true,
            });
        }
    }

    /// The live stream of someone else in `chat` (its latest).
    fn live_stream(&self, chat: ChatRef) -> Option<i64> {
        self.messages(chat)
            .iter()
            .rev()
            .find(|m| {
                m.author != MessageAuthor::Me && matches!(&m.body, MessageBody::Stream(s) if s.live)
            })
            .map(|m| m.id)
    }

    /// The game stopped: sharing it ends (chat.html `chatRunChanged`).
    pub fn game_stopped(&mut self, now: Timestamp) -> Vec<UiEvent> {
        self.running = None;
        let mut out = Vec::new();
        if self.snapshot.live.share.is_some() {
            self.stop_share(now, &mut out);
            out.push(UiEvent::ChatChanged);
        }
        out
    }

    /// Apply `cmd`; returns the events the service would announce.
    #[allow(clippy::too_many_lines)]
    pub fn handle(&mut self, cmd: ChatCommand, now: Timestamp) -> Vec<UiEvent> {
        let mut out = Vec::new();
        match cmd {
            ChatCommand::SetEnabled { on, name } => {
                let s = self.s();
                s.status = if on {
                    ChatStatus::Ready
                } else {
                    ChatStatus::Off
                };
                if let Some(n) = name {
                    s.profile.get_or_insert_with(ChatProfile::default).name = n;
                }
            }
            ChatCommand::SetName(n) => {
                self.s()
                    .profile
                    .get_or_insert_with(ChatProfile::default)
                    .name = n;
            }
            ChatCommand::SetProfile(p) => self.s().profile = Some(p),
            ChatCommand::SetStatus(st) => {
                self.s()
                    .profile
                    .get_or_insert_with(ChatProfile::default)
                    .status = st;
            }
            ChatCommand::CreateInvite => self.s().invite = Some(INVITE.to_owned()),
            ChatCommand::CreateAddress => self.s().address = Some(ADDRESS.to_owned()),
            ChatCommand::DeleteAddress => self.s().address = None,
            // chat.html: "Request sent. They appear in your chats once they accept."
            ChatCommand::Connect(_) | ChatCommand::OpenLink(_) => {}
            ChatCommand::Accept(id) | ChatCommand::Reject(id) => {
                let accept = matches!(cmd, ChatCommand::Accept(_));
                let s = self.s();
                let Some(i) = s.requests.iter().position(|r| r.id == id) else {
                    return out;
                };
                let r = s.requests.remove(i);
                if accept {
                    let fid = if id == KIT_REQUEST {
                        KIT
                    } else {
                        self.next += 1;
                        self.next
                    };
                    self.s().friends.push(Friend {
                        id: fid,
                        name: r.name,
                        ready: true,
                        presence: Presence::Online,
                        since: Some(now),
                        ..Friend::default()
                    });
                    self.made.insert(ChatRef::Direct(fid), now);
                    self.histories.entry(ChatRef::Direct(fid)).or_default();
                }
            }
            ChatCommand::Send {
                chat,
                text,
                reply_to,
                mentions,
            } => {
                let reply = reply_to.and_then(|id| {
                    let q = self.messages(chat).iter().find(|m| m.id == id)?;
                    Some(Quote {
                        id: Some(q.id),
                        author: q.author.clone(),
                        text: quote_text(&q.body),
                    })
                });
                let mine = match chat {
                    ChatRef::Group(g) => Some(my_member_id(g)),
                    ChatRef::Direct(_) => None,
                };
                let mut m = self.post(chat, MessageAuthor::Me, MessageBody::Text(text), now);
                m.reply = reply;
                m.mentions = mentions
                    .into_iter()
                    .map(|(name, member)| Mention {
                        name,
                        member,
                        me: Some(member) == mine,
                    })
                    .collect();
                let set = m.clone();
                self.change(chat, m.id, |x| *x = set);
                out.push(UiEvent::ChatMessages {
                    messages: vec![m],
                    updated: false,
                });
            }
            ChatCommand::SendVoice { chat, secs } => {
                // The seed only shapes the stand-in waveform.
                let seed = (self.next as u32).wrapping_add(secs);
                let m = self.post(chat, MessageAuthor::Me, voice(secs, seed), now);
                out.push(UiEvent::ChatMessages {
                    messages: vec![m],
                    updated: false,
                });
            }
            ChatCommand::Edit { chat, item, text } => {
                if let Some(m) = self.change(chat, item, |m| {
                    if m.author == MessageAuthor::Me
                        && matches!(&m.body, MessageBody::Text(t) if *t != text)
                    {
                        m.body = MessageBody::Text(text);
                        m.edited = true;
                    }
                }) {
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            ChatCommand::Delete { chat, item } => {
                if let Some(m) = self.change(chat, item, |m| {
                    if m.author == MessageAuthor::Me {
                        m.body = MessageBody::Deleted;
                        m.reactions.clear();
                        m.reply = None;
                        m.mentions.clear();
                    }
                }) {
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            ChatCommand::React {
                chat,
                item,
                emoji,
                on,
            } => {
                if let Some(m) = self.change(chat, item, |m| react(m, &emoji, on)) {
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            ChatCommand::SetMuted { chat, on } => {
                let s = self.s();
                match chat {
                    ChatRef::Direct(f) => {
                        if let Some(f) = s.friends.iter_mut().find(|x| x.id == f) {
                            f.muted = on;
                        }
                    }
                    ChatRef::Group(g) => {
                        if let Some(g) = s.groups.iter_mut().find(|x| x.id == g) {
                            g.muted = on;
                        }
                    }
                }
            }
            ChatCommand::SetBlocked { friend, on } => {
                if on {
                    if self
                        .snapshot
                        .live
                        .call
                        .as_ref()
                        .is_some_and(|c| c.chat == ChatRef::Direct(friend))
                    {
                        self.end_call(now, &mut out);
                    }
                    if self
                        .snapshot
                        .live
                        .watch
                        .as_ref()
                        .is_some_and(|w| w.chat == ChatRef::Direct(friend))
                    {
                        self.stop_watch(&mut out);
                    }
                    if self
                        .snapshot
                        .live
                        .ringing
                        .as_ref()
                        .is_some_and(|r| r.chat == ChatRef::Direct(friend))
                    {
                        self.s().live.ringing = None;
                        self.cancel(|t| *t == Timer::Missed);
                    }
                }
                let s = self.s();
                if let Some(f) = s.friends.iter_mut().find(|x| x.id == friend) {
                    f.blocked = on;
                }
                for m in s
                    .groups
                    .iter_mut()
                    .flat_map(|g| g.member_list.iter_mut())
                    .filter(|m| m.friend == Some(friend))
                {
                    m.blocked = on;
                }
            }
            ChatCommand::SetMemberBlocked { group, member, on } => {
                if let Some(m) = self
                    .s()
                    .groups
                    .iter_mut()
                    .filter(|g| g.id == group)
                    .flat_map(|g| g.member_list.iter_mut())
                    .find(|m| m.id == member && !m.me)
                {
                    m.blocked = on;
                }
            }
            ChatCommand::ShowCode(friend) => {
                let code = self.snapshot.friend(friend).map(|f| security_code(&f.name));
                self.s().code = code.map(|c| (friend, c));
            }
            ChatCommand::Verify { friend, .. } => {
                if let Some(f) = self.s().friends.iter_mut().find(|x| x.id == friend) {
                    f.verified = true;
                }
            }
            ChatCommand::Call(chat) => self.call(chat, now),
            ChatCommand::Answer => {
                if let Some(r) = self.s().live.ringing.take() {
                    self.cancel(|t| *t == Timer::Missed);
                    self.s().live.call = Some(Call {
                        chat: r.chat,
                        since: Some(now),
                        with: vec![r.name],
                        speaking: Vec::new(),
                        muted: false,
                        deafened: false,
                    });
                    self.schedule(
                        Timestamp::from_millis(now.as_millis() + SPEAK_MS),
                        Timer::Speak,
                    );
                }
            }
            ChatCommand::Decline => {
                if let Some(r) = self.s().live.ringing.take() {
                    self.cancel(|t| *t == Timer::Missed);
                    let m = self.post(r.chat, MessageAuthor::Me, call(CallKind::Declined, 0), now);
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: false,
                    });
                }
            }
            ChatCommand::EndCall => self.end_call(now, &mut out),
            ChatCommand::CallMute(on) => {
                if let Some(c) = &mut self.s().live.call {
                    c.muted = on;
                }
            }
            ChatCommand::CallDeafen(on) => {
                if let Some(c) = &mut self.s().live.call {
                    c.deafened = on;
                    if on {
                        c.speaking.clear();
                    }
                }
            }
            ChatCommand::StartShare(chat) => {
                let Some(title) = self.running.clone() else {
                    return out;
                };
                if self.snapshot.live.share.is_some() {
                    return out;
                }
                let m = self.post(
                    chat,
                    MessageAuthor::Me,
                    MessageBody::Stream(StreamRecord {
                        title: title.clone(),
                        live: true,
                        watching: 0,
                        minutes: 0,
                    }),
                    now,
                );
                self.share_msg = Some((chat, m.id));
                self.s().live.share = Some(Share {
                    chat,
                    title,
                    since: now,
                    watching: 0,
                });
                self.schedule(
                    Timestamp::from_millis(now.as_millis() + VIEWER_MS),
                    Timer::Viewer(m.id),
                );
                out.push(UiEvent::ChatMessages {
                    messages: vec![m],
                    updated: false,
                });
            }
            ChatCommand::StopShare => self.stop_share(now, &mut out),
            ChatCommand::Watch(chat) => {
                self.stop_watch(&mut out);
                if let Some(item) = self.live_stream(chat)
                    && let Some(m) = self.change(chat, item, |m| {
                        if let MessageBody::Stream(s) = &mut m.body {
                            s.watching += 1;
                        }
                    })
                {
                    let MessageBody::Stream(s) = &m.body else {
                        return out;
                    };
                    let watch = Watching {
                        chat,
                        name: self.name_of(&m.author, chat),
                        title: s.title.clone(),
                        watching: s.watching,
                        muted: false,
                    };
                    self.s().live.watch = Some(watch);
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            ChatCommand::StopWatch => self.stop_watch(&mut out),
            ChatCommand::WatchMute(on) => {
                if let Some(w) = &mut self.s().live.watch {
                    w.muted = on;
                }
            }
            ChatCommand::MarkRead(chat) => {
                if let Some(h) = self.histories.get_mut(&chat) {
                    for m in h.iter_mut() {
                        m.unread = false;
                    }
                }
            }
            ChatCommand::LoadChat {
                chat,
                before,
                reply_to,
            } => {
                out.push(answer(reply_to, page_of(chat, self.messages(chat), before)));
                return out;
            }
            ChatCommand::NewGroup(name) => {
                self.next += 1;
                let id = self.next;
                self.s().groups.push(ChatGroup {
                    id,
                    name,
                    membership: Membership::Member,
                    members: Some(1),
                    member_list: vec![me_member(id, true)],
                    muted: false,
                    voice: Vec::new(),
                    link: None,
                });
                self.made.insert(ChatRef::Group(id), now);
                self.histories.entry(ChatRef::Group(id)).or_default();
            }
            ChatCommand::AddMember { group, friend } => {
                let s = self.s();
                if let Some(f) = s.friends.iter().find(|f| f.id == friend).cloned()
                    && let Some(g) = s.groups.iter_mut().find(|g| g.id == group)
                    && !g.member_list.iter().any(|m| m.friend == Some(friend))
                {
                    g.member_list.push(group_member(group, &f, false));
                    g.members = Some(g.member_list.len() as u32);
                }
            }
            ChatCommand::JoinGroup(id) => {
                if let Some(g) = self.s().groups.iter_mut().find(|g| g.id == id) {
                    g.membership = Membership::Member;
                }
            }
            ChatCommand::LeaveGroup(id) => {
                let chat = ChatRef::Group(id);
                if self
                    .snapshot
                    .live
                    .call
                    .as_ref()
                    .is_some_and(|c| c.chat == chat)
                {
                    self.end_call(now, &mut out);
                }
                if self
                    .snapshot
                    .live
                    .watch
                    .as_ref()
                    .is_some_and(|w| w.chat == chat)
                {
                    self.stop_watch(&mut out);
                }
                if self
                    .snapshot
                    .live
                    .share
                    .as_ref()
                    .is_some_and(|w| w.chat == chat)
                {
                    self.stop_share(now, &mut out);
                }
                self.s().groups.retain(|g| g.id != id);
                self.histories.remove(&chat);
            }
            ChatCommand::RemoveFriend(id) => {
                let chat = ChatRef::Direct(id);
                let name = self.snapshot.friend(id).map(|f| f.name.clone());
                if self.snapshot.live.call.as_ref().is_some_and(|c| {
                    c.chat == chat || name.as_ref().is_some_and(|n| c.with.contains(n))
                }) {
                    self.end_call(now, &mut out);
                }
                if self
                    .snapshot
                    .live
                    .watch
                    .as_ref()
                    .is_some_and(|w| w.chat == chat || name.as_ref() == Some(&w.name))
                {
                    self.stop_watch(&mut out);
                }
                if self
                    .snapshot
                    .live
                    .share
                    .as_ref()
                    .is_some_and(|w| w.chat == chat)
                {
                    self.stop_share(now, &mut out);
                }
                let s = self.s();
                s.friends.retain(|f| f.id != id);
                for g in &mut s.groups {
                    g.member_list.retain(|m| m.friend != Some(id));
                    g.members = Some(g.member_list.len() as u32);
                    g.voice.retain(|v| *v != member_id(g.id, id));
                }
                self.histories.remove(&chat);
            }
            ChatCommand::GifSearch { query, reply_to } => {
                out.push(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Gifs(easyplay_core::gifs::demo(&query)),
                });
                return out;
            }
            ChatCommand::GifTrending { reply_to } => {
                out.push(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Gifs(easyplay_core::gifs::demo("")),
                });
                return out;
            }
            ChatCommand::SendGif { chat, gif } => {
                let name = match gif {
                    easyplay_core::gifs::GifPick::Found(g) => g.id,
                    easyplay_core::gifs::GifPick::Saved(p) => p.to_string_lossy().into_owned(),
                };
                let m = self.post(chat, MessageAuthor::Me, MessageBody::Gif(name), now);
                out.push(UiEvent::ChatMessages {
                    messages: vec![m],
                    updated: false,
                });
            }
            // No browser in the preview; GifPreview never arrives here.
            ChatCommand::OpenLinkAnyway(_) => {}
            // Commands added later reach the preview as nothing.
            #[allow(unreachable_patterns)]
            // Round 3: fetching is at once in the preview; opening does nothing.
            ChatCommand::ReceiveFile { chat, item } => {
                if let Some(m) = self.change(chat, item, |m| {
                    if let MessageBody::Image(md) | MessageBody::Video(md) | MessageBody::File(md) =
                        &mut m.body
                    {
                        md.state = FileState::Received(
                            std::path::Path::new("/nonexistent/easyplay-preview").join(&md.name),
                        );
                    }
                }) {
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            ChatCommand::CancelFile { chat, item } => {
                if let Some(m) = self.change(chat, item, |m| {
                    if let MessageBody::Image(md) | MessageBody::Video(md) | MessageBody::File(md) =
                        &mut m.body
                    {
                        md.state = FileState::Cancelled;
                    }
                }) {
                    out.push(UiEvent::ChatMessages {
                        messages: vec![m],
                        updated: true,
                    });
                }
            }
            _ => {}
        }
        self.relist();
        out.push(UiEvent::ChatChanged);
        out
    }

    /// Call a friend, or join a group's voice (chat.html `startCall`).
    fn call(&mut self, chat: ChatRef, now: Timestamp) {
        if self.snapshot.live.call.is_some() {
            return;
        }
        match chat {
            ChatRef::Direct(f) => {
                let Some(friend) = self.snapshot.friend(f).filter(|f| !f.blocked) else {
                    return;
                };
                let name = friend.name.clone();
                self.s().live.call = Some(Call {
                    chat,
                    since: None,
                    with: vec![name],
                    speaking: Vec::new(),
                    muted: false,
                    deafened: false,
                });
                self.schedule(
                    Timestamp::from_millis(now.as_millis() + ANSWER_MS),
                    Timer::Answer,
                );
            }
            ChatRef::Group(g) => {
                let Some(gr) = self.snapshot.group(g) else {
                    return;
                };
                let with = gr
                    .voice
                    .iter()
                    .filter(|v| **v != my_member_id(g))
                    .filter_map(|v| gr.member_list.iter().find(|m| m.id == *v))
                    .map(|m| m.name.clone())
                    .collect();
                if let Some(gr) = self.s().groups.iter_mut().find(|x| x.id == g)
                    && !gr.voice.contains(&my_member_id(g))
                {
                    gr.voice.push(my_member_id(g));
                }
                self.s().live.call = Some(Call {
                    chat,
                    since: Some(now),
                    with,
                    speaking: Vec::new(),
                    muted: false,
                    deafened: false,
                });
                self.schedule(
                    Timestamp::from_millis(now.as_millis() + SPEAK_MS),
                    Timer::Speak,
                );
            }
        }
    }
}

/// Add or take back the user's `emoji` on `m` (chat.html `react`).
fn react(m: &mut ChatMessage, emoji: &str, on: bool) {
    match m.reactions.iter().position(|r| r.emoji == emoji) {
        Some(i) => {
            let r = &mut m.reactions[i];
            if r.mine != on {
                r.mine = on;
                if on {
                    r.count += 1;
                } else {
                    r.count = r.count.saturating_sub(1);
                }
            }
            if r.count == 0 {
                m.reactions.remove(i);
            }
        }
        None if on => m.reactions.push(Reaction {
            emoji: emoji.to_owned(),
            count: 1,
            mine: true,
            who: Vec::new(),
        }),
        None => {}
    }
}

fn answer(reply_to: QueryId, page: ChatPage) -> UiEvent {
    UiEvent::QueryResult {
        id: reply_to,
        payload: QueryPayload::Chat(page),
    }
}

/// The stand-in's word for a GIF: the prototype's labels for its twelve, else the name.
pub fn gif_word(name: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(a, _)| a);
    match stem {
        "letsgo" => "LET'S GO".into(),
        "gn" => "GOOD NIGHT".into(),
        "ty" => "THANK YOU".into(),
        other => other.replace(['-', '_'], " ").to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> Timestamp {
        crate::demo::baseline_now()
    }

    fn later(ms: i64) -> Timestamp {
        Timestamp::from_millis(now().as_millis() + ms)
    }

    fn text(m: &ChatMessage) -> &str {
        match &m.body {
            MessageBody::Text(t) => t,
            _ => "",
        }
    }

    #[test]
    fn the_sample_is_chat_htmls() {
        let s = sample(now());
        // Crew 3, Dev 1, Aiko 2; Mod makers is muted with nothing unread.
        assert_eq!(s.unread(), 6);
        let order: Vec<&str> = s.chats.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            order,
            [
                "Friday Crew",
                "Mira Okafor",
                "Dev Patel",
                "Aiko Tanaka",
                "Tomasz Nowak",
                "Lucía Fernández",
                "Mod makers",
                "Jonas Weber",
                "Noor Haddad"
            ]
        );
        // Rex is blocked: his chat is not listed, but kept.
        assert!(s.friend(8).is_some_and(|f| f.blocked));
        assert_eq!(history(REX, now()).len(), 2);
        assert_eq!(s.requests[0].name, "Kit Brennan");
        let crew = s.group(10).unwrap();
        assert_eq!(crew.members, Some(5));
        assert!(
            crew.member_list
                .iter()
                .any(|m| m.owner && m.name == "Jonas Weber")
        );
        assert_eq!(crew.voice, vec![member_id(10, 2), member_id(10, 3)]);
        assert!(
            s.group(11)
                .unwrap()
                .member_list
                .iter()
                .any(|m| m.me && m.owner)
        );
        let jonas = s.friend(2).unwrap();
        assert_eq!(
            jonas.playing,
            Some(Playing {
                title: "Ninefold Arena".into(),
                minutes: 21,
                live: true
            })
        );
        assert_eq!(s.features, ChatFeatures::all());
    }

    #[test]
    fn messages_are_numbered_in_order_and_dated_from_now() {
        let mira = history(MIRA, now());
        assert_eq!(mira.len(), 11);
        assert!(
            mira.windows(2)
                .all(|w| w[0].id < w[1].id && w[0].at <= w[1].at)
        );
        // "so? did it work 👀" answers "nope. third try…".
        let reply = mira[5].reply.as_ref().unwrap();
        assert_eq!(reply.id, Some(mira[1].id));
        assert!(reply.text.starts_with("nope. third try"));
        assert_eq!(mira[10].at, later(-4 * MIN));
        // Yesterday 21:02 UTC.
        assert_eq!(mira[0].at.as_millis() % DAY, (21 * 60 + 2) * MIN);
        assert!(matches!(&mira[2].body, MessageBody::Voice { secs: 14, wave } if wave.len() == 36));
        let crew = history(CREW, now());
        let at_sam = crew.iter().find(|m| text(m).starts_with("@Sam")).unwrap();
        assert_eq!(at_sam.mentions[0].member, my_member_id(10));
        assert!(at_sam.mentions[0].me);
        assert_eq!(crew.iter().filter(|m| m.unread).count(), 3);
        let skill = crew.iter().find(|m| text(m) == "skill issue").unwrap();
        assert_eq!(skill.reactions[0].count, 3);
        assert!(skill.reactions[0].mine);
        assert_eq!(skill.reactions[0].who, vec!["Dev Patel", "Mira Okafor"]);
        let tomasz = history(TOMASZ, now());
        assert_eq!(tomasz[1].delivery, Delivery::Sending);
    }

    #[test]
    fn codes_and_waves_are_deterministic() {
        let a = security_code("Mira Okafor");
        assert_eq!(a, security_code("Mira Okafor"));
        assert_ne!(a, security_code("Lucía Fernández"));
        assert_eq!(a.split(' ').count(), 12);
        assert!(
            a.split(' ')
                .all(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit()))
        );
        // FNV-1a of "a" is 0xE40C292C.
        assert_eq!(hash("a"), 0xE40C_292C);
        assert_eq!(wave(6, 3).len(), 20);
        assert_eq!(wave(30, 1).len(), 56);
    }

    #[test]
    fn sending_editing_reacting_and_deleting() {
        let mut c = DemoChat::new(now());
        let ev = c.handle(
            ChatCommand::Send {
                chat: CREW,
                text: "on my way @Dev".into(),
                reply_to: Some(c.messages(CREW)[0].id),
                mentions: vec![("Dev".into(), member_id(10, 3))],
            },
            now(),
        );
        let UiEvent::ChatMessages {
            messages,
            updated: false,
        } = &ev[0]
        else {
            panic!("{ev:?}")
        };
        let m = &messages[0];
        assert_eq!(m.delivery, Delivery::Delivered);
        assert!(m.reply.is_some() && m.mentions[0].member == member_id(10, 3));
        assert_eq!(c.snapshot().chats[0].chat, CREW);
        let id = m.id;
        c.handle(
            ChatCommand::Edit {
                chat: CREW,
                item: id,
                text: "here".into(),
            },
            now(),
        );
        let m = c.messages(CREW).last().unwrap().clone();
        assert!(m.edited && text(&m) == "here");
        c.handle(
            ChatCommand::React {
                chat: CREW,
                item: id,
                emoji: "🔥".into(),
                on: true,
            },
            now(),
        );
        assert_eq!(c.messages(CREW).last().unwrap().reactions[0].count, 1);
        c.handle(
            ChatCommand::React {
                chat: CREW,
                item: id,
                emoji: "🔥".into(),
                on: false,
            },
            now(),
        );
        assert!(c.messages(CREW).last().unwrap().reactions.is_empty());
        let ev = c.handle(
            ChatCommand::Delete {
                chat: CREW,
                item: id,
            },
            now(),
        );
        assert!(
            matches!(&ev[0], UiEvent::ChatMessages { updated: true, messages } if messages[0].body == MessageBody::Deleted)
        );
        assert_eq!(
            c.snapshot().chats[0].last.as_ref().map(|m| &m.body),
            Some(&MessageBody::Deleted)
        );
    }

    #[test]
    fn blocking_muting_and_requests() {
        let mut c = DemoChat::new(now());
        c.handle(
            ChatCommand::SetBlocked {
                friend: 1,
                on: true,
            },
            now(),
        );
        assert!(c.snapshot().chats.iter().all(|p| p.chat != MIRA));
        assert!(
            c.snapshot()
                .group(10)
                .unwrap()
                .member_list
                .iter()
                .any(|m| m.friend == Some(1) && m.blocked)
        );
        c.handle(
            ChatCommand::SetBlocked {
                friend: 1,
                on: false,
            },
            now(),
        );
        assert!(c.snapshot().chats.iter().any(|p| p.chat == MIRA));
        c.handle(
            ChatCommand::SetMuted {
                chat: CREW,
                on: true,
            },
            now(),
        );
        assert_eq!(c.snapshot().unread(), 3);
        c.handle(ChatCommand::Accept(KIT_REQUEST), now());
        let s = c.snapshot();
        assert!(s.requests.is_empty());
        assert_eq!(s.friend(KIT).map(|f| f.name.as_str()), Some("Kit Brennan"));
        assert_eq!(s.chats[0].chat, ChatRef::Direct(KIT));
        c.handle(ChatCommand::ShowCode(1), now());
        assert_eq!(c.snapshot().code, Some((1, security_code("Mira Okafor"))));
        c.handle(
            ChatCommand::Verify {
                friend: 4,
                code: String::new(),
            },
            now(),
        );
        assert!(c.snapshot().friend(4).unwrap().verified);
        c.handle(ChatCommand::SetStatus(MyStatus::Busy), now());
        let p = c.snapshot().profile.clone().unwrap();
        assert_eq!(
            (p.status, p.about.as_str()),
            (MyStatus::Busy, "Mostly co-op, sometimes ranked.")
        );
        c.handle(ChatCommand::NewGroup("Raid".into()), now());
        let gid = c
            .snapshot()
            .groups
            .iter()
            .find(|g| g.name == "Raid")
            .unwrap()
            .id;
        c.handle(
            ChatCommand::AddMember {
                group: gid,
                friend: 3,
            },
            now(),
        );
        assert_eq!(c.snapshot().group(gid).unwrap().members, Some(2));
        c.handle(ChatCommand::LeaveGroup(gid), now());
        assert!(c.snapshot().group(gid).is_none());
        c.handle(ChatCommand::RemoveFriend(3), now());
        assert!(
            c.snapshot()
                .group(10)
                .unwrap()
                .member_list
                .iter()
                .all(|m| m.friend != Some(3))
        );
        assert_eq!(
            c.snapshot().group(10).unwrap().voice,
            vec![member_id(10, 2)]
        );
    }

    #[test]
    fn a_call_rings_goes_live_and_ends_with_a_record() {
        let mut c = DemoChat::new(now());
        c.handle(ChatCommand::Call(MIRA), now());
        assert_eq!(c.snapshot().live.call.as_ref().unwrap().since, None);
        assert_eq!(c.next_due(), Some(later(ANSWER_MS)));
        assert!(c.tick(later(ANSWER_MS - 1)).is_empty());
        c.tick(later(ANSWER_MS));
        assert_eq!(
            c.snapshot().live.call.as_ref().unwrap().since,
            Some(later(ANSWER_MS))
        );
        let mut spoke = false;
        for i in 1..12 {
            c.tick(later(ANSWER_MS + i * SPEAK_MS));
            spoke |= !c.snapshot().live.call.as_ref().unwrap().speaking.is_empty();
        }
        assert!(spoke);
        c.handle(ChatCommand::CallMute(true), now());
        assert!(c.snapshot().live.call.as_ref().unwrap().muted);
        let ev = c.handle(ChatCommand::EndCall, later(ANSWER_MS + 3 * 60_000));
        assert!(c.snapshot().live.call.is_none());
        assert_eq!(c.next_due(), None);
        assert!(matches!(&ev[0], UiEvent::ChatMessages { messages, .. }
            if messages[0].body == MessageBody::Call(CallRecord { kind: CallKind::Ended, minutes: 3 })));
        // A group's voice: live at once, the user joins it and leaves it.
        c.handle(ChatCommand::Call(CREW), now());
        let call = c.snapshot().live.call.clone().unwrap();
        assert!(call.since.is_some());
        assert_eq!(call.with, vec!["Jonas Weber", "Dev Patel"]);
        assert!(
            c.snapshot()
                .group(10)
                .unwrap()
                .voice
                .contains(&my_member_id(10))
        );
        c.handle(ChatCommand::EndCall, now());
        assert!(
            !c.snapshot()
                .group(10)
                .unwrap()
                .voice
                .contains(&my_member_id(10))
        );
    }

    #[test]
    fn incoming_calls_are_answered_declined_or_missed() {
        let mut c = DemoChat::new(now());
        c.ring_in(DEV, now());
        assert!(c.snapshot().live.ringing.is_some());
        c.handle(ChatCommand::Answer, now());
        assert!(c.snapshot().live.ringing.is_none());
        assert_eq!(
            c.snapshot().live.call.as_ref().unwrap().with,
            vec!["Dev Patel"]
        );
        c.handle(ChatCommand::EndCall, now());
        c.ring_in(DEV, now());
        c.handle(ChatCommand::Decline, now());
        assert!(matches!(
            c.messages(DEV).last().unwrap().body,
            MessageBody::Call(CallRecord {
                kind: CallKind::Declined,
                ..
            })
        ));
        c.ring_in(AIKO, now());
        c.tick(later(RING_MS));
        assert!(c.snapshot().live.ringing.is_none());
        let last = c.messages(AIKO).last().unwrap();
        assert!(matches!(
            last.body,
            MessageBody::Call(CallRecord {
                kind: CallKind::Missed,
                ..
            })
        ));
        assert!(last.unread);
    }

    #[test]
    fn sharing_and_watching() {
        let mut c = DemoChat::new(now());
        // Nothing running: nothing to share.
        c.handle(ChatCommand::StartShare(MIRA), now());
        assert!(c.snapshot().live.share.is_none());
        c.set_running(Some("Lantern Keep".into()));
        c.handle(ChatCommand::StartShare(MIRA), now());
        assert_eq!(c.snapshot().live.share.as_ref().unwrap().watching, 0);
        let ev = c.tick(later(VIEWER_MS));
        assert!(
            ev.iter()
                .any(|e| matches!(e, UiEvent::ChatMessages { updated: true, .. }))
        );
        assert_eq!(c.snapshot().live.share.as_ref().unwrap().watching, 1);
        c.handle(ChatCommand::StopShare, later(5 * 60_000));
        assert!(c.snapshot().live.share.is_none());
        assert!(matches!(&c.messages(MIRA).last().unwrap().body,
            MessageBody::Stream(s) if !s.live && s.minutes == 5));
        c.handle(ChatCommand::Watch(CREW), now());
        let w = c.snapshot().live.watch.clone().unwrap();
        assert_eq!(
            (w.name.as_str(), w.title.as_str(), w.watching),
            ("Jonas Weber", "Ninefold Arena", 3)
        );
        c.handle(ChatCommand::WatchMute(true), now());
        assert!(c.snapshot().live.watch.as_ref().unwrap().muted);
        c.handle(ChatCommand::StopWatch, now());
        assert!(c.snapshot().live.watch.is_none());
        assert!(
            c.messages(CREW)
                .iter()
                .any(|m| matches!(&m.body, MessageBody::Stream(s) if s.watching == 2))
        );
        // The game stops: the share ends with it.
        c.handle(ChatCommand::StartShare(CREW), now());
        c.game_stopped(now());
        assert!(c.snapshot().live.share.is_none());
    }
}

//! The Chat page (ADR-0033): turning Chat on, opening a conversation, the composer's keys,
//! adding a friend (invite, copy, connect), requests, a keyboard-only path, and snapshots.
//!
//! The interface's language is process-wide and a rig sets it from its preferences, so
//! every test here takes one lock and puts English back when it ends.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::sync::{Mutex, MutexGuard};

use common::{ChatBackend, Rig, now, rig_chat, settle, settled_library};
use easyplay_core::model::{ChatRef, ChatSnapshot, ChatStatus};
use easyplay_core::service::ChatCommand;
use easyplay_ui::bridge::{FakeBackend, Command, UiCommand};
use easyplay_ui::demo_chat::{self, CREW, INVITE, KIT_REQUEST, MIRA};
use easyplay_ui::i18n::{self, Language};
use easyplay_ui::route::Route;
use easyplay_ui::theme::{Appearance, ThemeChoice};
use egui::accesskit::Role;
use egui::{Key, Modifiers, Vec2};
use egui_kittest::kittest::{NodeT, Queryable};

static LOCK: Mutex<()> = Mutex::new(());

/// Holds the language lock; English again when dropped.
struct English(#[allow(dead_code)] MutexGuard<'static, ()>);

impl Drop for English {
    fn drop(&mut self) {
        i18n::set(Language::English);
    }
}

fn lock() -> English {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    i18n::set(Language::English);
    English(g)
}

const WIDE: Vec2 = Vec2::new(1280.0, 800.0);
const NARROW: Vec2 = Vec2::new(640.0, 480.0);

/// A rig on the Chat page over `chat`, in `language` ("en", "ar").
fn rig(chat: ChatSnapshot, size: Vec2, theme: ThemeChoice, language: &str) -> (Rig, ChatBackend) {
    let fake = FakeBackend::new(settled_library());
    let mut prefs = easyplay_ui::demo::preferences();
    prefs.general.language = language.into();
    prefs.appearance.theme = match theme {
        ThemeChoice::Light => easyplay_core::config::schema::Theme::Light,
        ThemeChoice::HighContrast => easyplay_core::config::schema::Theme::HighContrast,
        _ => easyplay_core::config::schema::Theme::Dark,
    };
    fake.publish_prefs(prefs);
    let backend = ChatBackend::new(fake, chat);
    let appearance = Appearance {
        theme,
        ..Appearance::default()
    };
    let mut r = rig_chat(&backend, Route::Chat, size, appearance);
    settle(&mut r);
    r.fake.clear_sent();
    (r, backend)
}

fn sample() -> ChatSnapshot {
    demo_chat::sample(now())
}

fn chat_sent(r: &Rig) -> Vec<ChatCommand> {
    r.fake
        .sent()
        .into_iter()
        .filter_map(|c| match c {
            UiCommand::Core(Command::Chat(c)) => Some(c),
            _ => None,
        })
        .collect()
}

/// The label of the focused node.
fn focused(r: &Rig) -> Option<String> {
    r.h.query_by(|n| n.is_focused())
        .map(|n| n.accesskit_node().label().unwrap_or_default())
}

fn key(r: &mut Rig, k: Key) {
    r.h.key_press(k);
    r.h.run_steps(6);
}

fn type_text(r: &mut Rig, text: &str) {
    r.h.input_mut().events.push(egui::Event::Text(text.into()));
    r.h.run_steps(3);
}

/// Press `Tab` until the focused control's name starts with `label`.
fn tab_to(r: &mut Rig, label: &str) {
    let mut seen = Vec::new();
    for _ in 0..40 {
        if focused(r).is_some_and(|f| f.starts_with(label)) {
            return;
        }
        key(r, Key::Tab);
        seen.push(focused(r).unwrap_or_default());
    }
    panic!("no control named {label:?} reachable with Tab; visited {seen:#?}");
}

/// The list's row of the conversation whose name starts its label (`sep` after the name:
/// ", " or Arabic "، "). Rows are 56 high and wide; the header's button (46), a member row
/// in the details (42) and a poster under "Playing now" (64 wide) share the start.
fn list_row<'a>(r: &'a Rig, name: &str, sep: &str) -> egui_kittest::Node<'a> {
    let start = format!("{name}{sep}");
    r.h.query_all_by(move |n: &egui_kittest::kittest::AccessKitNode<'_>| {
        n.role() == Role::Button
            && n.label().is_some_and(|l| l.starts_with(&start))
            && n.raw_bounds()
                .is_some_and(|b| b.width() > 150.0 && (50.0..70.0).contains(&b.height()))
    })
    .next()
    .unwrap_or_else(|| panic!("no list row for {name}"))
}

/// Open the conversation whose row starts with `name`, and answer its page.
fn open(r: &mut Rig, name: &str) {
    list_row(r, name, ", ").click();
    r.h.run_steps(3);
}

fn sends_load(cmds: &[ChatCommand], chat: ChatRef) -> bool {
    cmds.iter()
        .any(|c| matches!(c, ChatCommand::LoadChat { chat: c, before: None, .. } if *c == chat))
}

#[test]
fn turning_chat_on_sends_the_name() {
    let _l = lock();
    let (mut r, _) = rig(ChatSnapshot::default(), WIDE, ThemeChoice::Dark, "en");
    r.h.get_by_role_and_label(Role::Heading, "Play together, privately");
    // No name, no switch.
    assert!(
        r.h.get_by_role_and_label(Role::Switch, "Turn on Chat")
            .accesskit_node()
            .is_disabled()
    );
    r.h.get_by_label("Your name").click();
    r.h.run_steps(2);
    type_text(&mut r, "  Sam ");
    r.h.get_by_role_and_label(Role::Switch, "Turn on Chat")
        .click();
    r.h.run_steps(2);
    assert_eq!(
        chat_sent(&r),
        vec![ChatCommand::SetEnabled {
            on: true,
            name: Some("Sam".into())
        }]
    );
}

#[test]
fn opening_a_conversation_loads_it_and_marks_it_read() {
    let _l = lock();
    let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    // Dev has an unread message.
    open(&mut r, "Dev Patel");
    let sent = chat_sent(&r);
    assert!(sends_load(&sent, demo_chat::DEV), "{sent:?}");
    settle(&mut r);
    let sent = chat_sent(&r);
    assert!(
        sent.contains(&ChatCommand::MarkRead(demo_chat::DEV)),
        "{sent:?}"
    );
    // Asked once, read once.
    settle(&mut r);
    r.h.run_steps(5);
    let again = chat_sent(&r);
    assert_eq!(
        again
            .iter()
            .filter(|c| matches!(c, ChatCommand::MarkRead(_)))
            .count(),
        1,
        "{again:?}"
    );
    r.h.get_by_role_and_label(Role::Log, "Messages with Dev Patel");
    r.h.get_by(|n| {
        n.role() == Role::ListItem
            && n.label()
                .is_some_and(|l| l.contains("you up for ranked later?"))
    });
}

#[test]
fn enter_sends_and_shift_enter_breaks_the_line() {
    let _l = lock();
    let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    open(&mut r, "Mira Okafor");
    settle(&mut r);
    // A click on the row leaves nothing focused; typing then goes to the field (composer).
    type_text(&mut r, "see you");
    assert_eq!(focused(&r).as_deref(), Some("Message to Mira Okafor"));
    r.fake.clear_sent();
    r.h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
    r.h.run_steps(2);
    type_text(&mut r, "at nine");
    assert!(chat_sent(&r).is_empty(), "{:?}", chat_sent(&r));
    key(&mut r, Key::Enter);
    assert_eq!(
        chat_sent(&r),
        vec![ChatCommand::Send {
            chat: MIRA,
            text: "see you\nat nine".into(),
            reply_to: None,
            mentions: Vec::new(),
        }]
    );
    // The field is empty again and keeps the focus.
    assert_eq!(focused(&r).as_deref(), Some("Message to Mira Okafor"));
    key(&mut r, Key::Enter);
    assert_eq!(chat_sent(&r).len(), 1);
}

#[test]
fn add_a_friend_makes_an_invite_and_copies_its_link() {
    let _l = lock();
    let (mut r, backend) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    r.h.get_by_label("Add a friend or start a group").click();
    r.h.run_steps(4);
    r.h.get_by_label("Add a friend…").click();
    r.h.run_steps(4);
    assert_eq!(chat_sent(&r), vec![ChatCommand::CreateInvite]);
    let mut s = sample();
    s.invite = Some(INVITE.into());
    backend.publish_chat(s);
    r.h.run_steps(4);
    r.h.get_by_role_and_label(Role::Image, "QR code of your invite");
    // The link is never printed.
    assert!(r.h.query_by_label_contains("easyplay.invalid").is_none());
    r.h.get_by_label("Copy link").click();
    r.h.step();
    let copied: Vec<String> =
        r.h.output()
            .platform_output
            .commands
            .iter()
            .filter_map(|c| match c {
                egui::OutputCommand::CopyText(t) => Some(t.clone()),
                _ => None,
            })
            .collect();
    assert_eq!(copied, vec![INVITE.to_owned()]);
    // Asked once only.
    r.h.run_steps(4);
    assert_eq!(chat_sent(&r), vec![ChatCommand::CreateInvite]);
}

#[test]
fn a_request_is_accepted() {
    let _l = lock();
    let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    r.h.get_by_label("Accept Kit Brennan").click();
    r.h.run_steps(2);
    assert_eq!(chat_sent(&r), vec![ChatCommand::Accept(KIT_REQUEST)]);
}

/// Tab to a conversation, move with the arrows, open it with Enter, write, and in a narrow
/// window go back to the list with Esc.
#[test]
fn keyboard_only() {
    let _l = lock();
    let (mut r, _) = rig(sample(), NARROW, ThemeChoice::Dark, "en");
    tab_to(&mut r, "Friday Crew, ");
    key(&mut r, Key::ArrowDown);
    assert!(
        focused(&r).is_some_and(|f| f.starts_with("Mira Okafor, ")),
        "{:?}",
        focused(&r)
    );
    key(&mut r, Key::ArrowUp);
    key(&mut r, Key::ArrowDown);
    key(&mut r, Key::Enter);
    let sent = chat_sent(&r);
    assert!(sends_load(&sent, MIRA), "{sent:?}");
    settle(&mut r);
    assert_eq!(focused(&r).as_deref(), Some("Message to Mira Okafor"));
    type_text(&mut r, "on my way");
    key(&mut r, Key::Enter);
    assert!(chat_sent(&r).contains(&ChatCommand::Send {
        chat: MIRA,
        text: "on my way".into(),
        reply_to: None,
        mentions: Vec::new(),
    }));
    // Esc goes back to the list, to the conversation's row.
    key(&mut r, Key::Escape);
    assert!(r.h.state().state.chat.open.is_none());
    assert!(
        focused(&r).is_some_and(|f| f.starts_with("Mira Okafor, ")),
        "{:?}",
        focused(&r)
    );
}

/// The conversation's menu: remove a friend, add a member, leave a group (each asked first).
#[test]
fn the_menu_removes_adds_and_leaves() {
    let _l = lock();
    let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    open(&mut r, "Mira Okafor");
    settle(&mut r);
    r.fake.clear_sent();
    let pick = |r: &mut Rig, label: &str| {
        r.h.get_by_label(label).click();
        r.h.run_steps(6);
    };
    // An item of the open menu (the details panel has buttons of the same names).
    let item = |r: &mut Rig, label: &str| {
        r.h.get_by_role_and_label(Role::MenuItem, label).click();
        r.h.run_steps(6);
    };
    pick(&mut r, "More");
    item(&mut r, "Remove friend…");
    r.h.get_by_label("Remove Mira Okafor?");
    pick(&mut r, "Remove");
    // The list then opens the next chat (its page is asked for).
    assert_eq!(
        chat_sent(&r)
            .into_iter()
            .filter(|c| !matches!(c, ChatCommand::LoadChat { .. }))
            .collect::<Vec<_>>(),
        vec![ChatCommand::RemoveFriend(1)]
    );
    assert!(
        r.h.state()
            .state
            .chat
            .open
            .as_ref()
            .is_none_or(|c| c.chat != MIRA)
    );
    settle(&mut r);
    r.fake.clear_sent();
    open(&mut r, "Friday Crew");
    settle(&mut r);
    r.fake.clear_sent();
    // "Invite people…" opens its dialog (the details agent's tests use it).
    pick(&mut r, "More");
    item(&mut r, "Invite people…");
    assert!(
        r.h.state().state.dialogs.front().is_some_and(|(_, d)| matches!(
            d,
            easyplay_ui::vm::dialogs::Dialog::Chat(easyplay_ui::vm::chat::ChatDialog::InviteToGroup(10))
        )),
        "the invite dialog"
    );
    key(&mut r, Key::Escape);
    pick(&mut r, "More");
    item(&mut r, "Leave group…");
    pick(&mut r, "Leave");
    let sent = chat_sent(&r);
    assert_eq!(
        sent.into_iter()
            .filter(|c| !matches!(c, ChatCommand::MarkRead(_) | ChatCommand::LoadChat { .. }))
            .collect::<Vec<_>>(),
        vec![ChatCommand::LeaveGroup(10)]
    );
}

#[test]
fn an_invitation_is_joined_or_declined() {
    use easyplay_core::model::{ChatGroup, ChatPreview, Membership};
    let _l = lock();
    // chat.html has no invitation: this test adds one, "LAN night".
    let mut s = sample();
    s.groups.push(ChatGroup {
        id: 12,
        name: "LAN night".into(),
        membership: Membership::Invited,
        members: Some(6),
        ..ChatGroup::default()
    });
    s.chats.push(ChatPreview {
        chat: ChatRef::Group(12),
        name: "LAN night".into(),
        last: None,
        unread: 0,
        muted: false,
    });
    let (mut r, _) = rig(s, WIDE, ThemeChoice::Dark, "en");
    // Its row is the last of the list: scroll the list to it first.
    r.h.event(egui::Event::PointerMoved(egui::pos2(150.0, 500.0)));
    r.h.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, -100_000.0),
        phase: egui::TouchPhase::Move,
        source: egui::MouseWheelSource::Unknown,
        modifiers: Modifiers::NONE,
    });
    r.h.run_steps(30);
    open(&mut r, "LAN night");
    settle(&mut r);
    // Where the composer goes: who invited you, Join and Decline.
    r.h.get_by_label("You're invited to LAN night");
    r.fake.clear_sent();
    r.h.get_by_label("Join").click();
    r.h.run_steps(2);
    r.h.get_by_label("Decline").click();
    r.h.run_steps(2);
    assert_eq!(
        chat_sent(&r),
        vec![ChatCommand::JoinGroup(12), ChatCommand::LeaveGroup(12)]
    );
}

#[test]
fn starting_unavailable_and_failed_say_so() {
    let _l = lock();
    let mut s = ChatSnapshot {
        status: ChatStatus::Failed(easyplay_core::error::ErrorCode::ChtEngineFailing),
        ..ChatSnapshot::default()
    };
    let (mut r, backend) = rig(s.clone(), WIDE, ThemeChoice::Dark, "en");
    r.h.get_by_label("Try again").click();
    r.h.run_steps(2);
    assert_eq!(
        chat_sent(&r),
        vec![ChatCommand::SetEnabled {
            on: true,
            name: None
        }]
    );
    s.status = ChatStatus::Starting;
    backend.publish_chat(s.clone());
    r.h.run_steps(3);
    r.h.get_by_role_and_label(Role::Status, "Starting…");
    s.status = ChatStatus::Unavailable;
    backend.publish_chat(s);
    r.h.run_steps(3);
    r.h.get_by_role_and_label(Role::Heading, "Chat is not included in this build");
}

/// Scrolled to its top, a conversation asks once for the page before its oldest message,
/// and stops once a page shorter than 50 arrives.
#[test]
fn older_messages_load_at_the_top() {
    use easyplay_core::model::{ChatMessage, Delivery, MessageAuthor, MessageBody};
    use easyplay_core::service::ChatPage;
    use easyplay_ui::bridge::{QueryPayload, UiEvent};
    let _l = lock();
    let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
    open(&mut r, "Mira Okafor");
    let page = |ids: std::ops::Range<i64>| ChatPage {
        chat: MIRA,
        messages: ids
            .map(|id| ChatMessage {
                id,
                chat: MIRA,
                author: MessageAuthor::Friend,
                body: MessageBody::Text(format!("message {id}")),
                at: easyplay_core::clock::Timestamp::from_millis(
                    now().as_millis() - (2000 - id) * 60_000,
                ),
                delivery: Delivery::Received,
                unread: false,
                reply: None,
                reactions: Vec::new(),
                edited: false,
                mentions: Vec::new(),
            })
            .collect(),
    };
    // Answer the page asked for with `ids`; returns the `before` it was asked with.
    let answer = |r: &mut Rig, ids: std::ops::Range<i64>| -> Vec<Option<i64>> {
        let mut asked = Vec::new();
        for c in r.fake.sent() {
            if let UiCommand::Core(Command::Chat(ChatCommand::LoadChat {
                before, reply_to, ..
            })) = c
            {
                asked.push(before);
                r.fake.push_event(UiEvent::QueryResult {
                    id: reply_to,
                    payload: QueryPayload::Chat(page(ids.clone())),
                });
            }
        }
        r.fake.clear_sent();
        asked
    };
    assert_eq!(answer(&mut r, 1000..1050), vec![None]);
    r.h.run_steps(8);
    let to_top = |r: &mut Rig| {
        r.h.event(egui::Event::PointerMoved(egui::pos2(800.0, 400.0)));
        r.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 100_000.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(30);
    };
    to_top(&mut r);
    assert_eq!(answer(&mut r, 990..1000), vec![Some(1000)]);
    r.h.run_steps(8);
    to_top(&mut r);
    assert_eq!(answer(&mut r, 0..0), Vec::<Option<i64>>::new());
    let c = r.h.state().state.chat.open.as_ref().unwrap();
    assert_eq!(c.messages.first().map(|m| m.id), Some(990));
    assert!(c.complete);
}

// ---- Snapshots ----------------------------------------------------------------------------

fn snap_ready(name: &str, theme: ThemeChoice, language: &str) {
    let (mut r, _) = rig(sample(), WIDE, theme, language);
    let sep = if language == "ar" { "، " } else { ", " };
    list_row(&r, "Mira Okafor", sep).click();
    settle(&mut r);
    r.h.snapshot(name);
}

#[test]
fn snapshot_off() {
    let _l = lock();
    let (mut r, _) = rig(ChatSnapshot::default(), WIDE, ThemeChoice::Dark, "en");
    r.h.snapshot("chat_off_dark_1280x800");
}

#[test]
fn snapshot_ready_dark() {
    let _l = lock();
    snap_ready("chat_ready_dark_1280x800", ThemeChoice::Dark, "en");
}

#[test]
fn snapshot_ready_light() {
    let _l = lock();
    snap_ready("chat_ready_light_1280x800", ThemeChoice::Light, "en");
}

#[test]
fn snapshot_ready_high_contrast() {
    let _l = lock();
    snap_ready("chat_ready_hc_1280x800", ThemeChoice::HighContrast, "en");
}

#[test]
fn snapshot_ready_arabic() {
    let _l = lock();
    snap_ready("chat_ready_ar_dark_1280x800", ThemeChoice::Dark, "ar");
}

#[test]
fn snapshot_open_narrow() {
    let _l = lock();
    let (mut r, _) = rig(sample(), NARROW, ThemeChoice::Dark, "en");
    // At 480 high the first row sits under the pinned "you" row: open it with the keys.
    tab_to(&mut r, "Friday Crew, ");
    key(&mut r, Key::Enter);
    settle(&mut r);
    assert!(
        r.h.state()
            .state
            .chat
            .open
            .as_ref()
            .is_some_and(|c| c.chat == CREW)
    );
    r.h.snapshot("chat_open_dark_640x480");
}

// ---- The list column and the page's frame (list agent) ------------------------------------

mod list_tests {
    use super::*;

    const W1440: Vec2 = Vec2::new(1440.0, 900.0);
    const W1080: Vec2 = Vec2::new(1080.0, 800.0);
    const W760: Vec2 = Vec2::new(760.0, 800.0);

    fn has_button(r: &Rig, start: &str) -> bool {
        r.h.query_all_by(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with(start))
        })
        .next()
        .is_some()
    }

    fn has_heading(r: &Rig, text: &str) -> bool {
        r.h.query_by_role_and_label(Role::Heading, text).is_some()
    }

    /// Requests, Playing now and Chats, newest first; a blocked friend's chat is not
    /// listed but counted under "Blocked (n)".
    #[test]
    fn sections_rows_and_blocked() {
        let _l = lock();
        let (r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        for h in ["Requests", "Playing now", "Chats"] {
            assert!(has_heading(&r, h), "no heading {h}");
        }
        assert!(has_button(&r, "Accept Kit Brennan"));
        assert!(has_button(&r, "Mira Okafor playing Lantern Keep"));
        assert!(has_button(&r, "Friday Crew, "));
        assert!(!has_button(&r, "Rex Calloway, "));
        assert!(has_button(&r, "Blocked (1)"));
        // The top bar's count: the unread messages of the unmuted chats.
        let n = sample().unread();
        assert!(n > 0);
        assert!(
            r.h.query_by_label_contains(&format!("{n} unread messages"))
                .is_some()
                || {
                    let chat = r.h.get_by_role_and_label(Role::Button, "Chat");
                    chat.accesskit_node()
                        .description()
                        .is_some_and(|d| d.contains(&n.to_string()))
                }
        );
    }

    /// A search shows "Results" only, and says so when nobody matches.
    #[test]
    fn search_shows_results() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        r.h.key_press_modifiers(Modifiers::COMMAND, Key::F);
        r.h.run_steps(3);
        assert_eq!(focused(&r).as_deref(), Some("Search chats"));
        type_text(&mut r, "mi");
        r.h.run_steps(3);
        assert!(has_heading(&r, "Results"));
        assert!(!has_heading(&r, "Requests"));
        assert!(has_button(&r, "Mira Okafor, "));
        // (The details may list Dev as a member: look for his chat row.)
        assert!(!has_button(&r, "Dev Patel, Online, you up"));
        type_text(&mut r, "zzz");
        assert!(r.h.query_by_label_contains("No one called").is_some());
        // Esc in the field clears it.
        key(&mut r, Key::Escape);
        assert!(r.h.state().state.chat.query.is_empty());
        assert!(has_button(&r, "Dev Patel, Online, you up"));
    }

    /// A Chat that carries none of the extras draws none of their controls.
    #[test]
    fn features_off_hide_their_controls() {
        let _l = lock();
        let mut s = sample();
        s.features = easyplay_core::model::ChatFeatures::default();
        let (r, _) = rig(s, W1440, ThemeChoice::Dark, "en");
        assert!(!has_heading(&r, "Playing now"));
        assert!(!has_button(&r, "Blocked ("));
        assert!(has_button(&r, "Friday Crew, "));
    }

    /// Esc steps back one thing at a time: a reply, then the details.
    #[test]
    fn escape_steps_back() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        settle(&mut r);
        assert!(r.h.state().state.chat.open.is_some());
        assert!(r.h.state().state.chat.details_open());
        r.h.state_mut().state.chat.reply = Some(1);
        key(&mut r, Key::Escape);
        assert_eq!(r.h.state().state.chat.reply, None);
        assert!(r.h.state().state.chat.details_open());
        key(&mut r, Key::Escape);
        assert!(!r.h.state().state.chat.details_open());
        assert_eq!(r.h.state().state.nav.current(), Route::Chat);
    }

    /// A row's menu mutes it; the "you" menu sets the status.
    #[test]
    fn row_and_you_menus() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        r.h.query_all_by(|n| {
            n.role() == Role::Button
                && n.label()
                    .is_some_and(|l| l.starts_with("Dev Patel, Online, you up"))
        })
        .next()
        .unwrap()
        .click_secondary();
        r.h.run_steps(4);
        r.h.query_all_by(|n| {
            n.role() == Role::MenuItemCheckBox && n.label() == Some("Mute notifications".into())
        })
        .next()
        .unwrap()
        .click();
        r.h.run_steps(4);
        assert!(
            chat_sent(&r).contains(&ChatCommand::SetMuted {
                chat: demo_chat::DEV,
                on: true
            }),
            "{:?}",
            chat_sent(&r)
        );
        r.h.key_press(Key::Escape);
        r.h.run_steps(4);
        r.fake.clear_sent();
        r.h.get_by_label("Your status and profile").click();
        r.h.run_steps(4);
        r.h.query_all_by(|n| {
            n.role() == Role::MenuItemCheckBox && n.label() == Some("Away".into())
        })
        .next()
        .unwrap()
        .click();
        r.h.run_steps(4);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::SetStatus(easyplay_core::model::MyStatus::Away)]
        );
    }

    /// The row menu and the "you" menu, with their icons and status dots.
    #[test]
    fn snapshot_menus() {
        let _l = lock();
        let (mut r, _) = rig(read_crew(), W1440, ThemeChoice::Dark, "en");
        settle(&mut r);
        r.h.query_all_by(|n| {
            n.role() == Role::Button
                && n.label()
                    .is_some_and(|l| l.starts_with("Dev Patel, Online, you up"))
        })
        .next()
        .unwrap()
        .click_secondary();
        r.h.run_steps(30);
        r.h.snapshot("chat_list_row_menu_dark_1440x900");
        r.h.key_press(Key::Escape);
        r.h.run_steps(10);
        r.h.get_by_label("Your status and profile").click();
        r.h.run_steps(30);
        r.h.snapshot("chat_list_me_menu_dark_1440x900");
    }

    /// Typing while nothing has focus goes to the open chat's message field.
    #[test]
    fn typing_anywhere_goes_to_the_message_field() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        settle(&mut r);
        r.h.ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
        r.h.run_steps(3);
        type_text(&mut r, "h");
        r.h.run_steps(3);
        let open =
            r.h.state()
                .state
                .chat
                .open
                .as_ref()
                .map(|o| o.chat)
                .unwrap();
        assert!(
            r.h.state()
                .state
                .chat
                .drafts
                .get(&open)
                .is_some_and(|d| d.ends_with('h')),
            "{:?}",
            r.h.state().state.chat.drafts
        );
        assert_eq!(focused(&r).as_deref(), Some("Message to Friday Crew"));
    }

    /// The top bar during a call while a game runs: the call's pill stands left of the
    /// running game's (chat.html TOP BAR), on the Library.
    #[test]
    fn snapshot_top_bar_call() {
        let _l = lock();
        let mut first = true;
        let lib = common::rebuild(&settled_library(), |g| {
            if first && !g.is_steam() {
                first = false;
                g.activity = easyplay_core::model::Activity::Running {
                    since: easyplay_core::clock::Timestamp::from_millis(
                        now().as_millis() - 42 * 60_000,
                    ),
                };
            }
        });
        let fake = FakeBackend::new(lib);
        fake.publish_prefs(easyplay_ui::demo::preferences());
        let mut chat = read_crew();
        chat.live.call = Some(easyplay_core::model::Call {
            chat: MIRA,
            since: Some(easyplay_core::clock::Timestamp::from_millis(
                now().as_millis() - 42_000,
            )),
            with: vec!["Mira Okafor".into()],
            speaking: Vec::new(),
            muted: false,
            deafened: false,
        });
        let backend = ChatBackend::new(fake, chat);
        let mut r = rig_chat(&backend, Route::Library, W1440, Appearance::default());
        settle(&mut r);
        r.h.run_steps(30);
        r.h.snapshot("chat_topbar_call_dark_1440x900");
    }

    /// Under 760 the list and the conversation take turns.
    #[test]
    fn narrow_takes_turns() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W760, ThemeChoice::Dark, "en");
        assert!(r.h.state().state.chat.open.is_none());
        open(&mut r, "Dev Patel");
        settle(&mut r);
        assert!(!has_button(&r, "Friday Crew, "));
        key(&mut r, Key::Escape);
        key(&mut r, Key::Escape);
        assert!(r.h.state().state.chat.open.is_none());
        assert!(has_button(&r, "Friday Crew, "));
    }

    /// Nobody yet: one invitation whose button opens Add a friend.
    #[test]
    fn no_friends_is_an_invitation() {
        let _l = lock();
        let mut s = sample();
        s.friends.clear();
        s.groups.clear();
        s.requests.clear();
        s.chats.clear();
        let (mut r, _) = rig(s, W1440, ThemeChoice::Dark, "en");
        r.h.get_by_role_and_label(Role::Heading, "Play together, privately");
        r.h.get_by_role_and_label(Role::Button, "Add a friend")
            .click();
        r.h.run_steps(4);
        assert!(!r.h.state().state.nav.overlays().is_empty());
    }

    /// The sample with Friday Crew read (the fake does not play `MarkRead`): its row shows
    /// the voice going on as live bars, as in the reference.
    fn read_crew() -> ChatSnapshot {
        let mut s = sample();
        if let Some(c) = s.chats.iter_mut().find(|c| c.chat == CREW) {
            c.unread = 0;
        }
        s
    }

    fn snap(name: &str, size: Vec2, theme: ThemeChoice, language: &str) {
        let (mut r, _) = rig(read_crew(), size, theme, language);
        settle(&mut r);
        r.h.run_steps(30);
        r.h.snapshot(name);
    }

    #[test]
    fn snapshot_list_dark() {
        let _l = lock();
        snap("chat_list_dark_1440x900", W1440, ThemeChoice::Dark, "en");
    }

    #[test]
    fn snapshot_list_1080() {
        let _l = lock();
        snap("chat_list_dark_1080x800", W1080, ThemeChoice::Dark, "en");
    }

    #[test]
    fn snapshot_list_760() {
        let _l = lock();
        snap("chat_list_dark_760x800", W760, ThemeChoice::Dark, "en");
    }

    #[test]
    fn snapshot_list_light() {
        let _l = lock();
        snap("chat_list_light_1440x900", W1440, ThemeChoice::Light, "en");
    }

    #[test]
    fn snapshot_list_high_contrast() {
        let _l = lock();
        snap(
            "chat_list_hc_1440x900",
            W1440,
            ThemeChoice::HighContrast,
            "en",
        );
    }

    #[test]
    fn snapshot_list_arabic() {
        let _l = lock();
        snap("chat_list_ar_dark_1440x900", W1440, ThemeChoice::Dark, "ar");
    }

    #[test]
    fn snapshot_details_over_1080() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1080, ThemeChoice::Dark, "en");
        r.h.state_mut().state.chat.details = Some(true);
        settle(&mut r);
        r.h.run_steps(30);
        r.h.snapshot("chat_list_details_over_dark_1080x800");
    }

    #[test]
    fn snapshot_invitation() {
        let _l = lock();
        let mut s = sample();
        s.friends.clear();
        s.groups.clear();
        s.requests.clear();
        s.chats.clear();
        let (mut r, _) = rig(s, W1440, ThemeChoice::Dark, "en");
        settle(&mut r);
        r.h.snapshot("chat_list_invite_dark_1440x900");
    }

    #[test]
    fn snapshot_off() {
        let _l = lock();
        let (mut r, _) = rig(ChatSnapshot::default(), W1440, ThemeChoice::Dark, "en");
        r.h.run_steps(30);
        r.h.snapshot("chat_list_off_dark_1440x900");
    }
}

// ---- The conversation (conversation agent) -------------------------------------------------

mod conversation_tests {
    use super::*;

    const W1440: Vec2 = Vec2::new(1440.0, 900.0);
    const W760: Vec2 = Vec2::new(760.0, 800.0);

    /// A rig on the conversation `name` at `size`, its messages in.
    fn opened(name: &str, size: Vec2, theme: ThemeChoice, language: &str) -> Rig {
        opened_with(sample(), name, size, theme, language)
    }

    fn opened_with(
        snap: ChatSnapshot,
        name: &str,
        size: Vec2,
        theme: ThemeChoice,
        language: &str,
    ) -> Rig {
        let (mut r, _) = rig(snap, size, theme, language);
        let sep = if language == "ar" { "، " } else { ", " };
        list_row(&r, name, sep).click();
        settle(&mut r);
        r.h.run_steps(10);
        r.fake.clear_sent();
        r
    }

    fn message<'a>(r: &'a Rig, text: &str) -> egui_kittest::Node<'a> {
        let text = text.to_owned();
        r.h.get_by(move |n| {
            n.role() == Role::ListItem && n.label().is_some_and(|l| l.contains(&text))
        })
    }

    fn label(n: &egui_kittest::Node<'_>) -> Option<String> {
        n.accesskit_node().label()
    }

    /// The header says who and what they do; the messages are rows a screen reader reads
    /// with their author and time; the start says what encryption means here.
    #[test]
    fn header_rows_and_the_start() {
        let _l = lock();
        let r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        r.h.get_by(|n| {
            n.role() == Role::Button
                && n.label()
                    .is_some_and(|l| l.starts_with("Mira Okafor, Playing Lantern Keep, 42 min"))
        });
        r.h.get_by_role_and_label(Role::Log, "Messages with Mira Okafor");
        assert!(
            label(&message(&r, "did you get past the lighthouse boss yet?"))
                .is_some_and(|l| l.starts_with("Mira, "))
        );
        // A spoiler is not read out until it is opened.
        assert!(label(&message(&r, "the ending is spoiler by the way")).is_some());
        r.h.get_by_label_contains("This is the start of your chat with Mira.");
        // In the header (the details have their own Call).
        assert!(
            r.h.query_all_by_role_and_label(Role::Button, "Call")
                .any(|n| n
                    .accesskit_node()
                    .raw_bounds()
                    .is_some_and(|b| b.y1 < 120.0 && b.x0 < 1120.0))
        );
        r.h.get_by_label("Share your game");
    }

    /// A reaction pill toggles yours; the tools' quick emoji reacts; R replies.
    #[test]
    fn reactions_and_reply() {
        let _l = lock();
        let mut r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        let pill =
            r.h.get_by(|n| n.label().is_some_and(|l| l.starts_with("🏆, 2 reactions")));
        pill.click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        assert!(
            sent.iter().any(|c| matches!(c, ChatCommand::React { emoji, on: false, chat, .. } if emoji == "🏆" && *chat == MIRA)),
            "{sent:?}"
        );
    }

    /// The messages are one tab stop: arrows move, Enter opens the menu, R replies.
    #[test]
    fn keys_move_reply_and_open_the_menu() {
        let _l = lock();
        let mut r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        tab_to(&mut r, "Mira, ");
        let last = focused(&r).unwrap();
        assert!(last.contains("bell tower"), "{last}");
        key(&mut r, Key::ArrowUp);
        let up = focused(&r).unwrap();
        assert!(up.contains("ending is"), "{up}");
        key(&mut r, Key::R);
        let reply = r.h.state().state.chat.reply;
        assert!(reply.is_some());
        // Enter opens the message's menu.
        tab_to(&mut r, "Mira, ");
        key(&mut r, Key::Enter);
        r.h.get_by_label("Copy text");
        key(&mut r, Key::Escape);
    }

    /// "Delete for everyone" hides the message and offers Undo; nothing goes out until the
    /// toast leaves, and Undo keeps it.
    #[test]
    fn delete_waits_for_undo() {
        let _l = lock();
        let mut r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        tab_to(&mut r, "Mira, ");
        for _ in 0..12 {
            if focused(&r).is_some_and(|f| f.contains("first try with your route")) {
                break;
            }
            key(&mut r, Key::ArrowUp);
        }
        key(&mut r, Key::Enter);
        r.h.get_by_label("Delete for everyone").click();
        r.h.run_steps(4);
        assert!(
            r.h.query_by(|n| n.role() == Role::ListItem
                && n.label()
                    .is_some_and(|l| l.contains("first try with your route")))
                .is_none()
        );
        assert!(chat_sent(&r).is_empty(), "{:?}", chat_sent(&r));
        r.h.get_by_label("Undo").click();
        r.h.run_steps(4);
        message(&r, "first try with your route");
        assert!(r.h.state().state.chat.deleting.is_empty());
        assert!(chat_sent(&r).is_empty(), "{:?}", chat_sent(&r));
    }

    /// A link opens through the link check; a group's mention of you is a chip.
    #[test]
    fn links_and_mentions() {
        let _l = lock();
        let mut r = opened("Friday Crew", W1440, ThemeChoice::Dark, "en");
        r.h.event(egui::Event::PointerMoved(egui::pos2(700.0, 500.0)));
        r.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 100_000.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(30);
        let link = r.h.get_by(|n| {
            n.role() == Role::Link
                && n.label()
                    .is_some_and(|l| l.contains("ninefold.example/patch-4-2"))
        });
        link.click();
        r.h.run_steps(3);
        assert!(
            chat_sent(&r)
                .iter()
                .any(|c| matches!(c, ChatCommand::OpenLink(u) if u.contains("patch-4-2"))),
            "{:?}",
            chat_sent(&r)
        );
        message(&r, "@Sam you joining voice?");
        r.h.get_by_role_and_label(Role::Button, "Watch Jonas");
    }

    /// A message's text can be selected with the mouse and copied.
    #[test]
    fn text_is_selectable_and_copies() {
        let _l = lock();
        let mut r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        let b =
            r.h.get_by(|n| {
                n.role() == Role::Label && n.value().is_some_and(|l| l.contains("bell tower"))
            })
            .accesskit_node()
            .raw_bounds()
            .unwrap();
        let y = ((b.y0 + b.y1) / 2.0) as f32;
        let (x0, x1) = (b.x0 as f32 + 1.0, b.x1 as f32 - 1.0);
        r.h.event(egui::Event::PointerMoved(egui::pos2(x0, y)));
        r.h.event(egui::Event::PointerButton {
            pos: egui::pos2(x0, y),
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(2);
        r.h.event(egui::Event::PointerMoved(egui::pos2(x1, y)));
        r.h.run_steps(2);
        r.h.event(egui::Event::PointerButton {
            pos: egui::pos2(x1, y),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(2);
        r.h.event(egui::Event::Copy);
        r.h.step();
        let copied: Vec<String> =
            r.h.output()
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
        assert!(
            copied.iter().any(|t| t.contains("bell tower")),
            "{copied:?}"
        );
        // Selecting sent nothing.
        assert!(chat_sent(&r).is_empty(), "{:?}", chat_sent(&r));
    }

    /// Scrolling the transcript shows no message tools; a pointer move after scrolling has
    /// stopped shows them on one row, and nothing repaints in a loop meanwhile.
    #[test]
    fn no_tools_while_scrolling() {
        let _l = lock();
        let mut r = opened("Mira Okafor", W1440, ThemeChoice::Dark, "en");
        let tools = |r: &Rig| {
            r.h.query_all_by(|n: &egui_kittest::kittest::AccessKitNode<'_>| {
                n.label().as_deref() == Some("Add reaction")
            })
            .count()
        };
        let at = egui::pos2(700.0, 600.0);
        r.h.event(egui::Event::PointerMoved(at));
        for _ in 0..6 {
            r.h.event(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 40.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            });
            r.h.step();
            assert_eq!(tools(&r), 0, "tools while scrolling");
        }
        // Scrolling has stopped; the pointer has not moved: still quiet, and idle.
        r.h.run();
        assert_eq!(tools(&r), 0, "tools before the pointer moved");
        // Past the grace, the next pointer move lights one row.
        for _ in 0..20 {
            r.h.step();
        }
        r.h.event(egui::Event::PointerMoved(at + egui::vec2(2.0, 0.0)));
        r.h.run_steps(3);
        assert_eq!(tools(&r), 1, "one row's tools after the pointer moved");
    }

    fn bounds(r: &Rig, text: &str) -> Option<egui::Rect> {
        let text = text.to_owned();
        r.h.query_by(move |n| {
            n.role() == Role::ListItem && n.label().is_some_and(|l| l.contains(&text))
        })
        .and_then(|n| n.accesskit_node().raw_bounds())
        .map(|b| {
            egui::Rect::from_min_max(
                egui::pos2(b.x0 as f32, b.y0 as f32),
                egui::pos2(b.x1 as f32, b.y1 as f32),
            )
        })
    }

    /// Opening a chat shows its end at once: from the first frame its page is in, the last
    /// message stands in place and does not move while pictures decode.
    #[test]
    fn opening_a_chat_places_it_without_moving() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        list_row(&r, "Lucía Fernández", ", ").click();
        // Run until the page is in.
        let mut first = None;
        for _ in 0..20 {
            r.h.step();
            common::serve(&mut r);
            if r.h.state().state.chat.open.as_ref().is_some_and(|c| c.loaded) {
                r.h.step();
                first = bounds(&r, "save_slot2.dat");
                break;
            }
        }
        let first = first.expect("the last message on the first frame");
        assert!(first.bottom() <= 900.0 - 60.0 && first.top() > 108.0, "{first:?}");
        for _ in 0..30 {
            std::thread::sleep(std::time::Duration::from_millis(5));
            r.h.step();
            assert_eq!(bounds(&r, "save_slot2.dat"), Some(first));
        }
    }

    /// With unread messages the chat opens at "New", a third of the way down.
    #[test]
    fn opening_with_unread_shows_new() {
        let _l = lock();
        let r = opened("Friday Crew", W1440, ThemeChoice::Dark, "en");
        let new = r
            .h
            .query_all_by(|n: &egui_kittest::kittest::AccessKitNode<'_>| {
                n.role() == Role::Label && n.value().as_deref() == Some("New")
            })
            .next()
            .and_then(|n| n.accesskit_node().raw_bounds())
            .expect("New in view");
        assert!(new.y0 > 108.0 && new.y1 < 840.0, "{new:?}");
    }

    /// Without a feature its controls are not drawn.
    #[test]
    fn features_off_hide_controls() {
        let _l = lock();
        let mut s = sample();
        s.features = Default::default();
        let r = opened_with(s, "Mira Okafor", W1440, ThemeChoice::Dark, "en");
        assert!(r.h.query_by_role_and_label(Role::Button, "Call").is_none());
        assert!(
            r.h.query_by_role_and_label(Role::Button, "Share your game")
                .is_none()
        );
        assert!(
            r.h.query_by(|n| n.label().is_some_and(|l| l.starts_with("🏆, ")))
                .is_none()
        );
    }

    fn snap(name: &str, chat: &str, size: Vec2, theme: ThemeChoice, language: &str) {
        let mut r = opened(chat, size, theme, language);
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(20);
        r.h.snapshot(name);
    }

    #[test]
    fn snapshot_conversation_mira_dark() {
        let _l = lock();
        snap(
            "chat_conversation_mira_dark_1440x900",
            "Mira Okafor",
            W1440,
            ThemeChoice::Dark,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_mira_light() {
        let _l = lock();
        snap(
            "chat_conversation_mira_light_1440x900",
            "Mira Okafor",
            W1440,
            ThemeChoice::Light,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_mira_hc() {
        let _l = lock();
        snap(
            "chat_conversation_mira_hc_1440x900",
            "Mira Okafor",
            W1440,
            ThemeChoice::HighContrast,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_mira_ar() {
        let _l = lock();
        snap(
            "chat_conversation_mira_ar_dark_1440x900",
            "Mira Okafor",
            W1440,
            ThemeChoice::Dark,
            "ar",
        );
    }

    #[test]
    fn snapshot_conversation_mira_narrow() {
        let _l = lock();
        snap(
            "chat_conversation_mira_dark_760x800",
            "Mira Okafor",
            W760,
            ThemeChoice::Dark,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_crew_dark() {
        let _l = lock();
        snap(
            "chat_conversation_crew_dark_1440x900",
            "Friday Crew",
            W1440,
            ThemeChoice::Dark,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_crew_light() {
        let _l = lock();
        snap(
            "chat_conversation_crew_light_1440x900",
            "Friday Crew",
            W1440,
            ThemeChoice::Light,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_crew_hc() {
        let _l = lock();
        snap(
            "chat_conversation_crew_hc_1440x900",
            "Friday Crew",
            W1440,
            ThemeChoice::HighContrast,
            "en",
        );
    }

    #[test]
    fn snapshot_conversation_crew_ar() {
        let _l = lock();
        snap(
            "chat_conversation_crew_ar_dark_1440x900",
            "Friday Crew",
            W1440,
            ThemeChoice::Dark,
            "ar",
        );
    }

    #[test]
    fn snapshot_conversation_crew_narrow() {
        let _l = lock();
        snap(
            "chat_conversation_crew_dark_760x800",
            "Friday Crew",
            W760,
            ThemeChoice::Dark,
            "en",
        );
    }
}

// ---- Settings › Chat (demo agent) ---------------------------------------------------------

mod settings_tab {
    use super::*;
    use easyplay_core::model::ChatFeatures;
    use easyplay_ui::route::SettingsTab;

    const SIZE: Vec2 = Vec2::new(1440.0, 900.0);
    const TALL: Vec2 = Vec2::new(1440.0, 2200.0);

    /// A rig on Settings › Chat over `chat`.
    fn rig_settings(chat: ChatSnapshot, size: Vec2, language: &str) -> (Rig, ChatBackend) {
        let fake = FakeBackend::new(settled_library());
        let mut prefs = easyplay_ui::demo::preferences();
        prefs.general.language = language.into();
        prefs.chat.enabled = true;
        prefs.chat.name = "Sam".into();
        fake.publish_prefs(prefs);
        let backend = ChatBackend::new(fake, chat);
        let appearance = Appearance {
            theme: ThemeChoice::Dark,
            ..Appearance::default()
        };
        let mut r = rig_chat(
            &backend,
            Route::Settings(SettingsTab::Chat),
            size,
            appearance,
        );
        settle(&mut r);
        r.fake.clear_sent();
        (r, backend)
    }

    const ROWS: [&str; 18] = [
        "Show what I'm playing",
        "Emoji",
        "Send with",
        "Play GIFs automatically",
        "GIF search",
        "GIF search key",
        "Preview links I send",
        "Check links before opening",
        "Microphone",
        "Speakers",
        "Reduce background noise",
        "Hide my address in calls",
        "Picture",
        "Include the game's sound",
        "Pause while the game is not in front",
        "New messages",
        "Incoming calls",
        "Your name",
    ];

    #[test]
    fn every_row_of_chat_html_is_there_and_links_are_not_checked_by_default() {
        let _l = lock();
        let (mut r, _) = rig_settings(sample(), TALL, "en");
        for label in ROWS {
            assert!(
                r.h.query_all_by(|n| n.label().is_some_and(|l| l.contains(label)))
                    .next()
                    .is_some(),
                "no row {label:?}"
            );
        }
        let check =
            r.h.get_by_role_and_label(Role::Switch, "Check links before opening");
        assert_eq!(
            check.accesskit_node().toggled(),
            Some(egui::accesskit::Toggled::False)
        );
        check.click();
        r.h.run_steps(3);
        let keys: Vec<String> = r
            .fake
            .sent()
            .into_iter()
            .filter_map(|c| match c {
                UiCommand::SetPreference { patch, preview } => {
                    assert!(preview.chat.check_links);
                    Some(patch.keys().map(str::to_owned).collect::<Vec<_>>())
                }
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(keys, ["chat.check_links"]);
    }

    /// What the chat engine cannot do has no row.
    #[test]
    fn rows_follow_what_the_engine_can_do() {
        let _l = lock();
        let s = ChatSnapshot {
            features: ChatFeatures {
                reactions: true,
                ..ChatFeatures::default()
            },
            ..sample()
        };
        let (r, _) = rig_settings(s, TALL, "en");
        for gone in [
            "Show what I'm playing",
            "Play GIFs automatically",
            "GIF search key",
            "Preview links I send",
            "Microphone",
            "Include the game's sound",
            "Incoming calls",
        ] {
            assert!(
                r.h.query_all_by(|n| n.label().is_some_and(|l| l.contains(gone)))
                    .next()
                    .is_none(),
                "{gone:?} shown"
            );
        }
        for kept in [
            "Emoji",
            "Send with",
            "Check links before opening",
            "New messages",
        ] {
            assert!(
                r.h.query_all_by(|n| n.label().is_some_and(|l| l.contains(kept)))
                    .next()
                    .is_some(),
                "{kept:?} missing"
            );
        }
    }

    #[test]
    fn snapshot_settings_dark() {
        let _l = lock();
        let (mut r, _) = rig_settings(sample(), SIZE, "en");
        r.h.snapshot("chat_settings_dark_1440x900");
    }

    #[test]
    fn snapshot_settings_arabic() {
        let _l = lock();
        let (mut r, _) = rig_settings(sample(), SIZE, "ar");
        r.h.snapshot("chat_settings_ar_dark_1440x900");
    }
}

/// GIFs from Klipy and the link check (the services part).
mod services_tests {
    use super::*;
    use easyplay_core::gifs::{self, GifAnswer, GifPick, GifProblem};
    use easyplay_ui::bridge::{QueryPayload, UiEvent};

    const W1440: Vec2 = Vec2::new(1440.0, 900.0);

    /// Mira's conversation with the GIF picker open; returns the trending request's id.
    fn picker(theme: ThemeChoice, search: bool) -> (Rig, Option<easyplay_core::ids::QueryId>) {
        let (mut r, _) = rig(sample(), W1440, theme, "en");
        // chat.html's saved GIFs (GG, LET'S GO, NOPE).
        demo_chat::seed_vm(&mut r.h.state_mut().state.chat);
        if !search {
            let mut prefs = easyplay_ui::demo::preferences();
            prefs.chat.gif_search = false;
            r.fake.publish_prefs(prefs);
            settle(&mut r);
        }
        r.h.get_by(|n| {
            n.role() == Role::Button
                && n.label()
                    .is_some_and(|l| l.starts_with("Mira Okafor, ") && l.contains("bell tower"))
        })
        .click();
        settle(&mut r);
        r.h.run_steps(10);
        r.fake.clear_sent();
        r.h.get_by_role_and_label(Role::Button, "Send a GIF")
            .click();
        r.h.run_steps(4);
        let id = chat_sent(&r).into_iter().find_map(|c| match c {
            ChatCommand::GifTrending { reply_to } => Some(reply_to),
            _ => None,
        });
        (r, id)
    }

    fn answer(r: &mut Rig, id: easyplay_core::ids::QueryId, a: GifAnswer) {
        r.fake.push_event(UiEvent::QueryResult {
            id,
            payload: QueryPayload::Gifs(a),
        });
        r.h.run_steps(6);
    }

    /// Opening asks for trending; Enter searches the typed words at once; a click sends the
    /// GIF and closes the picker.
    #[test]
    fn trending_search_and_send() {
        let _l = lock();
        let (mut r, id) = picker(ThemeChoice::Dark, true);
        let id = id.expect("trending asked when the picker opens");
        answer(&mut r, id, gifs::demo(""));
        r.h.get_by_label_contains("Only your search words go to the GIF service.");
        r.h.get_by_role_and_label(Role::Button, "GIF: clutch");
        r.fake.clear_sent();
        type_text(&mut r, "good");
        key(&mut r, Key::Enter);
        let sent = chat_sent(&r);
        let q = sent
            .iter()
            .find_map(|c| match c {
                ChatCommand::GifSearch { query, reply_to } if query == "good" => Some(*reply_to),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no search in {sent:?}"));
        answer(&mut r, q, gifs::demo("good"));
        r.fake.clear_sent();
        r.h.get_by_role_and_label(Role::Button, "GIF: good night")
            .click();
        r.h.run_steps(4);
        let sent = chat_sent(&r);
        assert!(
            sent.iter().any(|c| matches!(
                c,
                ChatCommand::SendGif { chat, gif: GifPick::Found(g) } if *chat == MIRA && g.id == "gn"
            )),
            "{sent:?}"
        );
        assert!(
            r.h.query_by_label_contains("Only your search words")
                .is_none()
        );
    }

    /// With GIF search off nothing is asked; only saved GIFs show, and the footer says so.
    #[test]
    fn search_off_shows_only_saved_gifs() {
        let _l = lock();
        let (mut r, id) = picker(ThemeChoice::Dark, false);
        assert_eq!(id, None);
        assert!(
            chat_sent(&r)
                .iter()
                .all(|c| !matches!(c, ChatCommand::GifSearch { .. }))
        );
        r.h.get_by_label_contains("Showing the GIFs you saved.");
        r.h.run_steps(2);
        r.h.get_by_role_and_label(Role::Button, "GIF: gg");
        assert!(r.h.query_by_label_contains("GIF: clutch").is_none());
    }

    /// Without a Klipy key the picker says search is unavailable, in one line.
    #[test]
    fn no_key_says_so() {
        let _l = lock();
        let (mut r, id) = picker(ThemeChoice::Dark, true);
        answer(
            &mut r,
            id.expect("trending asked"),
            GifAnswer::problem(String::new(), GifProblem::NoKey),
        );
        r.h.get_by_label_contains("Add a GIF search key in Settings › Chat to search.");
    }

    /// A flagged link asks before it opens.
    #[test]
    fn an_unsafe_link_opens_the_dialog() {
        let _l = lock();
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        r.fake.push_event(UiEvent::ChatUnsafeLink {
            url: "https://evil.example/x".into(),
            host: "evil.example".into(),
            unchecked: false,
        });
        r.h.run_steps(6);
        r.h.get_by_label_contains("This site may be harmful");
    }

    #[test]
    fn snapshot_gif_picker() {
        let _l = lock();
        let (mut r, id) = picker(ThemeChoice::Dark, true);
        answer(&mut r, id.expect("trending asked"), gifs::demo(""));
        r.h.run_steps(20);
        r.h.snapshot("chat_gif_picker_dark_1440x900");
    }
}

// ---- Calls, trays, details and dialogs (details agent) ------------------------------------

mod details_tests {
    use super::*;
    use common::rebuild;
    use easyplay_core::model::{Activity, Call, ChatLive, Ringing, Share, Watching};

    const W1440: Vec2 = Vec2::new(1440.0, 900.0);

    /// A rig over `chat` on `route`, with `running` (a game title) running.
    fn rig_at(
        chat: ChatSnapshot,
        route: Route,
        running: Option<&str>,
        theme: ThemeChoice,
        language: &str,
    ) -> (Rig, ChatBackend) {
        let lib = rebuild(&settled_library(), |g| {
            if Some(&*g.name) == running {
                g.activity = Activity::Running { since: now() };
            }
        });
        let fake = FakeBackend::new(lib);
        let mut prefs = easyplay_ui::demo::preferences();
        prefs.general.language = language.into();
        prefs.appearance.theme = match theme {
            ThemeChoice::Light => easyplay_core::config::schema::Theme::Light,
            _ => easyplay_core::config::schema::Theme::Dark,
        };
        fake.publish_prefs(prefs);
        let backend = ChatBackend::new(fake, chat);
        let appearance = Appearance {
            theme,
            ..Appearance::default()
        };
        let mut r = rig_chat(&backend, route, W1440, appearance);
        settle(&mut r);
        r.fake.clear_sent();
        (r, backend)
    }

    /// The sample with `live` and nothing else going on.
    fn with_live(live: ChatLive) -> ChatSnapshot {
        let mut s = sample();
        s.live = live;
        s
    }

    fn calling_mira() -> ChatLive {
        ChatLive {
            call: Some(Call {
                chat: MIRA,
                since: Some(easyplay_core::clock::Timestamp::from_millis(
                    now().as_millis() - 42_000,
                )),
                with: vec!["Mira Okafor".into()],
                speaking: vec!["Mira Okafor".into()],
                muted: true,
                deafened: false,
            }),
            ..ChatLive::default()
        }
    }

    fn click(r: &mut Rig, label: &str) {
        let owned = label.to_owned();
        let found: Vec<_> =
            r.h.query_all_by(move |n| {
                matches!(
                    n.role(),
                    Role::Button | Role::RadioButton | Role::Switch | Role::CheckBox
                ) && n.label().as_deref() == Some(owned.as_str())
            })
            .collect();
        match found.first() {
            Some(n) => n.click(),
            None => r.h.get_by_label(label).click(),
        }
        r.h.run_steps(6);
    }

    fn open_chat(r: &mut Rig, name: &str, details: bool) {
        let (en, ar) = (format!("{name}, "), format!("{name}، "));
        let rows: Vec<_> =
            r.h.query_all_by(move |n| {
                n.role() == Role::Button
                    && n.label().is_some_and(|l| {
                        // A chat row says name, status, preview and time; a poster or a
                        // member row says less.
                        (l.starts_with(&en) || l.starts_with(&ar))
                            && l.matches(", ").count() + l.matches("، ").count() >= 2
                    })
            })
            .collect();
        rows.first().expect("the chat's row").click();
        r.h.run_steps(3);
        r.h.state_mut().state.chat.details = Some(details);
        settle(r);
        r.fake.clear_sent();
    }

    /// The card says who calls on every page; Answer answers and opens the chat.
    #[test]
    fn an_incoming_call_is_answered_from_the_library() {
        let _l = lock();
        let live = ChatLive {
            ringing: Some(Ringing {
                chat: demo_chat::DEV,
                name: "Dev Patel".into(),
                since: now(),
            }),
            ..ChatLive::default()
        };
        let (mut r, _) = rig_at(
            with_live(live.clone()),
            Route::Library,
            None,
            ThemeChoice::Dark,
            "en",
        );
        r.h.get_by_role_and_label(Role::AlertDialog, "Dev Patel is calling");
        r.h.snapshot("chat_call_incoming_dark_1440x900");
        click(&mut r, "Answer");
        assert_eq!(chat_sent(&r).first(), Some(&ChatCommand::Answer));
        assert_eq!(r.h.state().state.nav.current(), Route::Chat);
        assert_eq!(
            r.h.state().state.chat.open.as_ref().map(|o| o.chat),
            Some(demo_chat::DEV)
        );
        let (mut r, _) = rig_at(
            with_live(live),
            Route::Library,
            None,
            ThemeChoice::Dark,
            "en",
        );
        click(&mut r, "Decline");
        assert_eq!(chat_sent(&r), vec![ChatCommand::Decline]);
    }

    /// In a call, the top bar carries it on other pages: mute, leave, and back to the chat.
    #[test]
    fn the_top_bar_pill_mutes_leaves_and_opens_the_chat() {
        let _l = lock();
        let (mut r, _) = rig_at(
            with_live(calling_mira()),
            Route::Library,
            None,
            ThemeChoice::Dark,
            "en",
        );
        r.h.snapshot("chat_call_pill_dark_1440x900");
        click(&mut r, "Mute your microphone");
        click(&mut r, "Leave call");
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::CallMute(false), ChatCommand::EndCall]
        );
        click(&mut r, "Open Mira");
        assert_eq!(r.h.state().state.nav.current(), Route::Chat);
        settle(&mut r);
        // Not on the call's own conversation.
        assert!(r.h.query_by_label("Open Mira").is_none());
    }

    /// The call tray: its buttons send their commands; sharing asks first.
    #[test]
    fn the_call_tray() {
        let _l = lock();
        let (mut r, _) = rig_at(
            with_live(calling_mira()),
            Route::Chat,
            Some("Lantern Keep"),
            ThemeChoice::Dark,
            "en",
        );
        open_chat(&mut r, "Mira Okafor", false);
        r.h.snapshot("chat_call_tray_dark_1440x900");
        click(&mut r, "Mute your microphone");
        click(&mut r, "Stop hearing the call");
        click(&mut r, "Leave call");
        assert_eq!(
            chat_sent(&r),
            vec![
                ChatCommand::CallMute(false),
                ChatCommand::CallDeafen(true),
                ChatCommand::EndCall
            ]
        );
        // Share your game: a game runs, so the dialog asks.
        click(&mut r, "Share your game");
        r.h.get_by_label("Share Lantern Keep with Mira?");
        r.h.snapshot("chat_dialog_share_dark_1440x900");
        click(&mut r, "Share game");
        assert_eq!(chat_sent(&r).last(), Some(&ChatCommand::StartShare(MIRA)));
    }

    /// Watching: the stage, larger and back with Esc, and Stop watching.
    #[test]
    fn the_stage() {
        let _l = lock();
        let live = ChatLive {
            watch: Some(Watching {
                chat: CREW,
                name: "Jonas Weber".into(),
                title: "Ninefold Arena".into(),
                watching: 3,
                muted: false,
            }),
            ..ChatLive::default()
        };
        let (mut r, _) = rig_at(with_live(live), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Friday Crew", true);
        r.h.get_by_role_and_label(Role::Image, "Jonas, Ninefold Arena");
        r.h.snapshot("chat_call_stage_dark_1440x900");
        click(&mut r, "Larger");
        assert!(r.h.state().state.chat.stage_large);
        r.h.key_press(Key::Escape);
        r.h.run_steps(4);
        assert!(!r.h.state().state.chat.stage_large);
        click(&mut r, "Stop watching");
        assert_eq!(chat_sent(&r), vec![ChatCommand::StopWatch]);
    }

    /// Your own sharing outside a call has its tray with Stop sharing.
    #[test]
    fn your_sharing_tray() {
        let _l = lock();
        let live = ChatLive {
            share: Some(Share {
                chat: MIRA,
                title: "Lantern Keep".into(),
                since: now(),
                watching: 1,
            }),
            ..ChatLive::default()
        };
        let (mut r, _) = rig_at(
            with_live(live),
            Route::Chat,
            Some("Lantern Keep"),
            ThemeChoice::Dark,
            "en",
        );
        open_chat(&mut r, "Mira Okafor", false);
        r.h.get_by_label_contains("1 person watching");
        r.h.get_by_role_and_label(Role::Button, "Stop sharing")
            .click();
        r.h.run_steps(4);
        assert_eq!(chat_sent(&r), vec![ChatCommand::StopShare]);
    }

    /// A friend's details: Playing now, Also plays, the connection; Security checks the code.
    #[test]
    fn a_friends_details_and_the_code_check() {
        let _l = lock();
        // Not verified yet, so the check offers "They match".
        let unverified = || {
            let mut s = sample();
            for f in &mut s.friends {
                if f.id == 1 {
                    f.verified = false;
                }
            }
            s
        };
        let (mut r, backend) = rig_at(unverified(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Mira Okafor", true);
        for h in ["Playing now", "Also plays", "Your connection"] {
            assert!(
                r.h.query_all_by_role_and_label(Role::Heading, h).count() > 0,
                "no heading {h}"
            );
        }
        r.h.snapshot("chat_details_friend_dark_1440x900");
        r.h.get_by(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Security, "))
        })
        .click();
        r.h.run_steps(6);
        assert_eq!(chat_sent(&r), vec![ChatCommand::ShowCode(1)]);
        let mut s = unverified();
        let code = demo_chat::security_code("Mira Okafor");
        s.code = Some((1, code.clone()));
        backend.publish_chat(s);
        settle(&mut r);
        r.h.get_by_label("Check your connection with Mira");
        r.h.snapshot("chat_dialog_verify_dark_1440x900");
        click(&mut r, "They match");
        assert!(chat_sent(&r).contains(&ChatCommand::Verify { friend: 1, code }));
    }

    /// Block asks first, then sends; the toast offers Undo.
    #[test]
    fn blocking_from_the_details() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Mira Okafor", true);
        click(&mut r, "Block Mira…");
        r.h.get_by_label("Block Mira?");
        r.h.snapshot("chat_dialog_block_dark_1440x900");
        r.h.get_by_role_and_label(Role::Button, "Block").click();
        r.h.run_steps(6);
        assert_eq!(
            chat_sent(&r).first(),
            Some(&ChatCommand::SetBlocked {
                friend: 1,
                on: true
            })
        );
        // Their chat closed.
        assert_ne!(
            r.h.state().state.chat.open.as_ref().map(|o| o.chat),
            Some(MIRA)
        );
    }

    /// A group's details list its members, online first, with the owner marked.
    #[test]
    fn a_groups_details() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Friday Crew", true);
        r.h.get_by_role_and_label(Role::Heading, "Members");
        r.h.get_by_label_contains("Owner");
        r.h.snapshot("chat_details_group_dark_1440x900");
    }

    #[test]
    fn details_in_light() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Light, "en");
        open_chat(&mut r, "Mira Okafor", true);
        r.h.snapshot("chat_details_friend_light_1440x900");
    }

    #[test]
    fn details_in_arabic() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "ar");
        open_chat(&mut r, "Mira Okafor", true);
        r.h.snapshot("chat_details_friend_ar_dark_1440x900");
    }

    /// Add a friend: the link is a QR code and Copy link, never printed; "I have a link"
    /// connects.
    #[test]
    fn add_a_friend_has_two_tabs() {
        let _l = lock();
        let mut s = sample();
        s.invite = Some(INVITE.into());
        let (mut r, _) = rig_at(s, Route::Chat, None, ThemeChoice::Dark, "en");
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::AddFriend,
            ));
        settle(&mut r);
        assert!(r.h.query_by_label_contains("easyplay.invalid").is_none());
        r.h.snapshot("chat_dialog_add_friend_dark_1440x900");
        click(&mut r, "I have a link");
        settle(&mut r);
        r.h.get_by_role_and_label(Role::TextInput, "Invitation link")
            .click();
        r.h.run_steps(2);
        type_text(&mut r, "https://friend.invalid/invite#abc");
        click(&mut r, "Connect");
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::Connect(
                "https://friend.invalid/invite#abc".into()
            )]
        );
    }

    /// New group: Create is off until a friend is ticked; the friends are added once the
    /// group exists.
    #[test]
    fn new_group_adds_the_ticked_friends() {
        let _l = lock();
        let (mut r, backend) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::NewGroup,
            ));
        settle(&mut r);
        assert!(
            r.h.get_by_role_and_label(Role::Button, "Create group")
                .accesskit_node()
                .is_disabled()
        );
        type_text(&mut r, "Raid");
        r.h.get_by_role_and_label(Role::CheckBox, "Mira Okafor")
            .click();
        r.h.run_steps(4);
        r.h.snapshot("chat_dialog_new_group_dark_1440x900");
        click(&mut r, "Create group");
        assert_eq!(chat_sent(&r), vec![ChatCommand::NewGroup("Raid".into())]);
        r.fake.clear_sent();
        let mut s = sample();
        s.groups.push(easyplay_core::model::ChatGroup {
            id: 99,
            name: "Raid".into(),
            ..Default::default()
        });
        backend.publish_chat(s);
        settle(&mut r);
        assert!(chat_sent(&r).contains(&ChatCommand::AddMember {
            group: 99,
            friend: 1
        }));
    }

    /// Your profile: Change colour steps the pair; Save sends the whole profile.
    #[test]
    fn your_profile() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::Profile,
            ));
        settle(&mut r);
        r.h.snapshot("chat_dialog_profile_dark_1440x900");
        click(&mut r, "Change colour");
        click(&mut r, "Save");
        let sent = chat_sent(&r);
        let before = sample().profile.unwrap();
        match sent.as_slice() {
            [ChatCommand::SetProfile(p)] => {
                assert_eq!(p.name, before.name);
                assert_eq!(p.status, before.status);
                assert_ne!(p.paper, before.paper);
            }
            other => panic!("{other:?}"),
        }
    }

    /// Nothing runs: Start a game offers recent games; a click starts one, and sharing
    /// begins once it runs.
    #[test]
    fn start_a_game_to_share_it() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Mira Okafor", true);
        click(&mut r, "Share game");
        r.h.get_by_label("Start a game to share it");
        r.h.snapshot("chat_dialog_start_game_dark_1440x900");
        r.h.query_all_by(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Play and share "))
        })
        .next()
        .expect("a game to start")
        .click();
        r.h.run_steps(6);
        assert!(
            r.fake
                .sent()
                .iter()
                .any(|c| matches!(c, UiCommand::Launch { .. })),
            "{:?}",
            r.fake.sent()
        );
        let pending = r.h.state().state.chat.pending_share.expect("a share waits");
        // The game starts, then runs: sharing begins, once.
        let lib = settled_library();
        let started = |activity: Activity| {
            rebuild(&lib, |g| {
                if g.id == pending.game {
                    g.activity = activity;
                }
            })
        };
        r.fake
            .publish_library(started(Activity::Running { since: now() }));
        r.fake.clear_sent();
        settle(&mut r);
        r.h.run_steps(4);
        assert_eq!(
            chat_sent(&r)
                .into_iter()
                .filter(|c| matches!(c, ChatCommand::StartShare(_)))
                .collect::<Vec<_>>(),
            vec![ChatCommand::StartShare(MIRA)]
        );
        assert!(r.h.state().state.chat.pending_share.is_none());
    }

    /// A share whose game never starts stops waiting.
    #[test]
    fn a_share_whose_game_never_starts_is_dropped() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        let game = settled_library().games[0].id;
        r.h.state_mut().state.chat.pending_share = Some(easyplay_ui::vm::chat::PendingShare {
            chat: MIRA,
            game,
            started: false,
            asked: easyplay_core::clock::MonoInstant::ZERO,
        });
        r.clock.advance(std::time::Duration::from_secs(
            easyplay_ui::vm::chat::WAIT_SECS + 1,
        ));
        r.h.run_steps(4);
        assert!(r.h.state().state.chat.pending_share.is_none());
        assert!(
            !chat_sent(&r)
                .iter()
                .any(|c| matches!(c, ChatCommand::StartShare(_)))
        );
    }

    /// With none of the extras the real engine may lack, none of their controls is drawn.
    #[test]
    fn features_that_are_off_draw_nothing() {
        let _l = lock();
        let mut s = with_live(ChatLive {
            call: calling_mira().call,
            watch: Some(Watching {
                chat: MIRA,
                name: "Mira Okafor".into(),
                title: "Lantern Keep".into(),
                watching: 2,
                muted: false,
            }),
            ..ChatLive::default()
        });
        s.features = easyplay_core::model::ChatFeatures::default();
        let (mut r, _) = rig_at(s.clone(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Mira Okafor", true);
        let gone = |r: &Rig, label: &str| {
            assert!(
                r.h.query_all_by_label(label).next().is_none(),
                "{label} is drawn with its feature off"
            );
        };
        for label in [
            "Call",
            "Share game",
            "Watch",
            "Mute notifications",
            "Block Mira…",
            "Leave call",
            "Mute your microphone",
            "Stop watching",
        ] {
            gone(&r, label);
        }
        assert!(
            r.h.query_all_by(|n| n.label().is_some_and(|l| l.starts_with("Security, ")))
                .next()
                .is_none()
        );
        let (mut r, _) = rig_at(s, Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Friday Crew", true);
        for label in ["Join voice", "Start voice", "Join", "Mute notifications"] {
            gone(&r, label);
        }
    }

    /// The blocked people, each with Unblock.
    #[test]
    fn the_blocked_list() {
        let _l = lock();
        let (mut r, _) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::Blocked,
            ));
        settle(&mut r);
        r.h.snapshot("chat_dialog_blocked_dark_1440x900");
        r.h.get_by(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with("Unblock "))
        })
        .click();
        r.h.run_steps(6);
        assert_eq!(
            chat_sent(&r).first(),
            Some(&ChatCommand::SetBlocked {
                friend: 8,
                on: false
            })
        );
    }

    #[test]
    fn add_friend_in_arabic() {
        let _l = lock();
        let mut s = sample();
        s.invite = Some(INVITE.into());
        let (mut r, _) = rig_at(s, Route::Chat, None, ThemeChoice::Dark, "ar");
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::AddFriend,
            ));
        settle(&mut r);
        r.h.snapshot("chat_dialog_add_friend_ar_dark_1440x900");
    }

    /// Invite to a group: friends already in it are ticked and off; the ticked ones are
    /// added; the group's own link is made, copied (never printed) and deleted.
    #[test]
    fn invite_to_a_group() {
        let _l = lock();
        let (mut r, backend) = rig_at(sample(), Route::Chat, None, ThemeChoice::Dark, "en");
        open_chat(&mut r, "Friday Crew", true);
        click(&mut r, "Invite");
        settle(&mut r);
        r.h.get_by_label("Invite to Friday Crew");
        let mira = r.h.get_by_role_and_label(Role::CheckBox, "Mira Okafor");
        assert!(mira.accesskit_node().is_disabled());
        assert!(
            r.h.query_all_by_role_and_label(Role::Button, "Invite")
                .last()
                .expect("the dialog's Invite")
                .accesskit_node()
                .is_disabled()
        );
        r.h.get_by_role_and_label(Role::CheckBox, "Aiko Tanaka")
            .click();
        r.h.run_steps(4);
        r.h.snapshot("chat_dialog_invite_group_dark_1440x900");
        r.h.query_all_by_role_and_label(Role::Button, "Invite")
            .last()
            .expect("the dialog's Invite")
            .click();
        r.h.run_steps(6);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::AddMember {
                group: 10,
                friend: 4
            }]
        );
        // The link tab.
        r.fake.clear_sent();
        r.h.state_mut()
            .state
            .show_dialog(easyplay_ui::vm::dialogs::Dialog::Chat(
                easyplay_ui::vm::chat::ChatDialog::InviteToGroup(10),
            ));
        settle(&mut r);
        click(&mut r, "Share a link");
        settle(&mut r);
        click(&mut r, "Make a link");
        assert_eq!(chat_sent(&r), vec![ChatCommand::CreateGroupLink(10)]);
        r.fake.clear_sent();
        let mut s = sample();
        let link = "https://smp14.simplex.im/g#preview-friday-crew";
        for g in &mut s.groups {
            if g.id == 10 {
                g.link = Some(link.into());
            }
        }
        backend.publish_chat(s);
        settle(&mut r);
        r.h.get_by_role_and_label(Role::Image, "QR code of the group's link");
        r.h.get_by_label_contains("Anyone with this link can ask to join Friday Crew.");
        assert!(r.h.query_by_label_contains("preview-friday-crew").is_none());
        // The user's own invite is not here.
        assert!(
            r.h.query_by_role_and_label(Role::Image, "QR code of your invite")
                .is_none()
        );
        click(&mut r, "Copy link");
        let copied: Vec<String> =
            r.h.output()
                .platform_output
                .commands
                .iter()
                .filter_map(|c| match c {
                    egui::OutputCommand::CopyText(t) => Some(t.clone()),
                    _ => None,
                })
                .collect();
        let _ = copied;
        click(&mut r, "Delete link");
        assert_eq!(chat_sent(&r), vec![ChatCommand::DeleteGroupLink(10)]);
    }
}

// ---- The composer and the emoji picker (composer agent) ----------------------------------

mod composer_tests {
    use super::*;
    use easyplay_core::model::{MessageAuthor, MessageBody};
    use easyplay_ui::vm::chat::{Picker, SuggestKind};

    /// Open the conversation whose list row starts with `name` (the first such button: the
    /// row comes before the open conversation's header).
    fn open_row(r: &mut Rig, name: &str) {
        let sep = if i18n::language() == Language::Arabic {
            "، "
        } else {
            ", "
        };
        let prefix = format!("{name}{sep}");
        r.h.query_all_by(|n| {
            n.role() == Role::Button && n.label().is_some_and(|l| l.starts_with(&prefix))
        })
        .next()
        .expect("a row")
        .click();
        r.h.run_steps(3);
    }

    /// The rig with Mira's conversation open and the field focused.
    fn mira(theme: ThemeChoice, language: &str) -> Rig {
        let (mut r, _) = rig(sample(), WIDE, theme, language);
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        // Clicking the row of the open chat leaves focus on the row: give it to the field.
        r.h.state_mut().state.chat.focus_composer = true;
        r.h.run_steps(3);
        assert!(field_focused(&r), "{:?}", focused(&r));
        r.fake.clear_sent();
        r
    }

    fn draft(r: &Rig, chat: ChatRef) -> String {
        r.h.state()
            .state
            .chat
            .drafts
            .get(&chat)
            .cloned()
            .unwrap_or_default()
    }

    fn field_focused(r: &Rig) -> bool {
        focused(r).is_some_and(|f| f.starts_with("Message to ") || f.starts_with("رسالة إلى "))
    }

    #[test]
    fn colon_suggests_emoji_and_tab_inserts_one() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        type_text(&mut r, "hot :fir");
        let sg = r.h.state().state.chat.suggest.clone().expect("suggestions");
        assert_eq!((sg.kind, sg.query.as_str()), (SuggestKind::Emoji, "fir"));
        key(&mut r, Key::Tab);
        assert_eq!(draft(&r, MIRA), "hot 🔥 ");
        assert!(field_focused(&r), "{:?}", focused(&r));
        assert!(r.h.state().state.chat.suggest.is_none());
        assert_eq!(r.h.state().state.chat.freq.get("🔥"), Some(&1));
        // Esc dismisses the list and keeps the text.
        type_text(&mut r, ":smi");
        assert!(r.h.state().state.chat.suggest.is_some());
        key(&mut r, Key::Escape);
        assert!(r.h.state().state.chat.suggest.is_none());
        assert_eq!(draft(&r, MIRA), "hot 🔥 :smi");
    }

    #[test]
    fn up_in_an_empty_field_edits_the_last_message() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        let (item, text) = {
            let c = r.h.state().state.chat.open.clone().unwrap();
            c.messages
                .iter()
                .rev()
                .find_map(|m| match (&m.author, &m.body) {
                    (MessageAuthor::Me, MessageBody::Text(t)) => Some((m.id, t.clone())),
                    _ => None,
                })
                .expect("a message of mine")
        };
        key(&mut r, Key::ArrowUp);
        assert_eq!(r.h.state().state.chat.edit, Some(item));
        assert_eq!(draft(&r, MIRA), text);
        assert!(
            r.h.query_by_label_contains("Editing your message")
                .is_some()
        );
        type_text(&mut r, "!");
        key(&mut r, Key::Enter);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::Edit {
                chat: MIRA,
                item,
                text: format!("{text}!"),
            }]
        );
        assert_eq!(r.h.state().state.chat.edit, None);
        assert!(draft(&r, MIRA).is_empty());
    }

    #[test]
    fn replies_carry_the_message_and_esc_cancels() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        let item = r.h.state().state.chat.open.as_ref().unwrap().messages[0].id;
        r.h.state_mut().state.chat.reply = Some(item);
        r.h.run_steps(2);
        assert!(r.h.query_by_label_contains("Replying to").is_some());
        key(&mut r, Key::Escape);
        assert_eq!(r.h.state().state.chat.reply, None);
        r.h.state_mut().state.chat.reply = Some(item);
        r.h.state_mut().state.chat.focus_composer = true;
        r.h.run_steps(2);
        type_text(&mut r, "yes");
        key(&mut r, Key::Enter);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::Send {
                chat: MIRA,
                text: "yes".into(),
                reply_to: Some(item),
                mentions: Vec::new(),
            }]
        );
    }

    #[test]
    fn at_suggests_members_in_groups_and_sends_mentions() {
        let _l = lock();
        let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Friday Crew");
        settle(&mut r);
        r.fake.clear_sent();
        type_text(&mut r, "@jo");
        let sg = r.h.state().state.chat.suggest.clone().expect("suggestions");
        assert_eq!(sg.kind, SuggestKind::Mention);
        key(&mut r, Key::Enter);
        assert_eq!(draft(&r, CREW), "@'Jonas Weber' ");
        type_text(&mut r, "gg");
        key(&mut r, Key::Enter);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::Send {
                chat: CREW,
                text: "@'Jonas Weber' gg".into(),
                reply_to: None,
                mentions: vec![("Jonas Weber".into(), demo_chat::member_id(10, 2))],
            }]
        );
    }

    #[test]
    fn formatting_keys_wrap_the_selection() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        r.h.key_press_modifiers(Modifiers::COMMAND, Key::B);
        r.h.run_steps(3);
        type_text(&mut r, "loud");
        assert_eq!(draft(&r, MIRA), "*loud*");
        r.h.get_by_label("Formatting").click();
        r.h.run_steps(3);
        assert!(r.h.state().state.chat.format_bar);
        r.h.get_by_label("Code").click();
        r.h.run_steps(3);
        assert!(draft(&r, MIRA).contains("``"), "{}", draft(&r, MIRA));
        // The colour: a menu of the six; blue is `!3 …!`.
        r.h.state_mut()
            .state
            .chat
            .drafts
            .insert(MIRA, String::new());
        r.h.get_by_label("Colour").click();
        r.h.run_steps(3);
        r.h.get_by_label("Blue").click();
        r.h.run_steps(3);
        assert_eq!(draft(&r, MIRA), "!3 !");
    }

    #[test]
    fn ctrl_enter_sends_when_the_setting_says_so() {
        let _l = lock();
        let fake = FakeBackend::new(settled_library());
        let mut prefs = easyplay_ui::demo::preferences();
        prefs.chat.send_with = easyplay_core::config::schema::ChatSendWith::CtrlEnter;
        fake.publish_prefs(prefs);
        let backend = ChatBackend::new(fake, sample());
        let mut r = rig_chat(&backend, Route::Chat, WIDE, Default::default());
        settle(&mut r);
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        r.fake.clear_sent();
        type_text(&mut r, "one");
        key(&mut r, Key::Enter);
        type_text(&mut r, "two");
        assert!(chat_sent(&r).is_empty());
        assert_eq!(draft(&r, MIRA), "one\ntwo");
        r.h.key_press_modifiers(Modifiers::COMMAND, Key::Enter);
        r.h.run_steps(3);
        assert!(matches!(
            chat_sent(&r).as_slice(),
            [ChatCommand::Send { text, .. }] if text == "one\ntwo"
        ));
    }

    #[test]
    fn recording_sends_a_voice_message_and_esc_discards() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        r.h.get_by_label("Record a voice message").click();
        r.h.run_steps(3);
        assert!(r.h.state().state.chat.recording.is_some());
        key(&mut r, Key::Escape);
        assert!(r.h.state().state.chat.recording.is_none());
        r.h.get_by_label("Record a voice message").click();
        r.h.run_steps(3);
        r.h.get_by_label("Send the voice message").click();
        r.h.run_steps(3);
        assert!(matches!(
            chat_sent(&r).as_slice(),
            [ChatCommand::SendVoice { chat, secs: 1 }] if *chat == MIRA
        ));
    }

    #[test]
    fn without_voice_the_send_arrow_waits_for_text() {
        let _l = lock();
        let mut s = sample();
        s.features.voice = false;
        s.features.gifs = false;
        s.features.files = false;
        let (mut r, _) = rig(s, WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        assert!(r.h.query_by_label("Record a voice message").is_none());
        assert!(r.h.query_by_label("Send a GIF").is_none());
        assert!(r.h.query_by_label("Attach").is_none());
        r.h.get_by_label("Send");
    }

    #[test]
    fn a_blocked_friend_offers_unblock() {
        let _l = lock();
        let (mut r, backend) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        let mut s = sample();
        if let Some(f) = s.friends.iter_mut().find(|f| f.id == 1) {
            f.blocked = true;
        }
        backend.publish_chat(s);
        r.h.run_steps(6);
        r.fake.clear_sent();
        assert!(r.h.query_by_label_contains("You blocked Mira.").is_some());
        r.h.query_all_by_label("Unblock")
            .last()
            .expect("Unblock")
            .click();
        r.h.run_steps(3);
        assert!(chat_sent(&r).contains(&ChatCommand::SetBlocked {
            friend: 1,
            on: false
        }));
    }

    #[test]
    fn the_emoji_picker_inserts_searches_and_reacts() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        type_text(&mut r, "so ");
        r.h.get_by_label("Emoji").click();
        r.h.run_steps(4);
        assert_eq!(
            r.h.state().state.chat.picker,
            Some(Picker::Emoji { react_to: None })
        );
        assert_eq!(focused(&r).as_deref(), Some("Search emoji"));
        type_text(&mut r, "tada");
        key(&mut r, Key::Enter);
        assert_eq!(r.h.state().state.chat.picker, None);
        assert_eq!(draft(&r, MIRA), "so 🎉");
        assert!(field_focused(&r), "{:?}", focused(&r));
        // ↓ walks into the grid; Enter reacts to the message the picker was opened for.
        let item = r.h.state().state.chat.open.as_ref().unwrap().messages[0].id;
        r.h.state_mut().state.chat.picker = Some(Picker::Emoji {
            react_to: Some(item),
        });
        r.h.run_steps(4);
        key(&mut r, Key::ArrowDown);
        key(&mut r, Key::ArrowRight);
        key(&mut r, Key::Enter);
        let sent = chat_sent(&r);
        assert!(
            matches!(sent.as_slice(), [ChatCommand::React { chat, item: i, on: true, .. }] if *chat == MIRA && *i == item),
            "{sent:?}"
        );
        assert_eq!(r.h.state().state.chat.picker, None);
        // Esc closes it.
        r.h.state_mut().state.chat.picker = Some(Picker::Emoji { react_to: None });
        r.h.run_steps(4);
        key(&mut r, Key::Escape);
        assert_eq!(r.h.state().state.chat.picker, None);
    }

    #[test]
    fn the_reaction_picker_offers_only_what_the_engine_takes() {
        let _l = lock();
        let mut s = sample();
        s.reaction_emoji = Some(vec!["👍".into(), "❤️".into()]);
        let (mut r, _) = rig(s, WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        r.fake.clear_sent();
        let item = r.h.state().state.chat.open.as_ref().unwrap().messages[0].id;
        r.h.state_mut().state.chat.picker = Some(Picker::Emoji {
            react_to: Some(item),
        });
        r.h.run_steps(4);
        assert!(r.h.query_by_label("thumbsup").is_some());
        assert!(r.h.query_by_label("grinning").is_none());
    }

    #[test]
    fn clicking_a_suggestion_inserts_it() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        type_text(&mut r, ":fir");
        r.h.run_steps(2);
        r.h.get_by_label(":fire:").click();
        r.h.run_steps(4);
        assert_eq!(draft(&r, MIRA), "🔥 ");
        assert!(field_focused(&r), "{:?}", focused(&r));
    }

    #[test]
    fn opening_a_chat_focuses_the_field() {
        let _l = lock();
        let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Dev Patel");
        settle(&mut r);
        assert!(field_focused(&r), "{:?}", focused(&r));
        // Another chat: a reply begun in this one does not follow.
        let item = r.h.state().state.chat.open.as_ref().unwrap().messages[0].id;
        r.h.state_mut().state.chat.reply = Some(item);
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        assert_eq!(r.h.state().state.chat.reply, None);
    }

    #[test]
    fn typing_with_nothing_focused_goes_to_the_field() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        r.h.ctx.memory_mut(|m| {
            if let Some(f) = m.focused() {
                m.surrender_focus(f);
            }
        });
        r.h.run_steps(2);
        assert_eq!(focused(&r), None);
        type_text(&mut r, "h");
        type_text(&mut r, "i");
        assert_eq!(draft(&r, MIRA), "hi");
        assert!(field_focused(&r), "{:?}", focused(&r));
    }

    /// A clipboard holding `clip`.
    struct FakeClipboard(easyplay_ui::vm::chat::Clip);

    impl easyplay_ui::vm::chat::ClipboardSource for FakeClipboard {
        fn read(&self) -> easyplay_ui::vm::chat::Clip {
            self.0.clone()
        }
    }

    fn clipboard(r: &mut Rig, clip: easyplay_ui::vm::chat::Clip) {
        r.h.state_mut().state.chat.clipboard =
            easyplay_ui::vm::chat::Clipboard::new(std::sync::Arc::new(FakeClipboard(clip)));
    }

    /// Ctrl+V as the window delivers it.
    fn paste(r: &mut Rig, text: &str) {
        r.h.input_mut().events.push(egui::Event::Paste(text.into()));
        r.h.run_steps(3);
    }

    fn picture() -> easyplay_ui::vm::chat::Clip {
        easyplay_ui::vm::chat::Clip::Image {
            size: [64, 40],
            rgba: [200u8, 90, 60, 255].repeat(64 * 40),
        }
    }

    #[test]
    fn text_on_the_clipboard_pastes_as_text() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        clipboard(&mut r, easyplay_ui::vm::chat::Clip::Other);
        paste(&mut r, "gg wp");
        assert_eq!(draft(&r, MIRA), "gg wp");
        assert!(r.h.state().state.chat.attachments.is_empty());
    }

    #[derive(Debug)]
    struct Dropped(std::path::PathBuf);

    impl egui::DroppedFile for Dropped {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Err(String::new())
        }
    }

    fn drop_files(r: &mut Rig, paths: &[&str]) {
        for path in paths {
            r.h.input_mut()
                .dropped_files
                .push(std::sync::Arc::new(Dropped((*path).into())));
        }
        r.h.run_steps(3);
    }

    fn hover_files(r: &mut Rig, paths: &[&str], steps: usize) {
        for _ in 0..steps {
            r.h.input_mut().hovered_files = paths
                .iter()
                .map(|p| egui::HoveredFile {
                    path: Some((*p).into()),
                    mime: String::new(),
                })
                .collect();
            r.h.step();
        }
    }

    fn attachments(r: &Rig) -> Vec<easyplay_ui::vm::chat::Attachment> {
        r.h.state().state.chat.attachments.clone()
    }

    /// Commands sent, waiting for a worker thread (pictures are encoded off the UI thread).
    fn sent_soon(r: &mut Rig) -> Vec<ChatCommand> {
        for _ in 0..100 {
            let sent = chat_sent(r);
            if !sent.is_empty() {
                return sent;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
            r.h.run_steps(1);
        }
        Vec::new()
    }

    #[test]
    fn chosen_files_wait_with_the_others_and_take_the_caption() {
        use easyplay_ui::input::picker::PickPurpose;
        use easyplay_ui::vm::chat::Attachment;
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        let picture = std::path::PathBuf::from("/tmp/easyplay-composer/small.png");
        let file = std::path::PathBuf::from("/tmp/easyplay-composer/notes.txt");
        r.picker.answer(vec![picture.clone()]);
        r.h.get_by_label("Attach").click();
        r.h.run_steps(3);
        r.h.get_by_label("Picture or video…").click();
        r.h.run_steps(4);
        r.picker.answer(vec![file.clone()]);
        r.h.get_by_label("Attach").click();
        r.h.run_steps(3);
        r.h.get_by_label("File…").click();
        r.h.run_steps(4);
        assert_eq!(
            r.picker.asked(),
            vec![PickPurpose::ChatPicture(MIRA), PickPurpose::ChatFile(MIRA)]
        );
        assert_eq!(
            attachments(&r),
            vec![
                Attachment::File(picture.clone()),
                Attachment::File(file.clone())
            ]
        );
        assert!(chat_sent(&r).is_empty());
        r.h.get_by_label("Remove notes.txt");
        r.h.state_mut().state.chat.focus_composer = true;
        r.h.run_steps(3);
        type_text(&mut r, "the route");
        key(&mut r, Key::Enter);
        assert_eq!(
            chat_sent(&r),
            vec![
                ChatCommand::SendFile {
                    chat: MIRA,
                    path: picture,
                    text: "the route".into(),
                },
                ChatCommand::SendFile {
                    chat: MIRA,
                    path: file,
                    text: String::new(),
                },
            ]
        );
        assert!(attachments(&r).is_empty());
        assert!(draft(&r, MIRA).is_empty());
    }

    #[test]
    fn a_pasted_picture_goes_as_png_with_its_caption() {
        use easyplay_ui::vm::chat::Attachment;
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        clipboard(&mut r, picture());
        paste(&mut r, "");
        assert!(matches!(
            attachments(&r).as_slice(),
            [Attachment::Image { size: [64, 40], .. }]
        ));
        assert!(draft(&r, MIRA).is_empty());
        assert!(chat_sent(&r).is_empty());
        assert!(
            r.h.query_by_label("Remove Picture from the clipboard")
                .is_some()
        );
        type_text(&mut r, "look");
        key(&mut r, Key::Enter);
        match sent_soon(&mut r).as_slice() {
            [
                ChatCommand::SendClipboard {
                    chat,
                    bytes,
                    kind,
                    text,
                },
            ] => {
                assert_eq!(*chat, MIRA);
                assert_eq!(*kind, easyplay_core::service::ClipKind::Png);
                assert!(bytes.starts_with(b"\x89PNG"));
                assert_eq!(text, "look");
            }
            other => panic!("{other:?}"),
        }
        assert!(attachments(&r).is_empty());
        assert!(field_focused(&r), "{:?}", focused(&r));
    }

    #[test]
    fn pasted_files_backspace_esc_and_send() {
        use easyplay_ui::vm::chat::{Attachment, Clip};
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        let files: Vec<std::path::PathBuf> = (0..12)
            .map(|k| format!("/home/sam/Videos/clip-{k}.mp4").into())
            .collect();
        clipboard(&mut r, Clip::Files(files.clone()));
        paste(&mut r, "/home/sam/Videos/clip-0.mp4");
        let want: Vec<Attachment> = files[..10].iter().cloned().map(Attachment::File).collect();
        assert_eq!(attachments(&r), want);
        assert!(draft(&r, MIRA).is_empty(), "{}", draft(&r, MIRA));
        // Backspace in the empty field takes the last one away.
        key(&mut r, Key::Backspace);
        assert_eq!(attachments(&r), want[..9].to_vec());
        // Esc with text keeps them; in the empty field it takes them all away.
        type_text(&mut r, "x");
        key(&mut r, Key::Escape);
        assert_eq!(attachments(&r).len(), 9);
        r.h.state_mut()
            .state
            .chat
            .drafts
            .insert(MIRA, String::new());
        r.h.run_steps(2);
        key(&mut r, Key::Escape);
        assert!(attachments(&r).is_empty());
        assert!(chat_sent(&r).is_empty());
        // Back again, sent with the round Send.
        paste(&mut r, "");
        r.h.get_by_label("Send").click();
        r.h.run_steps(3);
        let sent: Vec<ChatCommand> = files[..10]
            .iter()
            .map(|path| ChatCommand::SendFile {
                chat: MIRA,
                path: path.clone(),
                text: String::new(),
            })
            .collect();
        assert_eq!(chat_sent(&r), sent);
    }

    #[test]
    fn dropped_files_join_the_attachments_and_go_on_enter() {
        use easyplay_ui::vm::chat::Attachment;
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        drop_files(
            &mut r,
            &[
                "/home/sam/Videos/boss.mp4",
                "/home/sam/Games",
                "/home/sam/notes.txt",
            ],
        );
        assert_eq!(
            attachments(&r),
            vec![
                Attachment::File("/home/sam/Videos/boss.mp4".into()),
                Attachment::File("/home/sam/notes.txt".into())
            ]
        );
        // The game-adding drop zone never saw them.
        assert!(
            !r.fake
                .sent()
                .iter()
                .any(|c| matches!(c, UiCommand::Core(Command::OpenPaths { .. })))
        );
        assert!(field_focused(&r), "{:?}", focused(&r));
        r.h.get_by_label("Remove notes.txt").click();
        r.h.run_steps(3);
        assert_eq!(attachments(&r).len(), 1);
        r.h.state_mut().state.chat.focus_composer = true;
        r.h.run_steps(3);
        key(&mut r, Key::Enter);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::SendFile {
                chat: MIRA,
                path: "/home/sam/Videos/boss.mp4".into(),
                text: String::new(),
            }]
        );
    }

    #[test]
    fn only_folders_dropped_say_so() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        drop_files(&mut r, &["/home/sam/Games", "/home/sam/Music"]);
        assert!(r.h.state().state.chat.attachments.is_empty());
        assert!(
            r.h.state()
                .state
                .toasts
                .visible()
                .iter()
                .any(|t| t.text == "Folders can't be sent")
        );
        assert!(
            r.fake
                .sent()
                .iter()
                .all(|c| !matches!(c, UiCommand::Core(Command::OpenPaths { .. })))
        );
    }

    #[test]
    fn drops_on_the_library_still_add_games() {
        let _l = lock();
        let (mut r, _) = rig(sample(), WIDE, ThemeChoice::Dark, "en");
        r.h.state_mut().state.nav.go_to(Route::Library);
        r.h.run_steps(4);
        r.fake.clear_sent();
        drop_files(&mut r, &["/home/sam/Downloads/Game.exe"]);
        assert!(r.fake.sent().iter().any(|c| matches!(
            c,
            UiCommand::Core(Command::OpenPaths { paths, .. }) if paths == &vec![std::path::PathBuf::from("/home/sam/Downloads/Game.exe")]
        )));
        assert!(r.h.state().state.chat.attachments.is_empty());
    }

    // ---- Snapshots

    #[test]
    fn snapshot_drop_overlay() {
        let _l = lock();
        let (mut r, _) = rig(sample(), Vec2::new(1440.0, 900.0), ThemeChoice::Dark, "en");
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        r.h.input_mut().events.push(egui::Event::PointerGone);
        hover_files(&mut r, &["/home/sam/Videos/boss.mp4"], 20);
        r.h.snapshot("chat_drop_overlay_dark_1440x900");
    }

    #[test]
    fn snapshot_composer_paste() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        clipboard(&mut r, picture());
        paste(&mut r, "");
        snapshot(&mut r, "chat_composer_paste_dark_1280x800");
    }

    #[test]
    fn snapshot_composer_attachments() {
        use easyplay_ui::vm::chat::Clip;
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        clipboard(&mut r, picture());
        paste(&mut r, "");
        clipboard(
            &mut r,
            Clip::Image {
                size: [40, 64],
                rgba: [70u8, 130, 200, 255].repeat(40 * 64),
            },
        );
        paste(&mut r, "");
        drop_files(
            &mut r,
            &["/tmp/easyplay-composer/patch notes for the boss fight.txt"],
        );
        r.h.state_mut().state.chat.focus_composer = true;
        r.h.run_steps(3);
        type_text(&mut r, "before and after");
        snapshot(&mut r, "chat_composer_attachments_dark_1280x800");
    }

    #[test]
    fn snapshot_composer_live_format() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        type_text(
            &mut r,
            "*bold* _italic_ ~gone~ `code` #secret# !3 blue! and plain https://example.org",
        );
        snapshot(&mut r, "chat_composer_live_format_dark_1280x800");
    }

    /// The pointer leaves the window, so no hover shows in a snapshot.
    fn snapshot(r: &mut Rig, name: &str) {
        r.h.input_mut().events.push(egui::Event::PointerGone);
        r.h.run_steps(4);
        settle(r);
        r.h.snapshot(name);
    }

    fn composer_snap(name: &str, theme: ThemeChoice, language: &str) {
        let mut r = mira(theme, language);
        let item = r.h.state().state.chat.open.as_ref().unwrap().messages[0].id;
        r.h.state_mut().state.chat.reply = Some(item);
        r.h.state_mut().state.chat.format_bar = true;
        r.h.run_steps(2);
        type_text(&mut r, "parry twice, then *run* 🏃");
        snapshot(&mut r, name);
    }

    #[test]
    fn snapshot_composer_dark() {
        let _l = lock();
        composer_snap("chat_composer_dark_1280x800", ThemeChoice::Dark, "en");
    }

    #[test]
    fn snapshot_composer_light() {
        let _l = lock();
        composer_snap("chat_composer_light_1280x800", ThemeChoice::Light, "en");
    }

    #[test]
    fn snapshot_composer_arabic() {
        let _l = lock();
        composer_snap("chat_composer_ar_dark_1280x800", ThemeChoice::Dark, "ar");
    }

    #[test]
    fn snapshot_composer_suggestions_offline() {
        let _l = lock();
        let mut s = sample();
        s.offline = true;
        let (mut r, _) = rig(s, WIDE, ThemeChoice::Dark, "en");
        open_row(&mut r, "Mira Okafor");
        settle(&mut r);
        type_text(&mut r, "nice :sm");
        snapshot(&mut r, "chat_composer_suggest_offline_dark_1280x800");
    }

    /// Ten lines: the field stops growing at eight and scrolls.
    #[test]
    fn snapshot_composer_long() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        for k in 1..=10 {
            type_text(&mut r, &format!("line {k}"));
            if k < 10 {
                r.h.key_press_modifiers(Modifiers::SHIFT, Key::Enter);
                r.h.run_steps(2);
            }
        }
        snapshot(&mut r, "chat_composer_long_dark_1280x800");
    }

    #[test]
    fn snapshot_composer_recording() {
        let _l = lock();
        let mut r = mira(ThemeChoice::Dark, "en");
        r.h.get_by_label("Record a voice message").click();
        r.h.input_mut().events.push(egui::Event::PointerGone);
        r.h.run_steps(40);
        r.h.snapshot("chat_composer_recording_dark_1280x800");
    }

    fn picker_snap(name: &str, theme: ThemeChoice, language: &str) {
        let mut r = mira(theme, language);
        for e in ["👍", "😂", "❤️", "🔥", "🎉", "😭", "👀", "💀"] {
            for _ in 0..3 {
                r.h.state_mut()
                    .state
                    .chat
                    .freq
                    .entry(e.into())
                    .and_modify(|n| *n += 1)
                    .or_insert(1);
            }
        }
        r.h.state_mut().state.chat.picker = Some(Picker::Emoji { react_to: None });
        r.h.run_steps(2);
        snapshot(&mut r, name);
    }

    #[test]
    fn snapshot_emoji_picker_dark() {
        let _l = lock();
        picker_snap("chat_emoji_picker_dark_1280x800", ThemeChoice::Dark, "en");
    }

    #[test]
    fn snapshot_emoji_picker_light() {
        let _l = lock();
        picker_snap("chat_emoji_picker_light_1280x800", ThemeChoice::Light, "en");
    }

    #[test]
    fn snapshot_emoji_picker_arabic() {
        let _l = lock();
        picker_snap(
            "chat_emoji_picker_ar_dark_1280x800",
            ThemeChoice::Dark,
            "ar",
        );
    }
}

// ---- Pictures, videos and files (conversation agent, round 3) ------------------------------

mod media_tests {
    use super::*;
    use easyplay_core::model::MessageBody;
    use easyplay_ui::demo_chat::LUCIA;

    const W1440: Vec2 = Vec2::new(1440.0, 900.0);

    fn lucia() -> Rig {
        let (mut r, _) = rig(sample(), W1440, ThemeChoice::Dark, "en");
        list_row(&r, "Lucía Fernández", ", ").click();
        settle(&mut r);
        r.h.run_steps(10);
        r.fake.clear_sent();
        r
    }

    /// Answer what was sent with the preview's own answers (the demo plays them).
    fn play(r: &mut Rig, backend_cmds: &[ChatCommand]) {
        let mut demo = demo_chat::DemoChat::new(now());
        let mut events = Vec::new();
        for c in backend_cmds {
            events.extend(demo.handle(c.clone(), now()));
        }
        for e in events {
            r.fake.push_event(e);
        }
        r.h.run_steps(6);
    }

    /// The file row's button (the pictures' round buttons share its words).
    fn row_button<'a>(r: &'a Rig, label: &str) -> egui_kittest::Node<'a> {
        let label = label.to_owned();
        r.h.query_all_by(move |n: &egui_kittest::kittest::AccessKitNode<'_>| {
            n.role() == Role::Button
                && n.label().as_deref() == Some(label.as_str())
                && n.raw_bounds().is_some_and(|b| b.height() < 40.0)
        })
        .next()
        .expect("the file's button")
    }

    fn by_label<'a>(r: &'a Rig, start: &str) -> egui_kittest::Node<'a> {
        let start = start.to_owned();
        r.h.get_by(move |n| n.label().is_some_and(|l| l.starts_with(&start)))
    }

    /// An offered picture downloads; once here, a click opens the viewer, Esc closes it, and
    /// its Open sends OpenFile.
    #[test]
    fn a_picture_downloads_and_opens_in_the_viewer() {
        let _l = lock();
        let mut r = lucia();
        by_label(&r, "Photo, 2.4 MB").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        let item = match sent.as_slice() {
            [ChatCommand::ReceiveFile { chat, item }] if *chat == LUCIA => *item,
            other => panic!("{other:?}"),
        };
        play(&mut r, &sent);
        r.fake.clear_sent();
        by_label(&r, "Photo, 2.4 MB").click();
        r.h.run_steps(4);
        assert!(r.h.state().state.chat.viewer.is_some());
        r.h.get_by_role_and_label(Role::Button, "Open").click();
        r.h.run_steps(3);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::OpenFile { chat: LUCIA, item }]
        );
        key(&mut r, Key::Escape);
        assert!(r.h.state().state.chat.viewer.is_none());
        // The chat is still open (Esc closed the viewer only).
        assert!(r.h.state().state.chat.open.is_some());
    }

    /// A file offers Download, then Open; your own file being sent offers Cancel.
    #[test]
    fn files_download_open_and_cancel() {
        let _l = lock();
        let mut r = lucia();
        row_button(&r, "Download").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        assert!(
            matches!(sent.as_slice(), [ChatCommand::ReceiveFile { .. }]),
            "{sent:?}"
        );
        play(&mut r, &sent);
        r.fake.clear_sent();
        row_button(&r, "Cancel").click();
        r.h.run_steps(3);
        assert!(
            matches!(chat_sent(&r).as_slice(), [ChatCommand::CancelFile { chat, .. }] if *chat == LUCIA),
            "{:?}",
            chat_sent(&r)
        );
        r.fake.clear_sent();
        row_button(&r, "Open").click();
        r.h.run_steps(3);
        assert!(
            matches!(chat_sent(&r).as_slice(), [ChatCommand::OpenFile { .. }]),
            "{:?}",
            chat_sent(&r)
        );
    }

    #[test]
    fn snapshot_media_image() {
        let _l = lock();
        let mut r = lucia();
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(10);
        r.h.snapshot("chat_media_image_dark_1440x900");
    }

    #[test]
    fn snapshot_media_video() {
        let _l = lock();
        let mut r = lucia();
        by_label(&r, "Video, 0:42").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        play(&mut r, &sent);
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(10);
        r.h.snapshot("chat_media_video_dark_1440x900");
    }

    #[test]
    fn snapshot_media_file() {
        let _l = lock();
        let mut r = lucia();
        row_button(&r, "Download").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        play(&mut r, &sent);
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(10);
        r.h.snapshot("chat_media_file_dark_1440x900");
    }

    #[test]
    fn snapshot_media_viewer() {
        let _l = lock();
        let mut r = lucia();
        by_label(&r, "Photo, 2.4 MB").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        play(&mut r, &sent);
        by_label(&r, "Photo, 2.4 MB").click();
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(10);
        r.h.snapshot("chat_media_viewer_dark_1440x900");
    }

    /// Scroll the transcript to its top (the GIF stands above the media).
    fn to_top(r: &mut Rig) {
        r.h.event(egui::Event::PointerMoved(egui::pos2(700.0, 500.0)));
        r.h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 100_000.0),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(30);
    }

    fn viewer_item(r: &Rig) -> Option<i64> {
        r.h.state().state.chat.viewer.as_ref().map(|v| v.item)
    }

    /// The id of the conversation's message whose body matches.
    fn item_of(r: &Rig, f: impl Fn(&MessageBody) -> bool) -> i64 {
        r.h.state()
            .state
            .chat
            .open
            .as_ref()
            .and_then(|c| c.messages.iter().find(|m| f(&m.body)))
            .map(|m| m.id)
            .expect("a message")
    }

    /// A click on a GIF opens it in the viewer; ← → step through the chat's pictures.
    #[test]
    fn the_viewer_opens_on_a_gif_and_steps() {
        let _l = lock();
        let mut r = lucia();
        let gif = item_of(&r, |b| matches!(b, MessageBody::Gif(_)));
        let pic = item_of(&r, |b| matches!(b, MessageBody::Image(_)));
        to_top(&mut r);
        by_label(&r, "GIF: gg").click();
        r.h.run_steps(4);
        assert_eq!(viewer_item(&r), Some(gif));
        key(&mut r, Key::ArrowRight);
        assert_eq!(viewer_item(&r), Some(pic));
        key(&mut r, Key::ArrowLeft);
        assert_eq!(viewer_item(&r), Some(gif));
        key(&mut r, Key::Escape);
        assert_eq!(viewer_item(&r), None);
    }

    /// A picture not fetched yet opens with its preview and Download; once here, Save as…
    /// sends SaveFile with the chosen path.
    #[test]
    fn the_viewer_downloads_and_saves() {
        let _l = lock();
        let mut r = lucia();
        let pic = item_of(&r, |b| matches!(b, MessageBody::Image(_)));
        // The picture itself (not its round button) opens the viewer.
        let b = by_label(&r, "Photo, 2.4 MB").accesskit_node().raw_bounds().unwrap();
        let at = egui::pos2(b.x0 as f32 + 30.0, b.y0 as f32 + 30.0);
        r.h.event(egui::Event::PointerMoved(at));
        r.h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        r.h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        r.h.run_steps(4);
        assert_eq!(viewer_item(&r), Some(pic));
        assert!(r.h.query_by_label("Save as…").is_none());
        row_button_any(&r, "Download").click();
        r.h.run_steps(3);
        let sent = chat_sent(&r);
        assert_eq!(sent, vec![ChatCommand::ReceiveFile { chat: LUCIA, item: pic }]);
        play(&mut r, &sent);
        r.fake.clear_sent();
        let to = std::path::PathBuf::from("/tmp/easyplay-test/sunset.jpg");
        r.picker.answer(vec![to.clone()]);
        r.h.get_by_label("Save as…").click();
        r.h.run_steps(6);
        assert_eq!(
            chat_sent(&r),
            vec![ChatCommand::SaveFile {
                chat: LUCIA,
                item: pic,
                to
            }]
        );
    }

    /// The viewer's primary "Download" (the picture's round button shares the words).
    fn row_button_any<'a>(r: &'a Rig, label: &str) -> egui_kittest::Node<'a> {
        let label = label.to_owned();
        r.h.query_all_by(move |n: &egui_kittest::kittest::AccessKitNode<'_>| {
            n.role() == Role::Button && n.label().as_deref() == Some(label.as_str())
        })
        .last()
        .expect("a button")
    }

    /// Our own sent picture shows from its file on this computer, not from the preview.
    #[test]
    fn a_sent_picture_shows_from_its_file() {
        use easyplay_core::model::{ChatMessage, Delivery, FileState, Media, MessageAuthor};
        let _l = lock();
        let mut r = lucia();
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/data/chat/sent.png");
        r.fake.push_event(easyplay_ui::bridge::UiEvent::ChatMessages {
            messages: vec![ChatMessage {
                id: 10_000,
                chat: LUCIA,
                author: MessageAuthor::Me,
                body: MessageBody::Image(Media {
                    name: "sent.png".into(),
                    size: 51_444,
                    preview: None,
                    duration: None,
                    caption: "the *new* route".into(),
                    local: Some(fixture),
                    state: FileState::Sent,
                }),
                at: now(),
                delivery: Delivery::Delivered,
                unread: false,
                reply: None,
                reactions: Vec::new(),
                edited: false,
                mentions: Vec::new(),
            }],
            updated: false,
        });
        // The file decodes on a worker: wait for the picture's own shape (240 × 180 → 360 ×
        // 270; without it, a 320 × 180 plate).
        let mut h = 0.0;
        for _ in 0..100 {
            r.h.run_steps(2);
            h = by_label(&r, "Photo, 51 KB")
                .accesskit_node()
                .raw_bounds()
                .map_or(0.0, |b| b.height());
            if (h - 270.0).abs() < 1.0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!((h - 270.0).abs() < 1.0, "height {h}");
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(30);
        r.h.snapshot("chat_media_sent_dark_1440x900");
    }

    #[test]
    fn snapshot_media_viewer_gif() {
        let _l = lock();
        let mut r = lucia();
        to_top(&mut r);
        by_label(&r, "GIF: gg").click();
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(20);
        r.h.snapshot("chat_media_viewer_gif_dark_1440x900");
    }

    #[test]
    fn snapshot_media_viewer_offered() {
        let _l = lock();
        let mut r = lucia();
        let b = by_label(&r, "Photo, 2.4 MB").accesskit_node().raw_bounds().unwrap();
        let at = egui::pos2(b.x0 as f32 + 30.0, b.y0 as f32 + 30.0);
        r.h.event(egui::Event::PointerMoved(at));
        r.h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        r.h.event(egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        r.h.event(egui::Event::PointerGone);
        r.h.run_steps(20);
        r.h.snapshot("chat_media_viewer_offered_dark_1440x900");
    }
}

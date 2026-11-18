//! Every sentence the overlay draws that is not a catalogue entry as it stands (times,
//! previews, counts, "in Friday Crew", hints with the key in them), made once per snapshot
//! instead of once per frame (AGENTS.md §6).
//!
//! **Eviction:** the cache is rebuilt whole when its key changes — the snapshot's revision,
//! the language's generation (`zaun_ui::i18n::generation`), or the minute of the clock
//! (times say "Today", "Yesterday") — and holds only the labels of the current snapshot: one
//! entry per row, message and toast of it, so it is never larger than the snapshot's bounds.
use std::ops::Range;

use zaun_core::chat::overlay::OverlaySnapshot;
use zaun_core::chat::overlay::snapshot::BodyKind;
use zaun_core::clock::Timestamp;

use crate::words;

#[derive(Default)]
pub struct RowLabels {
    pub time: String,
    pub preview: String,
    pub count: String,
}

#[derive(Default)]
pub struct MessageLabels {
    pub body: String,
    pub head_time: String,
    pub clock: String,
    pub day: String,
    /// Where `@name` mentions sit in `body` (bytes).
    pub mentions: Vec<Range<usize>>,
}

#[derive(Default)]
pub struct ToastLabels {
    pub id: u64,
    pub line: String,
    pub group: String,
}

/// One part of the hint under the panel: words, or a key cap.
pub struct HintPart {
    pub cap: bool,
    pub text: String,
}

#[derive(Default)]
pub struct Labels {
    key: Option<(u64, u64, i64)>,
    /// How many times it was built (the tests watch it).
    pub builds: u64,
    pub chats: Vec<RowLabels>,
    pub messages: Vec<MessageLabels>,
    pub toasts: Vec<ToastLabels>,
    /// The call widget's names, in the call's order ("You" for the user).
    pub people: Vec<String>,
    pub more: String,
    pub call: String,
    pub header: String,
    pub composer: String,
    /// The composer's placeholder as egui's hint, made on first use after each build.
    pub composer_hint: Option<std::sync::Arc<egui::RichText>>,
    pub start: String,
    pub reply_hint: (String, String),
    pub answer_hint: (String, String),
    pub back: Vec<HintPart>,
}

impl Labels {
    /// Make the labels of `snap` (revision `rev`) unless they are made already.
    pub fn refresh(&mut self, snap: &OverlaySnapshot, rev: u64, now: Timestamp, offset_s: i32) {
        let key = (
            rev,
            zaun_ui::i18n::generation(),
            now.as_millis().div_euclid(60_000),
        );
        if self.key == Some(key) {
            return;
        }
        self.key = Some(key);
        self.builds += 1;
        self.build(snap, now, offset_s);
    }

    fn build(&mut self, snap: &OverlaySnapshot, now: Timestamp, offset_s: i32) {
        self.chats = snap
            .chats
            .iter()
            .map(|c| RowLabels {
                time: c
                    .last
                    .as_ref()
                    .map(|l| words::list_time(l.at, now, offset_s))
                    .unwrap_or_default(),
                preview: c
                    .last
                    .as_ref()
                    .map(|l| words::preview(l, c.chat.is_group(), &c.who.name))
                    .unwrap_or_default(),
                count: c.unread.to_string(),
            })
            .collect();
        let friend = snap.open.as_ref().map_or("", |o| o.who.name.as_str());
        self.messages = snap
            .open
            .iter()
            .flat_map(|o| &o.messages)
            .map(|m| {
                let body = words::body(m, friend);
                let mentions = if m.kind == BodyKind::Text {
                    mention_ranges(&body, &m.mentions)
                } else {
                    Vec::new()
                };
                MessageLabels {
                    head_time: words::head_time(m.at, now, offset_s),
                    clock: words::clock(m.at, offset_s),
                    day: words::day(m.at, offset_s),
                    body,
                    mentions,
                }
            })
            .collect();
        self.toasts = snap
            .toasts
            .iter()
            .map(|t| ToastLabels {
                id: t.id,
                line: words::kind_text(t.kind, &t.text),
                group: t.group.as_deref().map(words::in_group).unwrap_or_default(),
            })
            .collect();
        self.people = snap
            .call
            .iter()
            .flat_map(|c| &c.people)
            .map(|p| {
                if p.me {
                    words::text("chat.conversation.you").to_owned()
                } else {
                    words::first_name(&p.who.name).to_owned()
                }
            })
            .collect();
        self.more = snap
            .call
            .as_ref()
            .filter(|c| c.more > 0)
            .map(|c| words::more(c.more))
            .unwrap_or_default();
        self.call = snap
            .call
            .as_ref()
            .map(|c| {
                if c.chat.is_group() {
                    words::voice_in(&c.name)
                } else {
                    c.name.clone()
                }
            })
            .unwrap_or_default();
        self.header = snap
            .open
            .as_ref()
            .map(|o| match (o.members, o.status) {
                (Some((n, online)), _) => words::members(n, online),
                (None, Some(s)) => words::status(s).to_owned(),
                (None, None) => String::new(),
            })
            .unwrap_or_default();
        self.composer = snap
            .open
            .as_ref()
            .map(|o| words::composer(&o.who.name))
            .unwrap_or_default();
        self.composer_hint = None;
        self.start = snap
            .open
            .as_ref()
            .map(|o| zaun_ui::strings::chat::conversation::start_dm(words::first_name(&o.who.name)))
            .unwrap_or_default();
        self.reply_hint = words::around_key("overlay.reply_hint");
        self.answer_hint = words::around_key("overlay.answer_hint");
        self.back = back_parts(snap.look.key.trim());
    }
}

/// "Press [key] or [Esc] to go back to the game", split into words and key caps; without a
/// key, "[Esc] Back to the game".
fn back_parts(key: &str) -> Vec<HintPart> {
    let words_part = |s: &str| HintPart {
        cap: false,
        text: s.trim().to_owned(),
    };
    let cap = |s: &str| HintPart {
        cap: true,
        text: s.to_owned(),
    };
    let mut v = if key.is_empty() {
        vec![cap("Esc"), words_part(words::text("overlay.back_short"))]
    } else {
        let (before, after) = words::around_key("overlay.back");
        let mut v = vec![words_part(&before), cap(key)];
        match after.split_once("Esc") {
            Some((mid, end)) => v.extend([words_part(mid), cap("Esc"), words_part(end)]),
            None => v.push(words_part(&after)),
        }
        v
    };
    v.retain(|p| !p.text.is_empty());
    v
}

/// Where each `@name` of `names` sits in `body`, in order.
fn mention_ranges(body: &str, names: &[String]) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some((at, len)) = names
        .iter()
        .filter_map(|n| {
            let needle = format!("@{n}");
            body[from..].find(&needle).map(|i| (from + i, needle.len()))
        })
        .min_by_key(|(i, _)| *i)
    {
        out.push(at..at + len);
        from = at + len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_are_found_in_order_and_each_once() {
        let r = mention_ranges("@Sam hi @Ana and @Sam", &["Sam".into(), "Ana".into()]);
        assert_eq!(r, vec![0..4, 8..12, 17..21]);
    }

    #[test]
    fn the_labels_are_made_once_per_snapshot_language_and_minute() {
        let mut l = Labels::default();
        let snap = OverlaySnapshot::default();
        let t = Timestamp::from_millis(60_000 * 1000 + 5);
        l.refresh(&snap, 1, t, 0);
        l.refresh(&snap, 1, Timestamp::from_millis(60_000 * 1000 + 30_000), 0);
        assert_eq!(l.builds, 1);
        l.refresh(&snap, 2, t, 0);
        assert_eq!(l.builds, 2, "a new snapshot");
        l.refresh(&snap, 2, Timestamp::from_millis(60_000 * 1001), 0);
        assert_eq!(l.builds, 3, "a new minute");
    }
}

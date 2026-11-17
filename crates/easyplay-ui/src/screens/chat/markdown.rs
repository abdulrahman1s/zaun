//! Message text, formatted exactly as the chat engine formats it (SimpleX Chat's
//! `Simplex.Chat.Markdown`, tag v7.0.3): `*bold*`, `_italic_`, `~strike~`, `` `code` ``,
//! `#secret#`, `!1 coloured!` (1 red · 2 green · 3 blue · 4 yellow · 5 cyan · 6 magenta, or
//! the colour's name), `!- small!`, plain links (`https://…`, bare domains such as
//! `example.com`), `[text](https://…)`, and mentions `@name` / `@'two words'`. Nothing nests;
//! anything else is plain text. Each line is parsed on its own. Owner: the conversation
//! agent.
//!
//! Two halves: [`parse`] (pure, unit-tested) turns the text into lines of spans; [`show`]
//! lays them out — words, chips and emoji on lines no longer than 68 characters, in the
//! message's own direction — and draws them as one galley through a selectable label, so the
//! text can be selected with the mouse and copied.

use std::collections::HashSet;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{
    Color32, CornerRadius, FontFamily, FontId, Galley, Id, Pos2, Rect, Sense, Shape, Stroke, Ui,
    pos2, vec2,
};

use super::emoji::{self, Run};
use crate::i18n::{Dir, bidi};
use crate::theme::{self, Type};

// ---- Parsing ------------------------------------------------------------------------------

/// The six colours of `!n text!`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hue {
    /// `!1` or `!red`.
    Red,
    /// `!2` or `!green`.
    Green,
    /// `!3` or `!blue`.
    Blue,
    /// `!4` or `!yellow`.
    Yellow,
    /// `!5` or `!cyan`.
    Cyan,
    /// `!6` or `!magenta`.
    Magenta,
}

/// What a span is. Formats do not nest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Plain words.
    Text,
    /// `*bold*`.
    Bold,
    /// `_italic_`.
    Italic,
    /// `~strike~`.
    Strike,
    /// `` `code` ``.
    Code,
    /// `#secret#`: the message's n-th (counted from 0), hidden until clicked.
    Secret(u32),
    /// `!- small!`.
    Small,
    /// `!1 coloured!`.
    Color(Hue),
    /// A link to this address.
    Link(String),
    /// `@Name` of a member the message mentions.
    Mention,
    /// "(edited)" after the text.
    Edited,
}

/// A run of text in one format.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    /// The text as shown.
    pub text: String,
    /// What it is.
    pub kind: Kind,
}

impl Span {
    fn new(text: impl Into<String>, kind: Kind) -> Self {
        Self {
            text: text.into(),
            kind,
        }
    }
}

/// One line of spans.
pub type Line = Vec<Span>;

/// Haskell's `isPunctuation` (Unicode categories Pc, Pd, Ps, Pe, Pi, Pf, Po), for the
/// characters a message is likely to hold.
fn is_punct(c: char) -> bool {
    if c.is_ascii() {
        return matches!(
            c,
            '!' | '"'
                | '#'
                | '%'
                | '&'
                | '\''
                | '('
                | ')'
                | '*'
                | ','
                | '-'
                | '.'
                | '/'
                | ':'
                | ';'
                | '?'
                | '@'
                | '['
                | '\\'
                | ']'
                | '_'
                | '{'
                | '}'
        );
    }
    matches!(c as u32,
        0x00A1 | 0x00A7 | 0x00AB | 0x00B6 | 0x00B7 | 0x00BB | 0x00BF
        | 0x037E | 0x0387 | 0x055A..=0x055F | 0x0589 | 0x058A
        | 0x060C | 0x060D | 0x061B | 0x061D..=0x061F | 0x066A..=0x066D | 0x06D4
        | 0x2010..=0x2027 | 0x2030..=0x2043 | 0x2045..=0x2051 | 0x2053..=0x205E
        | 0x2E00..=0x2E4F | 0x3001..=0x3003 | 0x3008..=0x3011 | 0x3014..=0x301F
        | 0xFE10..=0xFE19 | 0xFE30..=0xFE52 | 0xFE54..=0xFE61 | 0xFE63 | 0xFE68 | 0xFE6A | 0xFE6B
        | 0xFF01..=0xFF03 | 0xFF05..=0xFF0A | 0xFF0C..=0xFF0F | 0xFF1A | 0xFF1B | 0xFF1F | 0xFF20
        | 0xFF3B..=0xFF3D | 0xFF3F | 0xFF5B | 0xFF5D | 0xFF5F..=0xFF65)
}

/// The punctuation a word's link leaves outside (`isPunctuation'`: not `/ ) _ !`).
fn is_trailing_punct(c: char) -> bool {
    !matches!(c, '/' | ')' | '_' | '!') && is_punct(c)
}

/// A display name may start with this (`refChar`).
fn ref_char(c: char) -> bool {
    c > ' ' && c != '#' && c != '@' && c != '\''
}

/// An `http(s)` address with a host that has a dot (the engine's `parseUri`).
fn is_web_uri(s: &str) -> bool {
    let rest = match s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
    {
        Some(r) => r,
        None => return false,
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let host = host_port.split(':').next().unwrap_or("");
    !host.is_empty() && host.contains('.') && !host.chars().any(char::is_whitespace)
}

/// What is likely a domain (`isDomain`): `name.tld` or `sub.name.tld`, Latin letters only.
fn is_domain(s: &str) -> bool {
    let alpha = |t: &str| t.chars().all(|c| c.is_ascii_alphabetic());
    let ok = |name: &str, tld: &str| {
        (1..=24).contains(&name.chars().count())
            && (2..=8).contains(&tld.chars().count())
            && alpha(name)
            && alpha(tld)
    };
    let parts: Vec<&str> = s.split('.').collect();
    match parts.as_slice() {
        [name, tld] => ok(name, tld),
        [sub, name, tld] => (3..=8).contains(&sub.chars().count()) && ok(name, tld),
        _ => false,
    }
}

/// The parser of one line (`markdownP`): fragments one after another.
struct Parser<'a> {
    s: &'a [char],
    out: Line,
    secrets: &'a mut u32,
    /// The names the message mentions, longest first: received text writes `@Two Words`
    /// without quotes.
    names: &'a [Vec<char>],
}

impl Parser<'_> {
    fn push(&mut self, text: impl Into<String>, kind: Kind) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        match self.out.last_mut() {
            // Neighbours of one format join (the engine's `Semigroup`).
            Some(last) if last.kind == kind && kind == Kind::Text => last.text.push_str(&text),
            _ => self.out.push(Span::new(text, kind)),
        }
    }

    fn text(&self, a: usize, b: usize) -> String {
        self.s[a..b].iter().collect()
    }

    /// Up to (not including) the next `c` from `i`, or the end.
    fn till(&self, i: usize, c: char) -> usize {
        (i..self.s.len())
            .find(|&k| self.s[k] == c)
            .unwrap_or(self.s.len())
    }

    fn run(&mut self) {
        let mut i = 0;
        while i < self.s.len() {
            i = match self.s[i] {
                ' ' => {
                    let end = (i..self.s.len())
                        .find(|&k| self.s[k] != ' ')
                        .unwrap_or(self.s.len());
                    let t = self.text(i, end);
                    self.push(t, Kind::Text);
                    end
                }
                '*' => self.formatted(i, '*', Kind::Bold),
                '_' => self.formatted(i, '_', Kind::Italic),
                '~' => self.formatted(i, '~', Kind::Strike),
                '`' => self.formatted(i, '`', Kind::Code),
                '#' => self.secret(i),
                '!' => self.styled(i).unwrap_or_else(|| self.word(i)),
                '@' => self.mention(i).unwrap_or_else(|| self.word(i)),
                '[' => self.labelled_link(i).unwrap_or_else(|| self.word(i)),
                _ => self.word(i),
            };
        }
    }

    /// `c…c`: the text up to the next `c`; not formatted when empty or when it starts or
    /// ends with a space; without a closing `c` the rest of the line is plain.
    fn formatted(&mut self, i: usize, c: char, kind: Kind) -> usize {
        let j = self.till(i + 1, c);
        if j >= self.s.len() {
            let t = self.text(i, self.s.len());
            self.push(t, Kind::Text);
            return self.s.len();
        }
        let inner = &self.s[i + 1..j];
        if inner.is_empty() || inner[0] == ' ' || inner[inner.len() - 1] == ' ' {
            let t = self.text(i, j + 1);
            self.push(t, Kind::Text);
        } else {
            let t = self.text(i + 1, j);
            self.push(t, kind);
        }
        j + 1
    }

    /// `#secret#` (`secretP`), else `#` and the rest of the word as plain text.
    fn secret(&mut self, i: usize) -> usize {
        let n = self.s.len();
        let b_end = (i + 1..n).find(|&k| self.s[k] != '#').unwrap_or(n);
        let s_end = self.till(b_end, '#');
        let a_end = (s_end..n).find(|&k| self.s[k] != '#').unwrap_or(n);
        let inner = &self.s[b_end..s_end];
        if a_end > s_end && !inner.is_empty() && inner[0] != ' ' && inner[inner.len() - 1] != ' ' {
            // The leading and trailing marks beyond one each stay in the secret.
            let t = self.text(i + 1, a_end - 1);
            let k = *self.secrets;
            *self.secrets += 1;
            self.push(t, Kind::Secret(k));
            return a_end;
        }
        let end = self.till(i + 1, ' ');
        let t = self.text(i, end);
        self.push(t, Kind::Text);
        end
    }

    /// `!1 text!`, `!red text!`, `!- small!` (`styledP`).
    fn styled(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        let mut k = i + 1;
        let kind = match *s.get(k)? {
            '-' => {
                k += 1;
                Kind::Small
            }
            c => {
                let (hue, name) = match c {
                    'r' | '1' => (Hue::Red, "ed"),
                    'g' | '2' => (Hue::Green, "reen"),
                    'b' | '3' => (Hue::Blue, "lue"),
                    'y' | '4' => (Hue::Yellow, "ellow"),
                    'c' | '5' => (Hue::Cyan, "yan"),
                    'm' | '6' => (Hue::Magenta, "agenta"),
                    _ => return None,
                };
                k += 1;
                if c.is_ascii_alphabetic() {
                    let rest: Vec<char> = name.chars().collect();
                    if s.len() >= k + rest.len() && s[k..k + rest.len()] == rest[..] {
                        k += rest.len();
                    }
                }
                Kind::Color(hue)
            }
        };
        if !s.get(k)?.is_whitespace() {
            return None;
        }
        k += 1;
        let first = *s.get(k)?;
        if first == ' ' || first == '!' {
            return None;
        }
        let end = self.till(k, '!');
        if end >= s.len() || s[end - 1] == ' ' {
            return None;
        }
        let t = self.text(k, end);
        self.push(t, kind);
        Some(end + 1)
    }

    /// `@name` or `@'two words'` (`mentionP`); trailing punctuation stays outside.
    fn mention(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        // A name the message mentions, as written (spaces and all), not running on.
        let hit = self.names.iter().find(|n| {
            !n.is_empty()
                && s.len() >= i + 1 + n.len()
                && s[i + 1..i + 1 + n.len()] == n[..]
                && s.get(i + 1 + n.len())
                    .is_none_or(|&c| c.is_whitespace() || is_punct(c))
        });
        if let Some(n) = hit {
            let len = n.len();
            let name: String = n.iter().collect();
            self.push(format!("@{name}"), Kind::Mention);
            return Some(i + 1 + len);
        }
        let c = *s.get(i + 1)?;
        if c == '\'' {
            let first = *s.get(i + 2)?;
            if !ref_char(first) {
                return None;
            }
            let close = self.till(i + 2, '\'');
            if close >= s.len() {
                return None;
            }
            let name = self.text(i + 2, close);
            self.push(format!("@{name}"), Kind::Mention);
            return Some(close + 1);
        }
        if !ref_char(c) {
            return None;
        }
        let end = (i + 1..s.len())
            .find(|&k| s[k].is_whitespace())
            .unwrap_or(s.len());
        let mut name_end = end;
        while name_end > i + 1 && is_punct(s[name_end - 1]) {
            name_end -= 1;
        }
        let name = self.text(i + 1, name_end);
        self.push(format!("@{name}"), Kind::Mention);
        let punct = self.text(name_end, end);
        self.push(punct, Kind::Text);
        Some(end)
    }

    /// `[text](https://…)` (`sowLinkP`): the text without punctuation (but `-` and `_`)
    /// unless it is the address itself.
    fn labelled_link(&mut self, i: usize) -> Option<usize> {
        let s = self.s;
        let close = self.till(i + 1, ']');
        if close >= s.len() || close == i + 1 || s.get(close + 1) != Some(&'(') {
            return None;
        }
        let end = self.till(close + 2, ')');
        if end >= s.len() || end == close + 2 {
            return None;
        }
        let t = self.text(i + 1, close);
        let l = self.text(close + 2, end);
        let punct = t.chars().any(|c| is_punct(c) && c != '-' && c != '_');
        if punct && t != l && format!("https://{t}") != l {
            return None;
        }
        if !is_web_uri(&l) {
            return None;
        }
        self.push(t, Kind::Link(l));
        Some(end + 1)
    }

    /// A word up to the next space (`wordP`): a link when it is an address or a domain,
    /// with its trailing punctuation left outside; plain otherwise.
    fn word(&mut self, i: usize) -> usize {
        let end = (i + 1..self.s.len())
            .find(|&k| self.s[k] == ' ')
            .unwrap_or(self.s.len());
        let mut core_end = end;
        while core_end > i && is_trailing_punct(self.s[core_end - 1]) {
            core_end -= 1;
        }
        let core = self.text(i, core_end);
        let punct = self.text(core_end, end);
        let link = if core.chars().count() >= 10 && is_web_uri(&core) {
            Some(core.clone())
        } else if is_domain(&core) {
            Some(format!("https://{core}"))
        } else {
            None
        };
        match link {
            Some(url) => {
                self.push(core, Kind::Link(url));
                self.push(punct, Kind::Text);
            }
            None => {
                let t = self.text(i, end);
                self.push(t, Kind::Text);
            }
        }
        end
    }
}

/// A message's text as lines of spans. `mentions` are the names (without @) the message
/// mentions: only those are shown as mentions, any other `@word` stays plain.
pub fn parse(src: &str, mentions: &[&str]) -> Vec<Line> {
    let mut secrets = 0u32;
    let mut names: Vec<Vec<char>> = mentions.iter().map(|n| n.chars().collect()).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    let src = src.strip_suffix('\n').unwrap_or(src);
    src.split('\n')
        .map(|l| {
            let chars: Vec<char> = l.chars().collect();
            let mut p = Parser {
                s: &chars,
                out: Vec::new(),
                secrets: &mut secrets,
                names: &names,
            };
            p.run();
            let mut out: Line = Vec::new();
            for sp in p.out {
                let sp = match sp.kind {
                    Kind::Mention if !mentions.contains(&sp.text.trim_start_matches('@')) => {
                        Span::new(sp.text, Kind::Text)
                    }
                    _ => sp,
                };
                match out.last_mut() {
                    Some(last) if last.kind == Kind::Text && sp.kind == Kind::Text => {
                        last.text.push_str(&sp.text);
                    }
                    _ => out.push(sp),
                }
            }
            out
        })
        .collect()
}

/// "(edited)" after the last line.
pub fn with_edited(mut lines: Vec<Line>, word: &str) -> Vec<Line> {
    if let Some(l) = lines.last_mut() {
        l.push(Span::new(" ", Kind::Text));
        l.push(Span::new(word, Kind::Edited));
    }
    lines
}

/// The text as it reads, on one line, for a quoted reply: secrets become `secret`.
pub fn plain(src: &str, secret: &str) -> String {
    spoken(&parse(src, &[]), secret, |_| false)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// What a screen reader says: the text, closed secrets as `secret`.
pub fn spoken(lines: &[Line], secret: &str, revealed: impl Fn(u32) -> bool) -> String {
    lines
        .iter()
        .map(|l| {
            l.iter()
                .map(|sp| match sp.kind {
                    Kind::Secret(n) if !revealed(n) => secret,
                    _ => sp.text.as_str(),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

// ---- Layout -------------------------------------------------------------------------------

/// The measures of `.tx` (CSS "13 · CHAT").
mod m {
    /// Text size and line height.
    pub const SIZE: f32 = 14.0;
    pub const LINE: f32 = 21.0;
    /// `!- small!`.
    pub const SMALL: f32 = 12.0;
    /// Lines are at most this many `ch`.
    pub const MEASURE_CH: f32 = 68.0;
    /// `code`: mono 12.5, padded 5, radius 5, its box this high.
    pub const CODE_SIZE: f32 = 12.5;
    pub const CODE_PAD: f32 = 5.0;
    pub const CODE_BOX: f32 = 19.0;
    pub const CODE_R: u8 = 5;
    /// @Name: padded 3, radius 4.
    pub const AT_PAD: f32 = 3.0;
    pub const AT_R: u8 = 4;
    /// A secret's bar.
    pub const SP_R: u8 = 4;
    pub const SP_BOX: f32 = 19.0;
    /// Emoji in a line: 1.36 em.
    pub const EMOJI_EM: f32 = 1.36;
    /// Dropped 0.3 em (the one-line form).
    pub const EMOJI_DROP_EM: f32 = 0.3;
    /// A message of one to three emoji: 40 on lines 46 high (the emoji face's glyphs fill
    /// about 1.2 em, so 34 draws them 40 across).
    pub const JUMBO_FONT: f32 = 34.0;
    pub const JUMBO_LINE: f32 = 46.0;
}

/// How a message's text is shown.
pub struct Look<'a> {
    /// The text colour (`t1`; `t2` while it waits to be delivered).
    pub color: Color32,
    /// The message (secrets it opened are `(id, n)` in `revealed`).
    pub id: i64,
    /// Opened secrets.
    pub revealed: &'a HashSet<(i64, u32)>,
    /// Up to three emoji alone: shown at 40.
    pub jumbo: bool,
    /// Right to left (the text's first strong letter is).
    pub rtl: bool,
    /// Links and secrets take keyboard focus (the message is the active one).
    pub focusable: bool,
}

/// What was clicked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Clicked {
    /// A link: its address.
    pub link: Option<String>,
    /// A secret: its number.
    pub secret: Option<u32>,
    /// A secondary click on the text (the message's menu opens there).
    pub menu: Option<Pos2>,
}

/// The widest a line of text may be: 68 characters (`ch`, the width of "0").
pub fn measure(ui: &Ui) -> f32 {
    let g = ui.fonts_mut(|f| f.layout_no_wrap("0".into(), body_font(400), Color32::WHITE));
    g.size().x * m::MEASURE_CH
}

fn body_font(weight: u16) -> FontId {
    let family = match weight {
        600 => Type::BodyBold.font_id().family,
        500 => Type::BodyStrong.font_id().family,
        _ => FontFamily::Proportional,
    };
    FontId::new(m::SIZE, family)
}

/// The six colours, as the composer's colour menu shows them.
fn hue(h: Hue, kind: theme::palette::ThemeKind) -> Color32 {
    let n = match h {
        Hue::Red => 1,
        Hue::Green => 2,
        Hue::Blue => 3,
        Hue::Yellow => 4,
        Hue::Cyan => 5,
        Hue::Magenta => 6,
    };
    super::composer::mark_color(kind, n)
}

/// A piece on a line.
#[derive(Clone)]
struct Bit {
    /// Its text: a word, a space, an emoji, or a whole chip.
    text: String,
    format: TextFormat,
    /// Its width (measured), its chip padding on each side included.
    width: f32,
    /// The span it belongs to.
    span: usize,
    /// Chip padding on each side.
    pad: f32,
    space: bool,
    /// Strong direction: Some(true) right to left.
    dir: Option<bool>,
}

fn strong_dir(text: &str) -> Option<bool> {
    if bidi::has_rtl(text) {
        Some(true)
    } else if text.chars().any(char::is_alphabetic) {
        Some(false)
    } else {
        None
    }
}

/// The format of a span's text.
fn format_of(kind: &Kind, color: Color32, p: &theme::Palette, line_h: f32) -> TextFormat {
    let (font, c) = match kind {
        Kind::Code => (FontId::new(m::CODE_SIZE, FontFamily::Monospace), color),
        Kind::Mention => (body_font(500), p.accent_text),
        Kind::Link(_) => (body_font(400), p.accent_text),
        Kind::Edited => (Type::Micro.font_id(), p.t3),
        Kind::Bold => (body_font(600), color),
        Kind::Small => (FontId::new(m::SMALL, FontFamily::Proportional), p.t2),
        Kind::Color(h) => (body_font(400), hue(*h, p.kind)),
        _ => (body_font(400), color),
    };
    TextFormat {
        font_id: font,
        color: c,
        italics: *kind == Kind::Italic,
        strikethrough: if *kind == Kind::Strike {
            Stroke::new(1.0, c)
        } else {
            Stroke::NONE
        },
        line_height: Some(line_h),
        valign: egui::Align::Center,
        ..Default::default()
    }
}

fn width_of(ui: &Ui, text: &str, format: &TextFormat) -> f32 {
    ui.fonts_mut(|f| {
        f.layout_job(LayoutJob::single_section(text.to_owned(), format.clone()))
            .size()
            .x
    })
}

/// The bits of one line, broken into lines no wider than `width`, each in visual order.
#[allow(clippy::too_many_arguments)]
fn lay_line(
    ui: &Ui,
    line: &[Span],
    span_base: usize,
    color: Color32,
    width: f32,
    jumbo: bool,
    rtl: bool,
) -> Vec<Vec<Bit>> {
    let p = theme::theme(ui.ctx()).palette.clone();
    let base = Some(if rtl { Dir::Rtl } else { Dir::Ltr });
    let line_h = if jumbo { m::JUMBO_LINE } else { m::LINE };
    let mut bits: Vec<Bit> = Vec::new();
    for (k, sp) in line.iter().enumerate() {
        let span = span_base + k;
        let format = format_of(&sp.kind, color, &p, line_h);
        let visual = |t: &str| bidi::visual_line(t, base).into_owned();
        // A chip (code, @Name) stays whole.
        if matches!(sp.kind, Kind::Code | Kind::Mention) {
            let pad = if sp.kind == Kind::Code {
                m::CODE_PAD
            } else {
                m::AT_PAD
            };
            let text = visual(&sp.text);
            let w = width_of(ui, &text, &format) + pad * 2.0;
            bits.push(Bit {
                dir: strong_dir(&sp.text),
                text,
                format,
                width: w,
                span,
                pad,
                space: false,
            });
            continue;
        }
        let emoji_px = if jumbo {
            m::JUMBO_FONT
        } else {
            (format.font_id.size * m::EMOJI_EM).round()
        };
        let space_w = width_of(ui, " ", &format);
        let mut first = true;
        for w in sp.text.split(' ') {
            if !first {
                bits.push(Bit {
                    text: " ".into(),
                    format: format.clone(),
                    width: space_w,
                    span,
                    pad: 0.0,
                    space: true,
                    dir: None,
                });
            }
            first = false;
            if w.is_empty() {
                continue;
            }
            for r in emoji::runs(w) {
                match r {
                    Run::Text(t) if !t.is_empty() => {
                        let text = visual(t);
                        bits.push(Bit {
                            width: width_of(ui, &text, &format),
                            dir: strong_dir(t),
                            text,
                            format: format.clone(),
                            span,
                            pad: 0.0,
                            space: false,
                        });
                    }
                    Run::Emoji(e) => {
                        let mut f = format.clone();
                        f.font_id = FontId::new(emoji_px, FontFamily::Proportional);
                        f.italics = false;
                        bits.push(Bit {
                            width: width_of(ui, e, &f),
                            text: e.to_owned(),
                            format: f,
                            span,
                            pad: 0.0,
                            space: false,
                            dir: None,
                        });
                    }
                    Run::Text(_) => {}
                }
            }
        }
    }
    // Break at spaces; a word wider than the line breaks anywhere.
    let mut lines: Vec<Vec<Bit>> = vec![Vec::new()];
    let mut x = 0.0;
    let mut i = 0;
    while i < bits.len() {
        if bits[i].space {
            if let Some(l) = lines.last_mut()
                && !l.is_empty()
            {
                x += bits[i].width;
                l.push(bits[i].clone());
            }
            i += 1;
            continue;
        }
        let end = (i..bits.len())
            .find(|&j| bits[j].space)
            .unwrap_or(bits.len());
        let ww: f32 = bits[i..end].iter().map(|b| b.width).sum();
        let cur_empty = lines.last().is_none_or(|l| l.iter().all(|b| b.space));
        if x + ww > width && !cur_empty {
            trim_end(lines.last_mut());
            lines.push(Vec::new());
            x = 0.0;
        }
        for b in &bits[i..end] {
            if x + b.width > width && x > 0.0 {
                trim_end(lines.last_mut());
                lines.push(Vec::new());
                x = 0.0;
            }
            if b.width > width && b.text.chars().count() > 1 {
                // Longer than a line on its own: cut it by characters.
                for piece in split_text(ui, &b.text, &b.format, width - b.pad * 2.0) {
                    let pw = width_of(ui, &piece, &b.format) + b.pad * 2.0;
                    if x + pw > width && x > 0.0 {
                        lines.push(Vec::new());
                        x = 0.0;
                    }
                    x += pw;
                    if let Some(l) = lines.last_mut() {
                        l.push(Bit {
                            text: piece,
                            width: pw,
                            ..b.clone()
                        });
                    }
                }
                continue;
            }
            x += b.width;
            if let Some(l) = lines.last_mut() {
                l.push(b.clone());
            }
        }
        i = end;
    }
    trim_end(lines.last_mut());
    lines.into_iter().map(|l| visual_order(l, rtl)).collect()
}

fn trim_end(line: Option<&mut Vec<Bit>>) {
    if let Some(l) = line {
        while l.last().is_some_and(|b| b.space) {
            l.pop();
        }
    }
}

/// `text` cut into pieces no wider than `width` (at least one character each).
fn split_text(ui: &Ui, text: &str, format: &TextFormat, width: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        let mut next = cur.clone();
        next.push(c);
        if !cur.is_empty() && width_of(ui, &next, format) > width {
            out.push(std::mem::replace(&mut cur, c.to_string()));
        } else {
            cur = next;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The bits of a line in the order they stand from left to right: runs of one direction
/// keep their order in their direction, and the paragraph's direction orders the runs.
fn visual_order(line: Vec<Bit>, rtl: bool) -> Vec<Bit> {
    let dirs = resolve(&line.iter().map(|b| b.dir).collect::<Vec<_>>(), rtl);
    let mut runs: Vec<(bool, Vec<Bit>)> = Vec::new();
    for (b, d) in line.into_iter().zip(dirs) {
        match runs.last_mut() {
            Some((rd, v)) if *rd == d => v.push(b),
            _ => runs.push((d, vec![b])),
        }
    }
    if rtl {
        runs.reverse();
    }
    runs.into_iter()
        .flat_map(|(d, mut v)| {
            if d {
                v.reverse();
            }
            v
        })
        .collect()
}

/// Each bit's direction (`true` right to left): strong ones keep theirs; a neutral between
/// two of one direction takes it, else the paragraph's.
pub(crate) fn resolve(dirs: &[Option<bool>], rtl: bool) -> Vec<bool> {
    (0..dirs.len())
        .map(|i| {
            dirs[i].unwrap_or_else(|| {
                let before = dirs[..i].iter().rev().find_map(|d| *d).unwrap_or(rtl);
                let after = dirs[i + 1..].iter().find_map(|d| *d).unwrap_or(rtl);
                if before == after { before } else { rtl }
            })
        })
        .collect()
}

/// Every glyph of `g` with its row and the index of its character in the galley's text
/// (a row that ends a line is followed by its `'\n'`, which has no glyph).
fn glyphs(
    g: &Galley,
) -> impl Iterator<
    Item = (
        &egui::epaint::text::PlacedRow,
        &egui::epaint::text::Glyph,
        usize,
    ),
> {
    let mut at = 0usize;
    g.rows.iter().flat_map(move |row| {
        let start = at;
        at += row.row.glyphs.len() + usize::from(row.ends_with_newline);
        row.row
            .glyphs
            .iter()
            .enumerate()
            .map(move |(k, gl)| (row, gl, start + k))
    })
}

/// The span under `pos` (relative to the galley), if any.
fn span_at(g: &Galley, pos: egui::Vec2, char_span: &[usize]) -> Option<usize> {
    glyphs(g)
        .find(|(row, gl, _)| {
            let r = row.rect();
            let x0 = row.pos.x + gl.pos.x;
            pos.y >= r.top() && pos.y <= r.bottom() && pos.x >= x0 && pos.x <= x0 + gl.advance_width
        })
        .and_then(|(_, _, c)| char_span.get(c).copied())
}

/// Show `lines` at the cursor, at most [`measure`] wide, as one selectable galley.
pub fn show(ui: &mut Ui, id: Id, lines: &[Line], look: &Look<'_>) -> Clicked {
    let theme = theme::theme(ui.ctx());
    let p = theme.palette.clone();
    let width = ui.available_width().min(measure(ui)).max(1.0);
    let mut spans: Vec<&Span> = Vec::new();
    let mut laid: Vec<Vec<Bit>> = Vec::new();
    for l in lines {
        let base = spans.len();
        spans.extend(l.iter());
        if l.is_empty() {
            laid.push(Vec::new());
            continue;
        }
        laid.extend(lay_line(
            ui, l, base, look.color, width, look.jumbo, look.rtl,
        ));
    }
    let hidden = |span: usize| match spans[span].kind {
        Kind::Secret(n) => !look.revealed.contains(&(look.id, n)),
        _ => false,
    };
    // The link under the pointer last frame is underlined.
    let hover_key = id.with("hover");
    let hovered: Option<usize> = ui.ctx().data(|d| d.get_temp(hover_key)).flatten();
    // One job: each bit a section, lines joined with "\n"; right-to-left lines pushed to
    // the right edge; chips padded with leading space.
    let line_h = if look.jumbo { m::JUMBO_LINE } else { m::LINE };
    let mut job = LayoutJob::default();
    let mut section_span: Vec<usize> = Vec::new(); // one per character of the job's text
    for (k, l) in laid.iter().enumerate() {
        let line_w: f32 = l.iter().map(|b| b.width).sum();
        let mut lead = if look.rtl {
            (width - line_w).max(0.0)
        } else {
            0.0
        };
        if l.is_empty() {
            let f = format_of(&Kind::Text, look.color, &p, line_h);
            job.append(if k + 1 < laid.len() { "\n" } else { " " }, 0.0, f);
            section_span.push(usize::MAX);
            continue;
        }
        for (n, b) in l.iter().enumerate() {
            let mut f = b.format.clone();
            if hidden(b.span) {
                f.color = Color32::TRANSPARENT;
                f.strikethrough = Stroke::NONE;
            }
            if hovered == Some(b.span) && matches!(spans[b.span].kind, Kind::Link(_)) {
                f.underline = Stroke::new(1.0, f.color);
            }
            let mut text = b.text.clone();
            if n + 1 == l.len() && k + 1 < laid.len() {
                text.push('\n');
            }
            job.append(&text, lead + b.pad, f);
            section_span.extend(std::iter::repeat_n(b.span, text.chars().count()));
            lead = b.pad;
        }
    }
    if job.text.is_empty() {
        return Clicked::default();
    }
    job.wrap.max_width = f32::INFINITY;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let size = vec2(width, galley.size().y);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let mut clicked = Clicked::default();
    if !ui.is_rect_visible(rect) {
        return clicked;
    }
    // Chips and secret bars go under the text.
    let under = ui.painter().add(Shape::Noop);
    let label_rect = Rect::from_min_size(rect.min, galley.size());
    let label = egui::Label::new(egui::WidgetText::Galley(galley.clone()))
        .selectable(true)
        .sense(Sense::click());
    let resp = ui.put(label_rect, label);
    // Frames per span per row.
    let mut frames: Vec<(usize, Rect)> = Vec::new();
    for (row, gl, c) in glyphs(&galley) {
        {
            let Some(&span) = section_span.get(c) else {
                continue;
            };
            if span == usize::MAX {
                continue;
            }
            let wants = matches!(
                spans[span].kind,
                Kind::Code | Kind::Mention | Kind::Secret(_)
            );
            if !wants || gl.chr == '\n' {
                continue;
            }
            let x0 = label_rect.left() + row.pos.x + gl.pos.x;
            let r = Rect::from_min_size(
                pos2(x0, label_rect.top() + row.pos.y),
                vec2(gl.advance_width, row.rect().height()),
            );
            match frames
                .iter_mut()
                .find(|(s, f)| *s == span && (f.center().y - r.center().y).abs() < 1.0)
            {
                Some((_, f)) => *f = f.union(r),
                None => frames.push((span, r)),
            }
        }
    }
    let mut shapes = Vec::new();
    for (span, r) in &frames {
        let pad = match spans[*span].kind {
            Kind::Code => m::CODE_PAD,
            Kind::Mention => m::AT_PAD,
            _ => 0.0,
        };
        let (fill, corner, h) = match spans[*span].kind {
            Kind::Code => (p.o2, m::CODE_R, m::CODE_BOX),
            Kind::Mention => (p.tint, m::AT_R, m::SP_BOX),
            Kind::Secret(_) if hidden(*span) => (p.t3, m::SP_R, m::SP_BOX),
            Kind::Secret(_) => (p.o2, m::SP_R, m::SP_BOX),
            _ => continue,
        };
        let bg = Rect::from_center_size(r.center(), vec2(r.width() + pad * 2.0, h.min(r.height())));
        shapes.push(Shape::rect_filled(bg, CornerRadius::same(corner), fill));
    }
    ui.painter().set(under, Shape::Vec(shapes));
    // What the pointer is over: links act, closed secrets open.
    let over = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|pos| label_rect.contains(*pos))
        .and_then(|pos| span_at(&galley, pos - label_rect.min, &section_span))
        .filter(|&s| s != usize::MAX);
    let link_over = over.filter(|&s| matches!(spans[s].kind, Kind::Link(_)));
    if link_over != hovered {
        ui.ctx().data_mut(|d| d.insert_temp(hover_key, link_over));
        ui.ctx().request_repaint();
    }
    if let Some(s) = over {
        match &spans[s].kind {
            Kind::Link(url) => {
                crate::widgets::tooltip::attach(
                    resp.clone(),
                    &crate::strings::chat::conversation::opens(&host_of(url)),
                );
            }
            Kind::Secret(_) if hidden(s) => {
                crate::widgets::tooltip::attach(
                    resp.clone(),
                    crate::strings::chat::conversation::SPOILER_TIP(),
                );
            }
            _ => {}
        }
    }
    if resp.clicked()
        && let Some(s) = over
    {
        match &spans[s].kind {
            Kind::Link(url) => clicked.link = Some(url.clone()),
            Kind::Secret(n) if hidden(s) => clicked.secret = Some(*n),
            _ => {}
        }
    }
    if resp.secondary_clicked() {
        clicked.menu = resp.interact_pointer_pos();
    }
    // Links and closed secrets as their own nodes (over the text), so a click, the keys of
    // the active message and screen readers reach them.
    let mut seen: Vec<usize> = Vec::new();
    for (k, (span, r)) in frames_and_links(&galley, label_rect, &section_span, &spans)
        .into_iter()
        .enumerate()
    {
        let kind = &spans[span].kind;
        let acts = matches!(kind, Kind::Link(_)) || hidden(span);
        if !acts || seen.contains(&span) {
            continue;
        }
        seen.push(span);
        let sense = if look.focusable {
            Sense::click()
        } else {
            Sense::CLICK
        };
        let resp = ui.interact(r, id.with(("tx.act", k)), sense);
        match kind {
            Kind::Link(url) => {
                crate::a11y::item(&resp, egui::Role::Link, &spans[span].text);
                if resp.clicked() {
                    clicked.link = Some(url.clone());
                }
            }
            Kind::Secret(n) => {
                crate::a11y::button(&resp, crate::strings::chat::conversation::SPOILER());
                if resp.clicked() {
                    clicked.secret = Some(*n);
                }
            }
            _ => {}
        }
        if resp.secondary_clicked() {
            clicked.menu = resp.interact_pointer_pos();
        }
        if resp.has_focus() {
            crate::widgets::focus_ring(ui.painter(), r.expand(1.0), CornerRadius::same(3), &theme);
        }
    }
    clicked
}

/// The first row's rectangle of each link and secret span, in screen coordinates.
fn frames_and_links(
    g: &Galley,
    at: Rect,
    section_span: &[usize],
    spans: &[&Span],
) -> Vec<(usize, Rect)> {
    let mut out: Vec<(usize, Rect)> = Vec::new();
    for (row, gl, c) in glyphs(g) {
        {
            let Some(&span) = section_span.get(c) else {
                continue;
            };
            if span == usize::MAX
                || gl.chr == '\n'
                || !matches!(spans[span].kind, Kind::Link(_) | Kind::Secret(_))
            {
                continue;
            }
            let r = Rect::from_min_size(
                pos2(at.left() + row.pos.x + gl.pos.x, at.top() + row.pos.y),
                vec2(gl.advance_width, row.rect().height()),
            );
            match out
                .iter_mut()
                .find(|(s, f)| *s == span && (f.center().y - r.center().y).abs() < 1.0)
            {
                Some((_, f)) => *f = f.union(r),
                None => out.push((span, r)),
            }
        }
    }
    out
}

/// The baseline of a galley's first row, from its top.
fn galley_baseline(g: &Galley) -> f32 {
    g.rows
        .first()
        .and_then(|r| r.row.glyphs.first().map(|gl| r.pos.y + gl.pos.y))
        .unwrap_or(g.size().y * 0.78)
}

pub struct OneLine {
    pieces: Vec<(Option<Arc<Galley>>, Option<String>, f32)>,
    side: f32,
    size: f32,
    color: Color32,
}

impl OneLine {
    /// `text` in `font`, at most `max_w` wide.
    pub fn new(ui: &Ui, text: &str, font: FontId, color: Color32, max_w: f32) -> Self {
        let side = (font.size * m::EMOJI_EM).round();
        let size = font.size;
        let lay = |t: &str| {
            let t = bidi::visual_line(t, None).into_owned();
            ui.fonts_mut(|f| f.layout_no_wrap(t, font.clone(), color))
        };
        let ell = lay("…");
        let mut pieces = Vec::new();
        let mut w = 0.0;
        let mut cut = false;
        'outer: for r in emoji::runs(text) {
            match r {
                Run::Emoji(e) => {
                    if w + side > max_w {
                        cut = true;
                        break;
                    }
                    w += side;
                    pieces.push((None, Some(e.to_owned()), side));
                }
                Run::Text(t) => {
                    let g = lay(t);
                    if w + g.size().x <= max_w {
                        w += g.size().x;
                        pieces.push((Some(g), None, w));
                        continue;
                    }
                    // Cut by characters, leaving room for "…".
                    let mut acc = String::new();
                    for c in t.chars() {
                        acc.push(c);
                        let gw = lay(&acc).size().x;
                        if w + gw + ell.size().x > max_w {
                            acc.pop();
                            break;
                        }
                    }
                    if !acc.is_empty() {
                        let g = lay(&acc);
                        w += g.size().x;
                        pieces.push((Some(g), None, w));
                    }
                    cut = true;
                    break 'outer;
                }
            }
        }
        if cut {
            let ew = ell.size().x;
            pieces.push((Some(ell), None, ew));
        }
        for p in &mut pieces {
            p.2 = match (&p.0, &p.1) {
                (Some(g), _) => g.size().x,
                _ => side,
            };
        }
        Self {
            pieces,
            side,
            size,
            color,
        }
    }

    /// Its width.
    pub fn width(&self) -> f32 {
        self.pieces.iter().map(|p| p.2).sum()
    }

    /// Paint it with its vertical centre at `y`, from `x` (or ending at `x` when `rtl`).
    pub fn paint(&self, painter: &egui::Painter, x: f32, y: f32) {
        let mut x = x;
        let base = y + self.size * 0.36;
        for (g, e, w) in &self.pieces {
            match (g, e) {
                (Some(g), _) => {
                    let b = galley_baseline(g);
                    painter.galley(pos2(x, base - b), g.clone(), self.color);
                }
                (None, Some(e)) => {
                    let r = Rect::from_min_size(
                        pos2(x, base + self.size * m::EMOJI_DROP_EM - self.side),
                        vec2(self.side, self.side),
                    );
                    emoji::draw(painter, r, e, self.color);
                }
                _ => {}
            }
            x += w;
        }
    }
}

/// The site of an address, as a tooltip names it.
pub fn host_of(url: &str) -> String {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);
    rest.split(['/', '?', '#'])
        .next()
        .unwrap_or(rest)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> Span {
        Span::new(s, Kind::Text)
    }

    fn line(src: &str) -> Line {
        parse(src, &[]).remove(0)
    }

    #[test]
    fn plain_text_is_one_span() {
        assert_eq!(line("hello there"), vec![t("hello there")]);
    }

    #[test]
    fn single_marks_format_and_do_not_nest() {
        assert_eq!(
            line("did you get past the *lighthouse* boss yet?"),
            vec![
                t("did you get past the "),
                Span::new("lighthouse", Kind::Bold),
                t(" boss yet?")
            ]
        );
        assert_eq!(
            line("so you _bait_ the ~lunge~ `first`"),
            vec![
                t("so you "),
                Span::new("bait", Kind::Italic),
                t(" the "),
                Span::new("lunge", Kind::Strike),
                t(" "),
                Span::new("first", Kind::Code),
            ]
        );
        // Spaces inside are fine; nothing nests.
        assert_eq!(
            line("*two words*"),
            vec![Span::new("two words", Kind::Bold)]
        );
        assert_eq!(line("*_x_*"), vec![Span::new("_x_", Kind::Bold)]);
        // Right after a formatted fragment another may start.
        assert_eq!(
            line("*a*_b_"),
            vec![Span::new("a", Kind::Bold), Span::new("b", Kind::Italic)]
        );
    }

    #[test]
    fn marks_inside_words_with_spaces_inside_or_unclosed_stay_text() {
        assert_eq!(line("snake_case_name"), vec![t("snake_case_name")]);
        assert_eq!(line("2 * 3 * 4"), vec![t("2 * 3 * 4")]);
        assert_eq!(line("*open"), vec![t("*open")]);
        assert_eq!(line("a *b * c"), vec![t("a *b * c")]);
        assert_eq!(line("**"), vec![t("**")]);
        // Markdown elsewhere is not the engine's: double marks, ||, lists, quotes.
        assert_eq!(line("**x**"), vec![t("**x**")]);
        assert_eq!(line("||x||"), vec![t("||x||")]);
        assert_eq!(line("- item"), vec![t("- item")]);
        assert_eq!(line("> quote"), vec![t("> quote")]);
        assert_eq!(line(":fire:"), vec![t(":fire:")]);
    }

    #[test]
    fn secrets_are_numbered_through_the_message() {
        let l = parse("the ending is #not what you think# ok\n#again#", &[]);
        assert_eq!(
            l[0],
            vec![
                t("the ending is "),
                Span::new("not what you think", Kind::Secret(0)),
                t(" ok")
            ]
        );
        assert_eq!(l[1], vec![Span::new("again", Kind::Secret(1))]);
        // Not secrets: a lone mark, spaces at the edges, a hashtag.
        assert_eq!(line("#tag and more"), vec![t("#tag and more")]);
        assert_eq!(line("# x#"), vec![t("# x#")]);
        // Extra marks stay inside (the engine keeps them).
        assert_eq!(line("##x##"), vec![Span::new("#x#", Kind::Secret(0))]);
    }

    #[test]
    fn colours_and_small() {
        assert_eq!(
            line("!1 red alert! and !green go!"),
            vec![
                Span::new("red alert", Kind::Color(Hue::Red)),
                t(" and "),
                Span::new("go", Kind::Color(Hue::Green)),
            ]
        );
        assert_eq!(
            line("!- fine print!"),
            vec![Span::new("fine print", Kind::Small)]
        );
        assert_eq!(
            line("!6 x!"),
            vec![Span::new("x", Kind::Color(Hue::Magenta))]
        );
        // No colour, no space after it, nothing inside: text.
        assert_eq!(line("!7 x!"), vec![t("!7 x!")]);
        assert_eq!(line("!1x!"), vec![t("!1x!")]);
        assert_eq!(line("wow!"), vec![t("wow!")]);
    }

    #[test]
    fn links_bare_domains_and_labelled() {
        assert_eq!(
            line("patch notes are out: https://ninefold.example/patch-4-2."),
            vec![
                t("patch notes are out: "),
                Span::new(
                    "https://ninefold.example/patch-4-2",
                    Kind::Link("https://ninefold.example/patch-4-2".into())
                ),
                t(".")
            ]
        );
        assert_eq!(
            line("see example.com, ok"),
            vec![
                t("see "),
                Span::new("example.com", Kind::Link("https://example.com".into())),
                t(", ok")
            ]
        );
        assert_eq!(
            line("[the notes](https://ninefold.example/patch)"),
            vec![Span::new(
                "the notes",
                Kind::Link("https://ninefold.example/patch".into())
            )]
        );
        // Punctuation in the words, another scheme, no dot in the host: text.
        assert_eq!(
            line("[a.b](https://x.example)"),
            vec![t("[a.b](https://x.example)")]
        );
        assert_eq!(
            line("[x](javascript:alert(1))"),
            vec![t("[x](javascript:alert(1))")]
        );
        assert_eq!(line("http://localhost/x"), vec![t("http://localhost/x")]);
        // The engine's domain rule takes any `name.tld` of Latin letters, so "file.rs" is a
        // link there too; numbers are not.
        assert_eq!(
            line("file.rs"),
            vec![Span::new("file.rs", Kind::Link("https://file.rs".into()))]
        );
        assert_eq!(line("v1.2"), vec![t("v1.2")]);
        // Marks inside an address are part of it; trailing punctuation is not.
        assert_eq!(
            line("https://a.example/*b*"),
            vec![
                Span::new(
                    "https://a.example/*b",
                    Kind::Link("https://a.example/*b".into())
                ),
                t("*")
            ]
        );
    }

    #[test]
    fn mentions_only_for_members_the_message_names() {
        let l = parse("@Sam you joining? @Nobody too. mail@Sam @Sam,", &["Sam"]);
        assert_eq!(
            l[0],
            vec![
                Span::new("@Sam", Kind::Mention),
                t(" you joining? @Nobody too. mail@Sam "),
                Span::new("@Sam", Kind::Mention),
                t(","),
            ]
        );
        let l = parse("hi @'Jonas Weber'!", &["Jonas Weber"]);
        assert_eq!(
            l[0],
            vec![t("hi "), Span::new("@Jonas Weber", Kind::Mention), t("!")]
        );
        // Received text names it without quotes; the longest name wins.
        let l = parse("@Jonas Weber gg @Jonas", &["Jonas", "Jonas Weber"]);
        assert_eq!(
            l[0],
            vec![
                Span::new("@Jonas Weber", Kind::Mention),
                t(" gg "),
                Span::new("@Jonas", Kind::Mention)
            ]
        );
        assert_eq!(line("@Sam hi"), vec![t("@Sam hi")]);
    }

    #[test]
    fn lines_are_parsed_on_their_own() {
        let l = parse("*a\nb*", &[]);
        assert_eq!(l, vec![vec![t("*a")], vec![t("b*")]]);
    }

    #[test]
    fn edited_goes_after_the_last_line() {
        let l = with_edited(parse("a\nb", &[]), "(edited)");
        assert_eq!(l[1].last().map(|s| &s.kind), Some(&Kind::Edited));
    }

    #[test]
    fn plain_and_spoken() {
        assert_eq!(
            plain("*so* #secret# [notes](https://x.example) `c`", "secret"),
            "so secret notes c"
        );
        let l = parse("the end is #near# ok", &[]);
        assert_eq!(spoken(&l, "secret", |_| false), "the end is secret ok");
        assert_eq!(spoken(&l, "secret", |_| true), "the end is near ok");
    }

    #[test]
    fn neutrals_take_the_direction_around_them() {
        assert_eq!(
            resolve(&[Some(true), None, Some(true)], false),
            vec![true, true, true]
        );
        assert_eq!(
            resolve(&[Some(false), None, Some(true)], true),
            vec![false, true, true]
        );
        assert_eq!(resolve(&[None, None], true), vec![true, true]);
    }

    #[test]
    fn hosts() {
        assert_eq!(
            host_of("https://ninefold.example/patch?x#y"),
            "ninefold.example"
        );
        assert_eq!(host_of("http://a.example"), "a.example");
    }
}

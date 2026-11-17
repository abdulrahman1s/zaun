//! Emoji (chat.html section 8 EMOJI, EMOJI PICKER; script sections 21 and 24). Twemoji
//! pictures ship inside the app (the `twemoji-assets` crate's 72 px PNGs, never loaded from a
//! server; CC-BY 4.0, credited in About › Licences) and are laid out in a line of text at
//! 1.36 em dropped 0.3 em; a message of one to three emoji shows them at 40. "System"
//! (Settings › Chat › Emoji) draws the font's own glyphs instead: [`paint`] then returns
//! `false` and the caller draws the text. Names, shortcodes and groups come from the
//! `emojis` crate (Unicode CLDR and GitHub's shortcodes); only emoji with a picture are
//! offered while Twemoji is the style.
//!
//! The picker is drawn in ONE place: [`picker`], called at the end of `composer::show` (the
//! composer is shown whenever a conversation is open). It hangs from the rect last given
//! to [`set_anchor`] and does what `state.chat.picker` says:
//! - the composer's emoji button opens `Picker::Emoji { react_to: None }`: a pick is
//!   inserted at the field's cursor;
//! - a message's "Add reaction" sets `state.chat.picker =
//!   Some(Picker::Emoji { react_to: Some(id) })` and calls [`set_anchor`] with its button's
//!   rect: a pick sends `ChatCommand::React`.
//!
//! The GIF picker (`gif::picker`) reads the GIF button's rect with [`anchor`] the same way.
//! Every use of an emoji goes through [`bump`] (the counts in `state.chat.freq` drive
//! "Frequently used", the quick reactions and the order of `:` suggestions): a pick in the
//! picker or the `:` list, and a reaction added (as chat.html's `bump`).
//!
//! The shared signatures (`runs`, `jumbo`, `paint`, `name_of`, `by_name`, `Run`) change
//! only with the lead.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use easyplay_core::config::schema::{ChatEmoji, Preferences};
use easyplay_core::service::ChatCommand;
use egui::text::{CCursor, CCursorRange};
use egui::{
    Align2, Color32, ColorImage, Context, CornerRadius, Event, EventFilter, FontPriority,
    GlyphBitmap, GlyphRasterizer, GlyphRasterizerRequest, Id, Key, Order, Painter, RasterizedGlyph,
    Rect, Sense, Stroke, StrokeKind, TextureFilter, TextureHandle, TextureId, TextureOptions,
    UiBuilder, Vec2, has_emoji_presentation, pos2, vec2,
};
use twemoji_assets::png::PngTwemojiAsset;
use unicode_segmentation::UnicodeSegmentation;

use super::Env;
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::i18n::{self, Dir};
use crate::strings::chat::composer as s;
use crate::theme::motion::{appear, ease_out, scale_about};
use crate::theme::tokens::stroke;
use crate::theme::{self, Dur, Type, icons};
use crate::vm::chat::Picker;

// ---- Recognising emoji ---------------------------------------------------------------

/// A piece of text: plain text, or one emoji (a whole grapheme cluster).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Run<'a> {
    /// Text without emoji.
    Text(&'a str),
    /// One emoji.
    Emoji(&'a str),
}

/// The code points below U+1F000 that are shown as emoji without a variation selector
/// (Unicode's `Emoji_Presentation`). Others (©, ™, ❤, digits) stay text unless U+FE0F asks
/// for the emoji.
const PRESENTATION: &[(u32, u32)] = &[
    (0x231A, 0x231B),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
];

/// Whether this grapheme cluster is shown as an emoji.
fn is_emoji(g: &str) -> bool {
    let mut chars = g.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if g.contains('\u{FE0E}') {
        return false;
    }
    let sequence = chars.next().is_some();
    let looks = sequence
        || first as u32 >= 0x1F000
        || PRESENTATION
            .iter()
            .any(|&(a, b)| (a..=b).contains(&(first as u32)));
    looks && (picture(g).is_some() || emojis::get(g).is_some())
}

/// The Twemoji picture of `e`: as written, fully qualified, or without U+FE0F (the crate
/// keys `❤` bare and `❤️‍🔥` with it).
fn picture(e: &str) -> Option<&'static PngTwemojiAsset> {
    PngTwemojiAsset::from_emoji(e)
        .or_else(|| emojis::get(e).and_then(|q| PngTwemojiAsset::from_emoji(q.as_str())))
        .or_else(|| {
            if !e.contains('\u{FE0F}') {
                return None;
            }
            let bare: String = e.chars().filter(|&c| c != '\u{FE0F}').collect();
            PngTwemojiAsset::from_emoji(&bare)
        })
}

/// `text` split into runs of text and single emoji.
pub fn runs(text: &str) -> Vec<Run<'_>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (at, g) in text.grapheme_indices(true) {
        if is_emoji(g) {
            if start < at {
                out.push(Run::Text(&text[start..at]));
            }
            out.push(Run::Emoji(g));
            start = at + g.len();
        }
    }
    if start < text.len() {
        out.push(Run::Text(&text[start..]));
    }
    out
}

/// The text is one to three emoji and nothing else (shown at 40): how many.
pub fn jumbo(text: &str) -> Option<usize> {
    let mut n = 0;
    for r in runs(text) {
        match r {
            Run::Emoji(_) => n += 1,
            Run::Text(t) if t.trim().is_empty() => {}
            Run::Text(_) => return None,
        }
    }
    (1..=3).contains(&n).then_some(n)
}

// ---- Names, groups, search -------------------------------------------------------------

/// One emoji of the table (default skin tone).
struct Entry {
    /// The emoji, fully qualified.
    emoji: &'static str,
    /// Its short name for `:name:` (a GitHub shortcode, or one made from its Unicode name).
    name: String,
    /// Every name it answers to after `:` (the short name first).
    names: Vec<String>,
    /// Its Unicode name, lower case ("face with tears of joy").
    long: String,
    /// Its group.
    group: emojis::Group,
    /// There is a Twemoji picture for it.
    has_picture: bool,
}

struct Table {
    entries: Vec<Entry>,
    by_emoji: HashMap<&'static str, usize>,
    by_name: HashMap<String, usize>,
}

/// A short name made from a Unicode name: "flag: Côte d’Ivoire" → "flag_c_te_d_ivoire".
fn made_name(long: &str) -> String {
    let mut out = String::new();
    for c in long.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_owned()
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = Table {
            entries: Vec::new(),
            by_emoji: HashMap::new(),
            by_name: HashMap::new(),
        };
        for e in emojis::iter() {
            let mut names: Vec<String> = e.shortcodes().map(str::to_owned).collect();
            // `+1` reads worse than `thumbsup`: a name that starts with a letter or digit
            // comes first.
            names.sort_by_key(|n| !n.starts_with(|c: char| c.is_ascii_alphanumeric()));
            let long = e.name().to_lowercase();
            if names.is_empty() {
                names.push(made_name(&long));
            }
            let i = t.entries.len();
            let emoji = e.as_str();
            t.by_emoji.insert(emoji, i);
            for n in &names {
                t.by_name.entry(n.clone()).or_insert(i);
            }
            t.entries.push(Entry {
                emoji,
                name: names[0].clone(),
                names,
                long,
                group: e.group(),
                has_picture: picture(emoji).is_some(),
            });
        }
        t
    })
}

/// The table entry of `emoji` (any qualification; a skin tone counts as its base).
fn entry(emoji: &str) -> Option<&'static Entry> {
    let t = table();
    let i = t.by_emoji.get(emoji).copied().or_else(|| {
        let q = emojis::get(emoji)?;
        let base = q.with_skin_tone(emojis::SkinTone::Default).unwrap_or(q);
        t.by_emoji.get(base.as_str()).copied()
    })?;
    t.entries.get(i)
}

/// The emoji's short name (`thumbsup`), for `:name:` and the picker's footer.
pub fn name_of(emoji: &str) -> Option<&'static str> {
    entry(emoji).map(|e| e.name.as_str())
}

/// The emoji for a `:shortcode:` (any of its names).
#[allow(dead_code)] // Shared signature; nothing renders `:shortcodes:` now (SimpleX doesn't).
pub fn by_name(name: &str) -> Option<&'static str> {
    let t = table();
    t.by_name
        .get(name)
        .and_then(|&i| t.entries.get(i))
        .map(|e| e.emoji)
}

/// Whether the entry can be offered in the current style.
fn offered(e: &Entry, system: bool) -> bool {
    system || e.has_picture
}

/// The picker's groups in Unicode's order, each with its emoji.
fn groups(system: bool) -> &'static [(emojis::Group, Vec<&'static str>)] {
    static PICTURES: OnceLock<Vec<(emojis::Group, Vec<&'static str>)>> = OnceLock::new();
    static ALL: OnceLock<Vec<(emojis::Group, Vec<&'static str>)>> = OnceLock::new();
    let build = move || {
        emojis::Group::iter()
            .map(|g| {
                let list: Vec<&'static str> = table()
                    .entries
                    .iter()
                    .filter(|e| e.group == g && offered(e, system))
                    .map(|e| e.emoji)
                    .collect();
                (g, list)
            })
            .filter(|(_, l)| !l.is_empty())
            .collect()
    };
    if system {
        ALL.get_or_init(build)
    } else {
        PICTURES.get_or_init(build)
    }
}

/// A group's heading.
fn group_label(g: emojis::Group) -> &'static str {
    use emojis::Group as G;
    match g {
        G::SmileysAndEmotion => s::GROUP_SMILEYS(),
        G::PeopleAndBody => s::GROUP_PEOPLE(),
        G::AnimalsAndNature => s::GROUP_NATURE(),
        G::FoodAndDrink => s::GROUP_FOOD(),
        G::TravelAndPlaces => s::GROUP_TRAVEL(),
        G::Activities => s::GROUP_ACTIVITIES(),
        G::Objects => s::GROUP_OBJECTS(),
        G::Symbols => s::GROUP_SYMBOLS(),
        G::Flags => s::GROUP_FLAGS(),
    }
}

/// What a query looks for: lower case, without the colons of `:name:`.
fn normalise(query: &str) -> String {
    query
        .trim()
        .trim_matches(':')
        .trim()
        .to_lowercase()
        .replace(' ', "_")
}

/// The picker's search: names or Unicode names containing `query`; an exact name first,
/// then those that start with it.
pub fn search(query: &str, system: bool) -> Vec<&'static str> {
    let q = normalise(query);
    if q.is_empty() {
        return Vec::new();
    }
    let spaced = q.replace('_', " ");
    let mut exact = Vec::new();
    let mut starts = Vec::new();
    let mut contains = Vec::new();
    for e in table().entries.iter().filter(|e| offered(e, system)) {
        if e.names.contains(&q) || e.long == spaced {
            exact.push(e.emoji);
        } else if e.names.iter().any(|n| n.starts_with(&q)) || e.long.starts_with(&spaced) {
            starts.push(e.emoji);
        } else if e.names.iter().any(|n| n.contains(&q)) || e.long.contains(&spaced) {
            contains.push(e.emoji);
        }
    }
    exact.extend(starts);
    exact.extend(contains);
    exact
}

/// The `:` suggestions for what follows the colon: up to `n` emoji whose name, or a word of
/// whose Unicode name, starts with `query`; the most used first, then the shortest name.
/// Returns (emoji, its name as shown).
pub fn suggest(
    query: &str,
    freq: &HashMap<String, u32>,
    n: usize,
    system: bool,
) -> Vec<(&'static str, &'static str)> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(&'static str, &'static str)> = table()
        .entries
        .iter()
        .filter(|e| offered(e, system))
        .filter_map(|e| {
            let name = e.names.iter().find(|x| x.starts_with(&q));
            match name {
                Some(name) => Some((e.emoji, name.as_str())),
                None => e
                    .long
                    .split([' ', '-', ':'])
                    .any(|w| w.starts_with(&q))
                    .then_some((e.emoji, e.name.as_str())),
            }
        })
        .collect();
    let count = |e: &str| freq.get(&freq_key(e)).copied().unwrap_or(0);
    hits.sort_by(|a, b| count(b.0).cmp(&count(a.0)).then(a.1.len().cmp(&b.1.len())));
    hits.truncate(n);
    hits
}

// ---- How often each is used -------------------------------------------------------------

/// The key an emoji is counted under (fully qualified).
fn freq_key(e: &str) -> String {
    emojis::get(e).map_or(e, |q| q.as_str()).to_owned()
}

/// The user used `emoji` once more (sent, inserted or reacted with).
pub fn bump(freq: &mut HashMap<String, u32>, emoji: &str) {
    *freq.entry(freq_key(emoji)).or_default() += 1;
}

/// The `n` most used emoji, most used first ("Frequently used", the quick reactions).
pub fn top(freq: &HashMap<String, u32>, n: usize) -> Vec<String> {
    let mut all: Vec<(&String, &u32)> = freq.iter().filter(|(_, c)| **c > 0).collect();
    all.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    all.into_iter().take(n).map(|(e, _)| e.clone()).collect()
}

/// The quick reactions: the `n` most used that the engine takes (`allowed`, `None` = any),
/// filled up from `allowed` in its order, then from 👍 ❤️ 😂.
pub fn quick(freq: &HashMap<String, u32>, allowed: Option<&[String]>, n: usize) -> Vec<String> {
    let ok = |e: &str| allowed.is_none_or(|a| a.iter().any(|x| freq_key(x) == freq_key(e)));
    let mut out: Vec<String> = top(freq, usize::MAX)
        .into_iter()
        .filter(|e| ok(e))
        .take(n)
        .collect();
    let fill: Vec<String> = match allowed {
        Some(a) => a.iter().map(|e| freq_key(e)).collect(),
        None => ["👍", "❤️", "😂"].iter().map(|e| freq_key(e)).collect(),
    };
    for e in fill {
        if out.len() >= n {
            break;
        }
        if !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

// ---- Pictures -------------------------------------------------------------------------

/// Uploaded pictures, by (asset, greyed), and how many were decoded in this pass.
#[derive(Clone, Default)]
struct Textures {
    map: HashMap<(&'static str, bool), TextureHandle>,
    pass: u64,
    decoded: u32,
}

/// Pictures decoded in one pass at most (a picker opening shows ~80): the rest follow in
/// the next passes.
const DECODES_PER_PASS: u32 = 64;

/// Pictures kept uploaded at most.
const MAX_TEXTURES: usize = 400;

fn textures_id() -> Id {
    Id::unique("chat.emoji.textures")
}

/// The texture of `asset`, decoding and uploading it the first time (within the budget).
fn texture(ctx: &Context, asset: &'static PngTwemojiAsset, grey: bool) -> Option<TextureId> {
    let key = (asset.emoji, grey);
    let pass = ctx.cumulative_pass_nr();
    let (have, budget) = ctx.data_mut(|d| {
        let t = d.get_temp_mut_or_default::<Textures>(textures_id());
        if t.pass != pass {
            t.pass = pass;
            t.decoded = 0;
        }
        let have = t.map.get(&key).map(TextureHandle::id);
        let budget = have.is_none() && t.decoded < DECODES_PER_PASS;
        if budget {
            t.decoded += 1;
        }
        (have, budget)
    });
    if have.is_some() {
        return have;
    }
    if !budget {
        ctx.request_repaint();
        return None;
    }
    let mut image: ColorImage = egui_extras::image::load_image_bytes(asset.data.0).ok()?;
    if grey {
        for px in &mut image.pixels {
            let [r, g, b, a] = px.to_array();
            let l = (0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b))
                .round()
                .clamp(0.0, 255.0) as u8;
            *px = Color32::from_rgba_premultiplied(l, l, l, a);
        }
    }
    let options = TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear));
    let handle = ctx.load_texture(format!("emoji-{}", asset.emoji), image, options);
    let id = handle.id();
    ctx.data_mut(|d| {
        let t = d.get_temp_mut_or_default::<Textures>(textures_id());
        // At most a few hundred pictures stay on the GPU (a scroll through the picker would
        // keep thousands); the ones still shown come back within a few passes.
        if t.map.len() >= MAX_TEXTURES {
            t.map.clear();
        }
        t.map.insert(key, handle);
    });
    Some(id)
}

fn style_id() -> Id {
    Id::unique("chat.emoji.system")
}

/// Settings › Chat › Emoji says "System".
pub fn system_style(prefs: &Preferences) -> bool {
    prefs.chat.emoji == ChatEmoji::System
}

/// Take the emoji style from the preferences (the composer does it every pass; the Chat
/// page may do it earlier in the pass).
pub fn set_system(ctx: &Context, system: bool) {
    let was: Option<bool> = ctx.data(|d| d.get_temp(style_id()));
    ctx.data_mut(|d| d.insert_temp(style_id(), system));
    if was != Some(system) {
        SYSTEM.store(system, Ordering::Relaxed);
        // Replaced, not added: the glyph cache is rebuilt for the new style.
        ctx.set_glyph_rasterizers(rasterizers());
    }
}

/// Make sure the Twemoji glyphs are installed (in the style last set; Twemoji at first).
/// The app calls [`set_system`] with the preference when it starts and when it changes.
#[allow(dead_code)] // The app's start (the lead); `line` calls it too.
pub fn install(ctx: &Context) {
    for r in rasterizers() {
        ctx.add_glyph_rasterizer(r);
    }
}

/// The emoji are drawn by the font.
pub fn system(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(style_id())).unwrap_or(false)
}

fn paint_with(painter: &Painter, rect: Rect, emoji: &str, grey: bool) -> bool {
    if system(painter.ctx()) {
        return false;
    }
    let Some(asset) = picture(emoji) else {
        return false;
    };
    if let Some(id) = texture(painter.ctx(), asset, grey) {
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.image(id, rect, uv, Color32::WHITE);
    }
    true
}

/// Paint `emoji` filling `rect`. `false` when there is no picture for it (the caller draws
/// the text instead).
pub fn paint(painter: &Painter, rect: Rect, emoji: &str) -> bool {
    paint_with(painter, rect, emoji, false)
}

/// Paint `emoji` filling `rect`: its picture, or the font's glyph (System, or no picture).
pub fn draw(painter: &Painter, rect: Rect, emoji: &str, color: Color32) {
    if !paint(painter, rect, emoji) {
        glyph(painter, rect, emoji, color);
    }
}

fn glyph(painter: &Painter, rect: Rect, emoji: &str, color: Color32) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        emoji,
        egui::FontId::proportional(rect.height() * 0.9),
        color,
    );
}

// ---- Emoji as glyphs and one-line text ---------------------------------------------------

/// The Twemoji picture of `req.cluster` as a colour glyph 1.25 em square, dropped 0.22 em
/// below the baseline (CSS `.em`: 1.36 em, −0.3 em, fitted to the line). `any`: also for
/// clusters without emoji presentation (the fallback, for what no font has).
fn raster(req: &GlyphRasterizerRequest<'_>, any: bool) -> Option<RasterizedGlyph> {
    let c = req.cluster;
    if c.is_ascii() {
        return None;
    }
    if !any && (SYSTEM.load(Ordering::Relaxed) || !has_emoji_presentation(c)) {
        return None;
    }
    let asset = picture(c)?;
    let font = req.font_size_px.max(1.0);
    let side = (font * 1.25).round().max(1.0) as u32;
    let png = image::load_from_memory_with_format(asset.data.0, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let scaled = if side < png.width() {
        image::imageops::thumbnail(&png, side, side)
    } else {
        image::imageops::resize(&png, side, side, image::imageops::FilterType::CatmullRom)
    };
    let image = ColorImage::from_rgba_unmultiplied([side as usize, side as usize], scaled.as_raw());
    let drop = (font * 0.22).round();
    let bearing = (font * 0.05).round();
    Some(RasterizedGlyph {
        bitmap: GlyphBitmap {
            image,
            offset_px: vec2(bearing, drop - side as f32),
            is_color: true,
        },
        advance_px: side as f32 + 2.0 * bearing,
    })
}

/// The emoji style the glyphs were made for (the rasterizer runs inside the font system).
static SYSTEM: AtomicBool = AtomicBool::new(false);

/// Twemoji as glyphs, so any text egui lays out (a field, a label, a galley) shows emoji:
/// before the fonts for clusters with emoji presentation (not under "System"), and after
/// them for anything else no font has.
fn rasterizers() -> Vec<GlyphRasterizer> {
    vec![
        GlyphRasterizer::new("easyplay::twemoji", |r| raster(r, false))
            .with_priority(FontPriority::Highest),
        GlyphRasterizer::new("easyplay::twemoji.fallback", |r| raster(r, true)),
    ]
}

/// One line of text people wrote (a name, a preview, a status): the emoji are pictures 1 pt
/// larger than the text (`.cr-t .em`), dropped 1, the text between them is laid out as text,
/// and the whole is cut with "…" at its width. Right-to-left text runs from the right.
pub struct Line {
    pieces: Vec<(f32, Piece)>,
    size: Vec2,
    em: f32,
    rtl: bool,
}

/// A piece of a [`Line`].
enum Piece {
    Text(std::sync::Arc<egui::Galley>),
    Emoji(String),
}

impl Line {
    /// Lay `text` out in `style` and `color`, at most `width` wide.
    pub fn new(ui: &egui::Ui, style: Type, text: &str, color: Color32, width: f32) -> Self {
        let em = style.size() + 1.0;
        let mut pieces = Vec::new();
        let mut x = 0.0_f32;
        for run in runs(text) {
            let room = width - x;
            if room <= 1.0 {
                break;
            }
            match run {
                Run::Text(t) => {
                    let g = style.galley(ui, t, color, room, 1);
                    let w = g.size().x;
                    let cut = g.elided;
                    pieces.push((x, Piece::Text(g)));
                    x += w;
                    if cut {
                        break;
                    }
                }
                Run::Emoji(e) => {
                    if em > room {
                        let g = style.galley(ui, "…", color, f32::INFINITY, 1);
                        let w = g.size().x;
                        pieces.push((x, Piece::Text(g)));
                        x += w;
                        break;
                    }
                    pieces.push((x, Piece::Emoji(e.to_owned())));
                    x += em;
                }
            }
        }
        Self {
            pieces,
            size: vec2(x, style.line_height()),
            em,
            rtl: super::paint::reads_rtl(text),
        }
    }

    /// Its size.
    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Paint the line with its top left at `at`.
    pub fn paint(self, painter: &Painter, at: egui::Pos2, color: Color32) {
        let h = self.size.y;
        let total = self.size.x;
        for (x, piece) in self.pieces {
            let w = match &piece {
                Piece::Text(g) => g.size().x,
                Piece::Emoji(_) => self.em,
            };
            let x = if self.rtl { total - x - w } else { x };
            match piece {
                Piece::Text(g) => {
                    let y = at.y + (h - g.size().y) / 2.0;
                    painter.galley(pos2(at.x + x, y), g, color);
                }
                Piece::Emoji(e) => {
                    let r = Rect::from_center_size(
                        pos2(at.x + x + self.em / 2.0, at.y + h / 2.0 + 1.0),
                        Vec2::splat(self.em),
                    );
                    draw(painter, r, &e, color);
                }
            }
        }
    }
}

/// [`Line`] as a widget: allocated in `ui` (at the start edge), painted, read out as
/// `text`. Hover the response for a tooltip when the line was cut.
#[allow(dead_code)] // Shared: the list and details.
pub fn line(
    ui: &mut egui::Ui,
    text: &str,
    style: Type,
    color: Color32,
    max_w: f32,
) -> egui::Response {
    install(ui.ctx());
    let l = Line::new(ui, style, text, color, max_w);
    let (rect, resp) = ui.allocate_exact_size(l.size(), Sense::hover());
    crate::a11y::item(&resp, egui::Role::Label, text);
    l.paint(ui.painter(), rect.min, color);
    resp
}

// ---- The picker ---------------------------------------------------------------------------

/// The picker's measures (CSS `.pop`, `.ep`, `.pop-top`, `.ep-tabs`, `.pop-h`, `.ep-g`,
/// `.pop-f`).
mod m {
    pub const W: f32 = 344.0;
    pub const H: f32 = 408.0;
    pub const RADIUS: u8 = 14;
    /// Between the picker and its button, and from the window's edges.
    pub const GAP: f32 = 8.0;
    /// The picker's right edge stands this far past its button's.
    pub const OVERHANG: f32 = 4.0;
    pub const TOP_PAD: [f32; 3] = [10.0, 10.0, 8.0];
    pub const SEARCH_H: f32 = 30.0;
    pub const SEARCH_PAD: [f32; 2] = [9.0, 8.0];
    pub const SEARCH_GAP: f32 = 7.0;
    pub const SEARCH_RADIUS: u8 = 8;
    pub const TAB_H: f32 = 28.0;
    pub const TABS_PAD: f32 = 8.0;
    pub const TABS_BELOW: f32 = 6.0;
    pub const TAB_GAP: f32 = 2.0;
    pub const TAB_RADIUS: u8 = 7;
    pub const TAB_EMOJI: f32 = 17.0;
    pub const GRID_PAD: f32 = 8.0;
    pub const COLS: usize = 8;
    pub const CELL: f32 = 41.0;
    pub const CELL_RADIUS: u8 = 8;
    pub const EMOJI: f32 = 24.0;
    pub const HEAD_H: f32 = 29.0;
    pub const HEAD_TOP: f32 = 10.0;
    pub const HEAD_X: f32 = 4.0;
    pub const FOOT_H: f32 = 44.0;
    pub const FOOT_PAD: f32 = 12.0;
    pub const FOOT_GAP: f32 = 10.0;
    pub const FOOT_EMOJI: f32 = 26.0;
    pub const NONE_PAD: f32 = 28.0;
    pub const FREQUENT: usize = 16;
    /// Where the picker grows from: scale .97 → 1, 6 below its place.
    pub const FROM_SCALE: f32 = 0.97;
    pub const FROM_DROP: f32 = 6.0;
    /// Greyed tabs.
    pub const TAB_DIM: f32 = 0.6;
}

/// Where the open popover hangs from (its button), published by whoever opens it.
pub fn set_anchor(ctx: &Context, rect: Rect) {
    ctx.data_mut(|d| d.insert_temp(anchor_id(), rect));
}

/// The rect of the button the open popover hangs from.
pub fn anchor(ctx: &Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp(anchor_id()))
}

fn anchor_id() -> Id {
    Id::unique("chat.picker.anchor")
}

/// The picker's place: above its button (below when there is more room there), the right
/// edges aligned (the left ones in a right-to-left layout), inside the window. Returns the
/// rect and whether it opens upwards.
pub fn place(screen: Rect, anchor: Rect, size: Vec2, rtl: bool) -> (Rect, bool) {
    let up = anchor.top() - size.y - m::GAP >= screen.top() + m::GAP
        || anchor.top() - screen.top() > screen.bottom() - anchor.bottom();
    let x = if rtl {
        anchor.left() - m::OVERHANG
    } else {
        anchor.right() - size.x + m::OVERHANG
    };
    let x = x.clamp(
        screen.left() + m::GAP,
        (screen.right() - size.x - m::GAP).max(screen.left() + m::GAP),
    );
    let y = if up {
        (anchor.top() - size.y - m::GAP).max(screen.top() + m::GAP)
    } else {
        (anchor.bottom() + m::GAP).min(screen.bottom() - size.y - m::GAP)
    };
    (Rect::from_min_size(pos2(x, y), size), up)
}

/// The picker's own state between passes.
#[derive(Clone, Default)]
struct PickerUi {
    query: String,
    /// The grid cell that has the keyboard (index into the flat list).
    cell: Option<usize>,
    /// The scroll offset of the last pass.
    offset: f32,
    /// Scroll to this offset in the next pass (a tab was clicked).
    jump: Option<f32>,
    /// The last pass the picker was shown.
    pass: u64,
    /// Passes left in which the search field asks for focus (the click that opened the
    /// picker counts as a click elsewhere for the field, which would drop it at once).
    focus_search: u8,
}

/// A section of the grid.
struct Section {
    label: String,
    items: Vec<&'static str>,
    /// Top of its heading in the scrolled content.
    y: f32,
}

impl Section {
    fn height(&self) -> f32 {
        m::HEAD_H + self.items.len().div_ceil(m::COLS) as f32 * m::CELL
    }
}

/// The emoji picker, when `state.chat.picker` is `Picker::Emoji`. See the module's header.
pub(super) fn picker(ctx: &Context, state: &mut UiState, env: &mut Env<'_>) {
    let Some(Picker::Emoji { react_to }) = state.chat.picker.clone() else {
        return;
    };
    let Some(chat) = state.chat.open.as_ref().map(|c| c.chat) else {
        state.chat.picker = None;
        return;
    };
    let system = system(ctx);
    let id = Id::unique("chat.emoji.picker");
    let search_id = id.with("search");
    let grid_id = id.with("grid");
    let pass = ctx.cumulative_pass_nr();
    let mut ps: PickerUi = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let fresh = ps.pass == 0 || ps.pass + 1 < pass;
    if fresh {
        ps = PickerUi {
            focus_search: 2,
            ..PickerUi::default()
        };
    }
    if ps.focus_search > 0 {
        ps.focus_search -= 1;
        ctx.memory_mut(|mem| mem.request_focus(search_id));
    }
    ps.pass = pass;

    let screen = ctx.content_rect();
    let anchor = anchor(ctx).unwrap_or(Rect::from_center_size(screen.center(), Vec2::ZERO));
    let rtl = i18n::dir() == Dir::Rtl;
    let (rect, up) = place(screen, anchor, vec2(m::W, m::H), rtl);

    // The sections: frequently used and the groups, or the results of a search, or (a
    // reaction, where the engine takes only some emoji) those emoji.
    let allowed = react_to
        .and_then(|_| env.bridge.chat().reaction_emoji.clone())
        .map(|list| {
            let hits = search(&ps.query, true);
            list.iter()
                .filter_map(|e| entry(e).map(|x| x.emoji))
                .filter(|e| ps.query.trim().is_empty() || hits.contains(e))
                .collect::<Vec<_>>()
        });
    let searching = !ps.query.trim().is_empty() || allowed.is_some();
    let mut sections: Vec<Section> = Vec::new();
    if let Some(items) = allowed {
        if !items.is_empty() {
            sections.push(Section {
                label: s::EMOJI().to_owned(),
                items,
                y: 0.0,
            });
        }
    } else if searching {
        let hits = search(&ps.query, system);
        if !hits.is_empty() {
            sections.push(Section {
                label: s::RESULTS().to_owned(),
                items: hits,
                y: 0.0,
            });
        }
    } else {
        let frequent: Vec<&'static str> = top(&state.chat.freq, m::FREQUENT)
            .iter()
            .filter_map(|e| entry(e).map(|x| x.emoji))
            .collect();
        if !frequent.is_empty() {
            sections.push(Section {
                label: s::FREQUENT().to_owned(),
                items: frequent,
                y: 0.0,
            });
        }
        for (g, list) in groups(system) {
            sections.push(Section {
                label: group_label(*g).to_owned(),
                items: list.clone(),
                y: 0.0,
            });
        }
    }
    let mut y = 0.0;
    for sec in &mut sections {
        sec.y = y;
        y += sec.height();
    }
    let content_h = y + m::GRID_PAD;
    let flat: Vec<&'static str> = sections.iter().flat_map(|s| s.items.clone()).collect();

    // Keys: Esc closes; ↓ from the search goes into the grid; arrows move; Enter picks;
    // typing in the grid goes back to the search.
    let search_focused = ctx.memory(|mem| mem.has_focus(search_id));
    let mut picked: Option<(&'static str, bool)> = None;
    let mut close = false;
    let mut refocus = false;
    ctx.input_mut(|i| {
        let mut keep = Vec::with_capacity(i.events.len());
        for ev in std::mem::take(&mut i.events) {
            let used = match &ev {
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    close = true;
                    true
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if search_focused => match key {
                    Key::ArrowDown if !flat.is_empty() => {
                        ps.cell = Some(0);
                        true
                    }
                    Key::Enter => {
                        if let Some(e) = flat.first() {
                            picked = Some((e, modifiers.shift));
                        }
                        true
                    }
                    _ => false,
                },
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if ps.cell.is_some() => {
                    let at = ps.cell.unwrap_or(0) as isize;
                    let cols = m::COLS as isize;
                    let (left, right) = if rtl { (1, -1) } else { (-1, 1) };
                    let to = match key {
                        Key::ArrowLeft => Some(at + left),
                        Key::ArrowRight => Some(at + right),
                        Key::ArrowUp => Some(at - cols),
                        Key::ArrowDown => Some(at + cols),
                        Key::Home => Some(0),
                        Key::End => Some(flat.len() as isize - 1),
                        _ => None,
                    };
                    match (key, to) {
                        (_, Some(to)) if to < 0 && *key == Key::ArrowUp => {
                            ps.cell = None;
                            refocus = true;
                            true
                        }
                        (_, Some(to)) => {
                            let last = flat.len().saturating_sub(1) as isize;
                            ps.cell = Some(to.clamp(0, last) as usize);
                            true
                        }
                        (Key::Enter | Key::Space, None) => {
                            if let Some(e) = flat.get(at as usize) {
                                picked = Some((e, modifiers.shift));
                            }
                            true
                        }
                        _ => false,
                    }
                }
                Event::Text(t) if ps.cell.is_some() && !t.trim().is_empty() => {
                    ps.query.push_str(t);
                    ps.cell = None;
                    refocus = true;
                    true
                }
                _ => false,
            };
            if !used {
                keep.push(ev);
            }
        }
        i.events = keep;
    });
    if refocus {
        ctx.memory_mut(|mem| mem.request_focus(search_id));
    }
    if ps.cell.is_some() {
        // The grid keeps the keyboard: arrows and Esc are its own, and the page's
        // "typing goes to the field" leaves it alone.
        ctx.memory_mut(|mem| {
            mem.request_focus(grid_id);
            mem.set_focus_lock_filter(
                grid_id,
                EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            );
        });
    }

    let th = theme::theme(ctx);
    let p = th.palette.clone();
    let motion = th.motion;
    let alpha = appear(ctx, id.with("in"), motion.fade(Dur::Fast));
    let t = ease_out(appear(ctx, id.with("move"), motion.movement(Dur::Fast)));
    let pivot = pos2(
        anchor.center().x.clamp(rect.left(), rect.right()),
        if up { rect.bottom() } else { rect.top() },
    );
    let transform = scale_about(
        pivot,
        1.0 - (1.0 - m::FROM_SCALE) * (1.0 - t),
        vec2(0.0, m::FROM_DROP * (1.0 - t)),
    );

    let mut hover: Option<&'static str> = None;
    let area = egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.with_visual_transform(transform, |ui| {
                ui.multiply_opacity(alpha);
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                // Clicks inside never reach what is under the picker.
                ui.interact(rect, id.with("plate"), Sense::click());
                crate::a11y::labelled(ui.ctx(), id, egui::Role::Dialog, s::EMOJI());
                let painter = ui.painter().clone();
                painter.add(
                    p.shade_pop
                        .shadow()
                        .as_shape(rect, CornerRadius::same(m::RADIUS)),
                );
                painter.rect_filled(rect, CornerRadius::same(m::RADIUS), p.raised);

                // Search.
                let sr = Rect::from_min_max(
                    rect.min + vec2(m::TOP_PAD[1], m::TOP_PAD[0]),
                    pos2(
                        rect.right() - m::TOP_PAD[1],
                        rect.top() + m::TOP_PAD[0] + m::SEARCH_H,
                    ),
                );
                let before = ps.query.clone();
                search_field(ui, sr, search_id, &mut ps.query, s::SEARCH_EMOJI(), true);
                if ps.query != before {
                    ps.cell = None;
                    ps.jump = Some(0.0);
                }

                // Tabs.
                let mut body_top = sr.bottom() + m::TOP_PAD[2];
                if !searching {
                    let row = Rect::from_min_size(
                        pos2(rect.left() + m::TABS_PAD, body_top),
                        vec2(m::W - 2.0 * m::TABS_PAD, m::TAB_H),
                    );
                    let mut tabs: Vec<(Option<&'static str>, String, f32)> = Vec::new();
                    let freq_y = sections
                        .iter()
                        .find(|x| x.label == s::FREQUENT())
                        .map_or(0.0, |x| x.y);
                    tabs.push((None, s::FREQUENT().to_owned(), freq_y));
                    for sec in sections.iter().filter(|x| x.label != s::FREQUENT()) {
                        if let Some(first) = sec.items.first() {
                            tabs.push((Some(first), sec.label.clone(), sec.y));
                        }
                    }
                    let n = tabs.len() as f32;
                    let tab_w = ((row.width() - m::TAB_GAP * (n - 1.0)) / n).min(32.0);
                    // The section at the top of the grid is the pressed tab.
                    let current = sections
                        .iter()
                        .rposition(|x| x.y <= ps.offset + 1.0)
                        .map(|i| sections[i].y);
                    for (k, (first, label, sy)) in tabs.iter().enumerate() {
                        let x = if rtl {
                            row.right() - (k as f32 + 1.0) * tab_w - k as f32 * m::TAB_GAP
                        } else {
                            row.left() + k as f32 * (tab_w + m::TAB_GAP)
                        };
                        let r = Rect::from_min_size(pos2(x, row.top()), vec2(tab_w, m::TAB_H));
                        let resp = ui.interact(r, id.with(("tab", k)), Sense::click());
                        let on = resp.hovered() || current == Some(*sy);
                        if on {
                            painter.rect_filled(r, CornerRadius::same(m::TAB_RADIUS), p.o1);
                        }
                        let er = Rect::from_center_size(r.center(), Vec2::splat(m::TAB_EMOJI));
                        match first {
                            None => {
                                painter.text(
                                    r.center(),
                                    Align2::CENTER_CENTER,
                                    icons::CLOCK.text(false),
                                    icons::font(m::TAB_EMOJI),
                                    if on { p.t1 } else { p.t3 },
                                );
                            }
                            Some(e) => {
                                let mut dim = painter.clone();
                                dim.multiply_opacity(if on { 1.0 } else { m::TAB_DIM });
                                if !paint_with(&dim, er, e, !on) {
                                    glyph(&dim, er, e, p.t1);
                                }
                            }
                        }
                        crate::a11y::button(&resp, label);
                        let resp = crate::widgets::tooltip::attach(resp, label);
                        if resp.clicked() {
                            ps.jump = Some(*sy);
                        }
                    }
                    body_top = row.bottom() + m::TABS_BELOW;
                    painter.hline(
                        rect.x_range(),
                        body_top - stroke::BORDER / 2.0,
                        Stroke::new(stroke::BORDER, p.line),
                    );
                }

                // The grid.
                let foot_top = rect.bottom() - m::FOOT_H;
                let body =
                    Rect::from_min_max(pos2(rect.left(), body_top), pos2(rect.right(), foot_top));
                let grid_resp = ui.interact(body, grid_id, Sense::focusable_noninteractive());
                if ps.cell.is_some() && !grid_resp.has_focus() {
                    grid_resp.request_focus();
                }
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(body.intersect(ui.clip_rect()));
                if sections.is_empty() {
                    let msg = s::no_emoji(ps.query.trim());
                    let g =
                        Type::Body.galley(&child, &msg, p.t3, body.width() - 2.0 * m::FOOT_PAD, 3);
                    let at = pos2(body.center().x - g.size().x / 2.0, body.top() + m::NONE_PAD);
                    painter.galley(at, g, p.t3);
                } else {
                    let mut area = egui::ScrollArea::vertical()
                        .id_salt(id.with("scroll"))
                        .auto_shrink([false, false]);
                    if let Some(y) = ps.jump.take() {
                        area = area.vertical_scroll_offset(y);
                    }
                    let out = area.show_viewport(&mut child, |ui, vp| {
                        ui.set_height(content_h);
                        let origin = ui.min_rect().min;
                        let grid_left = origin.x + m::GRID_PAD;
                        let grid_w = m::COLS as f32 * m::CELL;
                        let cell_x = |col: usize| {
                            if rtl {
                                grid_left + grid_w - (col as f32 + 1.0) * m::CELL
                            } else {
                                grid_left + col as f32 * m::CELL
                            }
                        };
                        let shift = ui.input(|i| i.modifiers.shift);
                        let mut base = 0;
                        for sec in &sections {
                            let top = sec.y;
                            let bottom = sec.y + sec.height();
                            if bottom >= vp.top() && top <= vp.bottom() {
                                header(ui.painter(), &p, origin, rect, sec, top, rtl);
                                for (k, e) in sec.items.iter().enumerate() {
                                    let cy = top + m::HEAD_H + (k / m::COLS) as f32 * m::CELL;
                                    if cy + m::CELL < vp.top() || cy > vp.bottom() {
                                        continue;
                                    }
                                    let r = Rect::from_min_size(
                                        pos2(cell_x(k % m::COLS), origin.y + cy),
                                        Vec2::splat(m::CELL),
                                    );
                                    let index = base + k;
                                    let resp =
                                        ui.interact(r, id.with(("cell", index)), Sense::click());
                                    let kb = ps.cell == Some(index);
                                    if kb {
                                        ui.scroll_to_rect(r, None);
                                    }
                                    if resp.hovered() || kb {
                                        ui.painter().rect_filled(
                                            r,
                                            CornerRadius::same(m::CELL_RADIUS),
                                            p.o2,
                                        );
                                    }
                                    if resp.hovered() || (kb && hover.is_none()) {
                                        hover = Some(e);
                                    }
                                    let er =
                                        Rect::from_center_size(r.center(), Vec2::splat(m::EMOJI));
                                    draw(ui.painter(), er, e, p.t1);
                                    let name = name_of(e).unwrap_or_default().replace('_', " ");
                                    crate::a11y::button(&resp, &name);
                                    if resp.clicked() {
                                        picked = Some((e, shift));
                                    }
                                }
                            }
                            base += sec.items.len();
                        }
                    });
                    ps.offset = out.state.clamped_offset().y;
                    // The heading of the section at the top stays put until the next one
                    // pushes it up.
                    let off = ps.offset;
                    if off > 0.0
                        && let Some(i) = sections.iter().rposition(|x| x.y <= off)
                    {
                        let push = sections
                            .get(i + 1)
                            .map_or(0.0, |n| (n.y - off - m::HEAD_H).min(0.0));
                        let clip = painter.clone().with_clip_rect(body);
                        let origin = pos2(body.left(), body.top() - sections[i].y + push);
                        let band = Rect::from_min_size(
                            pos2(body.left(), body.top() + push),
                            vec2(body.width(), m::HEAD_H),
                        );
                        clip.rect_filled(band, CornerRadius::ZERO, p.raised);
                        header(&clip, &p, origin, rect, &sections[i], sections[i].y, rtl);
                    }
                }

                // Footer.
                painter.hline(
                    rect.x_range(),
                    foot_top + stroke::BORDER / 2.0,
                    Stroke::new(stroke::BORDER, p.line),
                );
                let cy = foot_top + m::FOOT_H / 2.0;
                let start = if rtl {
                    rect.right() - m::FOOT_PAD
                } else {
                    rect.left() + m::FOOT_PAD
                };
                let dir = if rtl { -1.0 } else { 1.0 };
                match hover {
                    Some(e) => {
                        let er = Rect::from_center_size(
                            pos2(start + dir * m::FOOT_EMOJI / 2.0, cy),
                            Vec2::splat(m::FOOT_EMOJI),
                        );
                        draw(&painter, er, e, p.t1);
                        let label = format!(":{}:", name_of(e).unwrap_or_default());
                        let x = start + dir * (m::FOOT_EMOJI + m::FOOT_GAP);
                        painter.text(
                            pos2(x, cy),
                            if rtl {
                                Align2::RIGHT_CENTER
                            } else {
                                Align2::LEFT_CENTER
                            },
                            label,
                            Type::Label.font_id(),
                            p.t1,
                        );
                    }
                    None => {
                        let g = Type::Caption.galley(
                            ui,
                            s::SHIFT_HINT(),
                            p.t3,
                            m::W - 2.0 * m::FOOT_PAD,
                            1,
                        );
                        let x = if rtl { start - g.size().x } else { start };
                        painter.galley(pos2(x, cy - g.size().y / 2.0), g, p.t3);
                    }
                }
                painter.rect_stroke(
                    rect,
                    CornerRadius::same(m::RADIUS),
                    Stroke::new(stroke::BORDER, p.line),
                    StrokeKind::Inside,
                );
            });
        });

    // A press outside (not on the button, which toggles it) closes the picker.
    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|at| !area.response.rect.contains(at) && !anchor.contains(at))
    });

    if let Some((e, keep)) = picked {
        match react_to {
            None => {
                bump(&mut state.chat.freq, e);
                super::composer::insert(ctx, state, chat, e);
            }
            Some(item) => {
                let mine = state
                    .chat
                    .open
                    .as_ref()
                    .and_then(|c| c.messages.iter().find(|x| x.id == item))
                    .is_some_and(|x| {
                        x.reactions
                            .iter()
                            .any(|r| r.mine && freq_key(&r.emoji) == freq_key(e))
                    });
                if !mine {
                    bump(&mut state.chat.freq, e);
                }
                env.bridge.send(UiCommand::Chat(ChatCommand::React {
                    chat,
                    item,
                    emoji: e.to_owned(),
                    on: !mine,
                }));
            }
        }
        if !keep {
            close = true;
        }
    }
    if close || outside {
        state.chat.picker = None;
        if react_to.is_none() && !outside {
            state.chat.focus_composer = true;
        }
        ps = PickerUi::default();
    }
    ctx.data_mut(|d| d.insert_temp(id, ps));
}

/// A picker's search field (CSS `.pop-top .search`): a 30 pt plate, o1 at rest and the well
/// with a 1.5 accent mark while focused, the glyph 9 in, the text 7 after it, centred in the
/// plate's height. Shared by the emoji and GIF pickers; `enabled: false` greys it (search
/// turned off). Returns the text field's response.
pub fn search_field(
    ui: &mut egui::Ui,
    plate: Rect,
    id: Id,
    query: &mut String,
    hint: &str,
    enabled: bool,
) -> egui::Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    let rtl = i18n::dir() == Dir::Rtl;
    let focused = ui.ctx().memory(|mem| mem.has_focus(id));
    let painter = ui.painter().clone();
    painter.rect(
        plate,
        CornerRadius::same(m::SEARCH_RADIUS),
        if focused { p.well } else { p.o1 },
        if focused {
            Stroke::new(stroke::FIELD_FOCUS, p.accent_mark)
        } else {
            Stroke::NONE
        },
        StrokeKind::Inside,
    );
    let icon = theme::tokens::icon::INLINE;
    let icon_x = if rtl {
        plate.right() - m::SEARCH_PAD[0] - icon / 2.0
    } else {
        plate.left() + m::SEARCH_PAD[0] + icon / 2.0
    };
    painter.text(
        pos2(icon_x, plate.center().y),
        Align2::CENTER_CENTER,
        icons::SEARCH.text(false),
        icons::font(icon),
        if enabled { p.t3 } else { p.t4 },
    );
    let text_w = plate.width() - m::SEARCH_PAD[0] - icon - m::SEARCH_GAP - m::SEARCH_PAD[1];
    let tx = if rtl {
        plate.left() + m::SEARCH_PAD[1]
    } else {
        plate.left() + m::SEARCH_PAD[0] + icon + m::SEARCH_GAP
    };
    // The whole height of the plate: the field centres its line in it (a rect one line high
    // would put the line below the glyph).
    let tr = Rect::from_min_size(pos2(tx, plate.top()), vec2(text_w, plate.height()));
    let edit = egui::TextEdit::singleline(query)
        .id(id)
        .interactive(enabled)
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(Type::Body.font_id())
        .text_color(p.t1)
        .hint_text(
            egui::RichText::new(i18n::bidi::visual_line(hint, None)).color(if enabled {
                p.t3
            } else {
                p.t4
            }),
        )
        .desired_width(text_w)
        .align(if rtl {
            Align2::RIGHT_CENTER
        } else {
            Align2::LEFT_CENTER
        });
    ui.put(tr, edit).accessible_name(hint)
}

/// A section heading (11/14 500 t3), its content's top at `origin`.
fn header(
    painter: &Painter,
    p: &theme::palette::Palette,
    origin: egui::Pos2,
    picker: Rect,
    sec: &Section,
    top: f32,
    rtl: bool,
) {
    let y = origin.y + top + m::HEAD_TOP;
    let (x, align) = if rtl {
        (picker.right() - m::GRID_PAD - m::HEAD_X, Align2::RIGHT_TOP)
    } else {
        (origin.x + m::GRID_PAD + m::HEAD_X, Align2::LEFT_TOP)
    };
    painter.text(
        pos2(x, y),
        align,
        // Painted text has no bidirectional layout: put it in visual order first.
        i18n::bidi::visual_line(&sec.label, None),
        Type::MicroStrong.font_id(),
        p.t3,
    );
}

/// Put `text` at the cursor of the text field `id` holding `buf` (the selection is
/// replaced); the cursor ends after it. Cursor positions count characters.
pub fn insert_at_cursor(ctx: &Context, id: Id, buf: &mut String, text: &str) {
    let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    let len = buf.chars().count();
    let (a, b) = st.cursor.char_range().map_or((len, len), |r| {
        let [x, y] = r.sorted_cursors();
        (x.index.0.min(len), y.index.0.min(len))
    });
    let byte = |i: usize| buf.char_indices().nth(i).map_or(buf.len(), |(b, _)| b);
    let (ba, bb) = (byte(a), byte(b));
    buf.replace_range(ba..bb, text);
    let at = a + text.chars().count();
    st.cursor
        .set_char_range(Some(CCursorRange::one(CCursor::new(at))));
    st.store(ctx, id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_split_text_and_emoji() {
        assert_eq!(
            runs("gg 🎉 well played"),
            vec![
                Run::Text("gg "),
                Run::Emoji("🎉"),
                Run::Text(" well played")
            ]
        );
        assert_eq!(runs("👍🏽👨‍👩‍👧"), vec![Run::Emoji("👍🏽"), Run::Emoji("👨‍👩‍👧")]);
        assert_eq!(runs("🇸🇪!"), vec![Run::Emoji("🇸🇪"), Run::Text("!")]);
        assert_eq!(
            runs("❤️ and 1️⃣"),
            vec![Run::Emoji("❤️"), Run::Text(" and "), Run::Emoji("1️⃣")]
        );
    }

    #[test]
    fn text_symbols_stay_text() {
        for t in ["1", "#", "*", "©", "™", "®", "❤", "a\u{301}", "مرحبا"] {
            assert_eq!(runs(t), vec![Run::Text(t)], "{t:?}");
        }
        assert_eq!(runs(""), Vec::<Run<'_>>::new());
    }

    #[test]
    fn jumbo_counts_one_to_three() {
        assert_eq!(jumbo("🎉"), Some(1));
        assert_eq!(jumbo(" 🎉 🎉🎉 "), Some(3));
        assert_eq!(jumbo("🎉🎉🎉🎉"), None);
        assert_eq!(jumbo("🎉 yes"), None);
        assert_eq!(jumbo(""), None);
        assert_eq!(jumbo("   "), None);
    }

    #[test]
    fn pictures_are_found_in_any_qualification() {
        for e in [
            "❤",
            "❤️",
            "1️⃣",
            "👍🏽",
            "👨‍👩‍👧",
            "🇸🇪",
            "❤️‍🔥",
            "☺",
            "☺️",
            "🐿",
            "🐿️",
        ] {
            assert!(picture(e).is_some(), "{e:?}");
        }
        assert!(picture("a").is_none());
    }

    #[test]
    fn names_and_shortcodes() {
        assert_eq!(name_of("👍"), Some("thumbsup"));
        assert_eq!(name_of("👍🏽"), Some("thumbsup"));
        assert_eq!(name_of("❤"), name_of("❤️"));
        assert_eq!(by_name("thumbsup"), Some("👍"));
        assert_eq!(by_name("+1"), Some("👍"));
        assert_eq!(by_name("joy"), Some("😂"));
        assert_eq!(by_name("tada"), Some("🎉"));
        assert_eq!(by_name("no such emoji"), None);
        // Every emoji has a name that finds it again.
        for e in &table().entries {
            assert_eq!(by_name(&e.name), Some(e.emoji), "{}", e.name);
        }
    }

    #[test]
    fn the_picker_offers_only_emoji_with_pictures() {
        let all: usize = groups(false).iter().map(|(_, l)| l.len()).sum();
        assert!(all > 1500, "{all}");
        assert!(
            groups(false)
                .iter()
                .flat_map(|(_, l)| l)
                .all(|e| picture(e).is_some())
        );
        assert!(groups(true).iter().map(|(_, l)| l.len()).sum::<usize>() >= all);
        assert_eq!(
            groups(false).first().and_then(|(_, l)| l.first()),
            Some(&"😀")
        );
    }

    #[test]
    fn search_finds_names_and_words() {
        let hits = search(":fire:", false);
        assert_eq!(hits.first(), Some(&"🔥"));
        assert!(search("tears of joy", false).contains(&"😂"));
        assert!(search("zzzzzz", false).is_empty());
        assert!(search("  ", false).is_empty());
    }

    #[test]
    fn suggestions_put_the_most_used_first_then_the_shortest() {
        let mut freq = HashMap::new();
        let plain = suggest("sm", &freq, 6, false);
        assert_eq!(plain.len(), 6);
        assert!(
            plain.windows(2).all(|w| w[0].1.len() <= w[1].1.len()),
            "{plain:?}"
        );
        bump(&mut freq, "😼");
        bump(&mut freq, "😼");
        let used = suggest("sm", &freq, 6, false);
        assert_eq!(used[0].0, "😼");
        assert!(suggest("", &freq, 6, false).is_empty());
        // A word of the Unicode name counts too.
        assert!(
            suggest("joy", &freq, 6, false)
                .iter()
                .any(|(e, _)| *e == "😂")
        );
    }

    #[test]
    fn quick_reactions_are_the_most_used_that_are_allowed() {
        let mut freq = HashMap::new();
        assert_eq!(quick(&freq, None, 3), vec!["👍", "❤️", "😂"]);
        for _ in 0..3 {
            bump(&mut freq, "🔥");
        }
        bump(&mut freq, "😂");
        assert_eq!(quick(&freq, None, 3), vec!["🔥", "😂", "👍"]);
        let allowed: Vec<String> = ["👍", "👎", "😂"].map(String::from).to_vec();
        assert_eq!(quick(&freq, Some(&allowed), 3), vec!["😂", "👍", "👎"]);
    }

    #[test]
    fn bump_counts_one_key_per_emoji() {
        let mut freq = HashMap::new();
        bump(&mut freq, "❤");
        bump(&mut freq, "❤️");
        bump(&mut freq, "🎉");
        assert_eq!(freq.get("❤️"), Some(&2));
        assert_eq!(top(&freq, 1), vec!["❤️".to_owned()]);
        assert_eq!(top(&freq, 5).len(), 2);
    }

    #[test]
    fn the_picker_hangs_above_its_button_right_edges_aligned() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1440.0, 900.0));
        let button = Rect::from_min_size(pos2(1030.0, 845.0), vec2(32.0, 32.0));
        let (r, up) = place(screen, button, vec2(344.0, 408.0), false);
        assert!(up);
        assert_eq!(r.right(), button.right() + 4.0);
        assert_eq!(r.bottom(), button.top() - 8.0);
        let (r, _) = place(screen, button, vec2(344.0, 408.0), true);
        assert_eq!(r.left(), button.left() - 4.0);
        // Near the top: below the button.
        let high = Rect::from_min_size(pos2(600.0, 20.0), vec2(32.0, 32.0));
        let (r, up) = place(screen, high, vec2(344.0, 408.0), false);
        assert!(!up);
        assert_eq!(r.top(), high.bottom() + 8.0);
    }

    #[test]
    fn insertion_counts_characters() {
        let ctx = Context::default();
        let id = Id::unique("t");
        let mut buf = "héllo wörld".to_owned();
        let mut st = egui::text_edit::TextEditState::default();
        st.cursor
            .set_char_range(Some(CCursorRange::two(CCursor::new(6), CCursor::new(11))));
        st.store(&ctx, id);
        insert_at_cursor(&ctx, id, &mut buf, "🎉");
        assert_eq!(buf, "héllo 🎉");
        insert_at_cursor(&ctx, id, &mut buf, "!");
        assert_eq!(buf, "héllo 🎉!");
    }
}

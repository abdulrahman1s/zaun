//! GIFs (chat.html section 8 GIF PICKER and the GIF message; script section 24 `gifPicker`,
//! `sendGif`). GIFs come from Giphy, through Signal's content proxy (`easyplay_core::gifs`):
//! the sender downloads one and sends it as a file, so friends never contact Giphy or the
//! proxy. Owner: the services agent.
//!
//! - [`picker`]: drawn when `state.chat.picker == Some(Picker::Gif)`, hanging from the GIF
//!   button's rect ([`emoji::anchor`]), in the emoji picker's shell at 372 × 428: the search
//!   field ([`emoji::search_field`]) · "Saved", then "Trending" (or the results) in two
//!   columns · the footer "Only your search words go to the GIF service." beside "Powered by
//!   GIPHY" (Giphy's terms require it). Opening asks for trending (the service keeps it for
//!   ten minutes); typing searches after a pause, or at once with Enter. A click sends
//!   (`ChatCommand::SendGif`) and closes. With "GIF search" off in Settings, without a key,
//!   or with a relay that is not an address, only saved GIFs show, with a one-line reason.
//! - [`paint_message`]: a GIF message's picture (the conversation draws the tag and the
//!   star around it). A file is decoded off the UI thread by `easyplay_core::gifs::decode`
//!   (size and frame limits) and animates while "Play GIFs automatically" is on; otherwise
//!   it stays still until clicked. A name that is not an existing absolute path (a stand-in
//!   in the preview, a received file not fetched) draws chat.html's stand-in: a paper colour
//!   with the GIF's words in the display face.
//! - Pictures (Giphy's previews, GIF files) are decoded by one worker thread and uploaded
//!   as egui textures here, never through egui's loaders.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use easyplay_core::gifs::{self, DecodeLimits, Frame, Gif, GifPick, GifProblem};
use easyplay_core::service::ChatCommand;
use egui::{
    Align2, ColorImage, Context, CornerRadius, Id, Key, Order, Painter, Rect, Response, Sense,
    Stroke, StrokeKind, TextureHandle, TextureOptions, Ui, UiBuilder, Vec2, pos2, vec2,
};

use super::{Env, emoji};
use crate::app::UiState;
use crate::bridge::UiCommand;
use crate::i18n::{self, Dir};
use crate::strings::chat::services as s;
use crate::theme::motion::{appear, ease_out, scale_about};
use crate::theme::tokens::stroke;
use crate::theme::{self, Dur, Type, palette};
use crate::vm::chat::{GifPicture, Picker};
use crate::vm::toasts::ToastKind;

/// Lengths (chat.html CSS `.gp`, `.pop*`, `.gp-g`, `.gif`).
pub mod m {
    /// The picker's size.
    pub const W: f32 = 372.0;
    /// The picker's height.
    pub const H: f32 = 428.0;
    /// Two columns.
    pub const COLS: usize = 2;
    /// Between cells.
    pub const GAP: f32 = 6.0;
    /// The grid's side and bottom padding.
    pub const GRID_PAD: f32 = 8.0;
    /// A cell's width over its height (16:10).
    pub const RATIO: f32 = 1.6;
    /// A cell's corner.
    pub const CELL_RADIUS: u8 = 10;
    /// A pointed-at cell shrinks to this.
    pub const PRESS: f32 = 0.975;
    /// Seconds after the last key before a search goes out.
    pub const DEBOUNCE: f64 = 0.45;
    /// A GIF message's widest.
    pub const MESSAGE_W: f32 = 280.0;
    /// The stand-in's words: this share of the width (`21cqw`).
    pub const WORD: f32 = 0.21;
    /// The stand-in's words are set in by this share of the width on each side (`6cqw`).
    pub const WORD_IN: f32 = 0.06;
    /// The "Powered by GIPHY" mark's gap to the footer line.
    pub const MARK_GAP: f32 = 10.0;
    // The emoji picker's shell (`.pop`, `.pop-top`, `.pop-h`, `.pop-f`, `.pop-none`).
    /// The shell's corner.
    pub const RADIUS: u8 = 14;
    /// Above, beside and below the search field.
    pub const TOP_PAD: [f32; 3] = [10.0, 10.0, 8.0];
    /// The search field's height.
    pub const SEARCH_H: f32 = 30.0;
    /// A section heading's height, its text's top and inset.
    pub const HEAD_H: f32 = 29.0;
    /// The heading text's top.
    pub const HEAD_TOP: f32 = 10.0;
    /// The heading text's inset.
    pub const HEAD_X: f32 = 4.0;
    /// The footer's height.
    pub const FOOT_H: f32 = 44.0;
    /// The footer's padding.
    pub const FOOT_PAD: f32 = 12.0;
    /// Above an empty or problem line.
    pub const NONE_PAD: f32 = 28.0;
    /// Where the picker grows from: scale .97 → 1, 6 below its place.
    pub const FROM_SCALE: f32 = 0.97;
    /// The drop it rises from.
    pub const FROM_DROP: f32 = 6.0;
}

// ---- Pictures --------------------------------------------------------------------------

/// Pictures kept decoded at most (the least recently drawn go first).
const KEEP: usize = 64;
/// Decoded pixels kept at most over all pictures (the least recently drawn go first).
const KEEP_BYTES: usize = 192 * 1024 * 1024;
/// The largest GIF file read from disk.
const MAX_FILE: u64 = 16 * 1024 * 1024;

/// A decoded picture: its frames as textures.
struct Anim {
    size: [usize; 2],
    frames: Vec<(TextureHandle, u32)>,
    total_ms: u64,
    /// Decoded bytes (RGBA of every frame).
    bytes: usize,
}

enum Slot {
    Loading,
    Ready(Arc<Anim>),
    Failed,
}

struct Entry {
    slot: Slot,
    used: u64,
}

enum Source {
    Bytes(Arc<[u8]>),
    File(PathBuf),
}

type Done = (String, Result<Vec<Frame>, String>);

/// The decoder: one worker thread, and the pictures decoded so far.
struct Decoder {
    slots: HashMap<String, Entry>,
    jobs: Option<mpsc::Sender<(String, Source)>>,
    done_tx: mpsc::Sender<Done>,
    done: mpsc::Receiver<Done>,
}

type Shared = Arc<Mutex<Decoder>>;

fn decoder(ctx: &Context) -> Shared {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(Id::unique("chat.gif.decoder"), || {
            let (done_tx, done) = mpsc::channel();
            Arc::new(Mutex::new(Decoder {
                slots: HashMap::new(),
                jobs: None,
                done_tx,
                done,
            }))
        })
        .clone()
    })
}

/// Read a GIF file on the worker thread (never the UI thread), at most [`MAX_FILE`].
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    use easyplay_core::fsx::Fs;
    easyplay_core::fsx::RealFs::new()
        .read(path, MAX_FILE)
        .map_err(|e| e.to_string())
}

fn worker(ctx: Context, jobs: mpsc::Receiver<(String, Source)>, done: mpsc::Sender<Done>) {
    for (key, src) in jobs {
        let bytes = match src {
            Source::Bytes(b) => Ok(b.to_vec()),
            Source::File(p) => read_file(&p),
        };
        let out = bytes
            .and_then(|b| gifs::decode(&b, DecodeLimits::default()).map_err(|e| e.to_string()));
        if done.send((key, out)).is_err() {
            return;
        }
        ctx.request_repaint();
    }
}

impl Decoder {
    fn ask(&mut self, ctx: &Context, key: &str, src: Source) {
        if self.jobs.is_none() {
            let (tx, rx) = mpsc::channel();
            let done = self.done_tx.clone();
            let c = ctx.clone();
            let spawned = std::thread::Builder::new()
                .name("chat-gif-decode".into())
                .spawn(move || worker(c, rx, done));
            if spawned.is_err() {
                self.slots.insert(
                    key.to_owned(),
                    Entry {
                        slot: Slot::Failed,
                        used: 0,
                    },
                );
                return;
            }
            self.jobs = Some(tx);
        }
        let sent = self
            .jobs
            .as_ref()
            .is_some_and(|j| j.send((key.to_owned(), src)).is_ok());
        self.slots.insert(
            key.to_owned(),
            Entry {
                slot: if sent { Slot::Loading } else { Slot::Failed },
                used: ctx.cumulative_pass_nr(),
            },
        );
    }

    fn collect(&mut self, ctx: &Context) {
        while let Ok((key, out)) = self.done.try_recv() {
            let slot = match out {
                Ok(frames) => {
                    let size = frames.first().map_or([1, 1], |f| f.size);
                    let total_ms = frames.iter().map(|f| u64::from(f.delay_ms)).sum();
                    let bytes = frames.iter().map(|f| f.rgba.len()).sum();
                    let frames = frames
                        .into_iter()
                        .enumerate()
                        .map(|(i, f)| {
                            let img = ColorImage::from_rgba_unmultiplied(f.size, &f.rgba);
                            (
                                ctx.load_texture(
                                    format!("chat-gif-{key}-{i}"),
                                    img,
                                    TextureOptions::LINEAR,
                                ),
                                f.delay_ms,
                            )
                        })
                        .collect();
                    Slot::Ready(Arc::new(Anim {
                        size,
                        frames,
                        total_ms,
                        bytes,
                    }))
                }
                Err(e) => {
                    tracing::debug!(error = %e, "gif_undecodable");
                    Slot::Failed
                }
            };
            let used = self.slots.get(&key).map_or(0, |e| e.used);
            self.slots.insert(key, Entry { slot, used });
        }
        let size = |e: &Entry| match &e.slot {
            Slot::Ready(a) => a.bytes,
            _ => 0,
        };
        let mut bytes: usize = self.slots.values().map(size).sum();
        if self.slots.len() > KEEP || bytes > KEEP_BYTES {
            let mut by_age: Vec<(u64, String)> = self
                .slots
                .iter()
                .filter(|(_, e)| !matches!(e.slot, Slot::Loading))
                .map(|(k, e)| (e.used, k.clone()))
                .collect();
            by_age.sort();
            let now = ctx.cumulative_pass_nr();
            for (used, k) in by_age {
                if self.slots.len() <= KEEP && bytes <= KEEP_BYTES {
                    break;
                }
                // Pictures drawn in this pass stay.
                if used + 1 >= now {
                    break;
                }
                if let Some(e) = self.slots.remove(&k) {
                    bytes -= size(&e);
                }
            }
        }
    }
}

/// The picture under `key`; asks for it (from `source`) the first time. `None` while it
/// decodes or when it cannot be read.
fn picture(ctx: &Context, key: &str, source: impl FnOnce() -> Option<Source>) -> Option<Arc<Anim>> {
    let shared = decoder(ctx);
    let mut d = shared.lock().unwrap_or_else(PoisonError::into_inner);
    d.collect(ctx);
    let pass = ctx.cumulative_pass_nr();
    match d.slots.get_mut(key) {
        Some(e) => {
            e.used = pass;
            match &e.slot {
                Slot::Ready(a) => Some(Arc::clone(a)),
                Slot::Loading | Slot::Failed => None,
            }
        }
        None => {
            if let Some(src) = source() {
                d.ask(ctx, key, src);
            }
            None
        }
    }
}

/// Draw `anim` over `rect`, cropped to fill it (`object-fit: cover`). `play`: the frame
/// for the current time, and a repaint when the next one is due; otherwise the first.
fn paint_anim(ui: &Ui, rect: Rect, anim: &Anim, play: bool, corner: CornerRadius) {
    let mut index = 0;
    if play && anim.frames.len() > 1 && anim.total_ms > 0 {
        let now = (ui.input(|i| i.time) * 1000.0) as u64 % anim.total_ms;
        let mut acc = 0u64;
        for (i, (_, d)) in anim.frames.iter().enumerate() {
            acc += u64::from(*d);
            if now < acc {
                index = i;
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(acc - now));
                break;
            }
        }
    }
    let Some((tex, _)) = anim.frames.get(index) else {
        return;
    };
    let [w, h] = anim.size;
    let (iw, ih) = (w.max(1) as f32, h.max(1) as f32);
    let (ia, ra) = (iw / ih, rect.width() / rect.height().max(1.0));
    let uv = if ia > ra {
        let f = ra / ia;
        Rect::from_min_max(pos2((1.0 - f) / 2.0, 0.0), pos2((1.0 + f) / 2.0, 1.0))
    } else {
        let f = ia / ra;
        Rect::from_min_max(pos2(0.0, (1.0 - f) / 2.0), pos2(1.0, (1.0 + f) / 2.0))
    };
    egui::Image::from_texture((tex.id(), vec2(iw, ih)))
        .uv(uv)
        .corner_radius(corner)
        .paint_at(ui, rect);
}

/// The stand-in's paper: chat.html's `PAPER[hash('gif' + k) % PAPER.length]`.
pub fn stand_in_paper(name: &str) -> palette::Paper {
    palette::paper(palette::fnv1a(&format!("gif{name}")) as usize % palette::PAPER_COUNT)
}

/// The stand-in: the paper colour with `words` in the display face (21 % of the width,
/// line height .9, centred), clipped to `rect`.
pub fn paint_stand_in(ui: &Ui, painter: &Painter, rect: Rect, name: &str, words: &str) {
    let palette::Paper { paper, ink } = stand_in_paper(name);
    let corner = CornerRadius::same(m::CELL_RADIUS);
    painter.rect_filled(rect, corner, paper);
    if words.is_empty() {
        return;
    }
    let w = rect.width();
    let size = (w * m::WORD).round().max(1.0);
    let mut job = egui::text::LayoutJob::single_section(
        words.to_owned(),
        egui::TextFormat {
            font_id: theme::typography::display_font(size),
            line_height: Some(size * 0.9),
            color: ink,
            ..Default::default()
        },
    );
    job.wrap.max_width = w * (1.0 - m::WORD_IN * 2.0);
    job.halign = egui::Align::Center;
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let clip = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
    clip.galley(
        pos2(rect.center().x, rect.center().y - g.size().y / 2.0),
        g,
        ink,
    );
}

/// The words a GIF without a picture shows: a stand-in's, else the file's name.
fn words_of(name: &str) -> String {
    if let Some(w) = gifs::stand_in_words(name) {
        return w.to_owned();
    }
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    stem.trim_end_matches(|c: char| c.is_ascii_digit() || c == '-')
        .replace(['-', '_'], " ")
        .to_uppercase()
}

/// The size a GIF message takes at most `max_w` wide: its picture's aspect once decoded
/// (between 1:2 and 3:1), 16:10 before and for stand-ins. At most [`m::MESSAGE_W`] wide.
pub fn message_size(ctx: &Context, name: &str, max_w: f32) -> Vec2 {
    let w = max_w.min(m::MESSAGE_W);
    let aspect = if Path::new(name).is_absolute() {
        let shared = decoder(ctx);
        let d = shared.lock().unwrap_or_else(PoisonError::into_inner);
        match d.slots.get(&file_key(name)).map(|e| &e.slot) {
            Some(Slot::Ready(a)) => {
                (a.size[0].max(1) as f32 / a.size[1].max(1) as f32).clamp(0.5, 3.0)
            }
            _ => m::RATIO,
        }
    } else {
        m::RATIO
    };
    vec2(w, (w / aspect).round())
}

fn file_key(path: &str) -> String {
    format!("f:{path}")
}

/// A GIF message's picture in `rect` (radius 10): the file `name` (an absolute path),
/// decoded and animated while `autoplay` is on, still until clicked otherwise; or the
/// preview's stand-in. Returns the picture's response (a click plays or stops it when
/// `autoplay` is off). The conversation draws the "GIF" tag and the star over it.
pub fn paint_message(ui: &mut Ui, name: &str, rect: Rect, autoplay: bool) -> Response {
    let id = Id::unique(("chat.gif.message", name));
    let resp = ui.interact(
        rect,
        ui.scope_id().with(("chat.gif.message", name)),
        if autoplay {
            Sense::hover()
        } else {
            Sense::click()
        },
    );
    let mut playing = autoplay || ui.ctx().data(|d| d.get_temp::<bool>(id).unwrap_or(false));
    if !autoplay && resp.clicked() {
        playing = !playing;
        ui.ctx().data_mut(|d| d.insert_temp(id, playing));
    }
    let corner = CornerRadius::same(m::CELL_RADIUS);
    let painter = ui.painter().clone();
    let anim = Path::new(name)
        .is_absolute()
        .then(|| {
            picture(ui.ctx(), &file_key(name), || {
                Some(Source::File(PathBuf::from(name)))
            })
        })
        .flatten();
    match anim {
        Some(a) => paint_anim(ui, rect, &a, playing, corner),
        None => paint_stand_in(ui, &painter, rect, name, &words_of(name)),
    }
    if autoplay {
        resp
    } else {
        crate::widgets::tooltip::attach(resp, s::PLAY())
    }
}

// ---- The picker ------------------------------------------------------------------------

/// The picker's own state between passes.
#[derive(Clone, Default)]
struct PickerUi {
    /// The last pass the picker was shown.
    pass: u64,
    /// Passes left in which the search field asks for the keyboard (the button that
    /// opened the picker may take it back in the pass it was clicked).
    focus: u8,
}

/// One cell of the grid.
#[derive(Clone)]
enum Cell {
    /// A saved GIF (a file, or a stand-in's name in the preview).
    Saved(String),
    /// One of Giphy's.
    Found(Gif),
}

struct Section {
    label: String,
    cells: Vec<Cell>,
    /// A line instead of (or after) the cells.
    note: Option<String>,
    y: f32,
}

fn cell_size() -> Vec2 {
    let w = (m::W - 2.0 * m::GRID_PAD - m::GAP * (m::COLS as f32 - 1.0)) / m::COLS as f32;
    vec2(w, w / m::RATIO)
}

impl Section {
    fn height(&self) -> f32 {
        let rows = self.cells.len().div_ceil(m::COLS) as f32;
        let grid = if rows > 0.0 {
            rows * cell_size().y + (rows - 1.0) * m::GAP
        } else {
            0.0
        };
        let note = if self.note.is_some() {
            2.0 * m::NONE_PAD + Type::Body.line_height() * 2.0
        } else {
            0.0
        };
        let head = if self.label.is_empty() {
            0.0
        } else {
            m::HEAD_H
        };
        head + grid + note
    }
}

fn problem_text(p: GifProblem) -> Option<String> {
    match p {
        GifProblem::Offline => Some(s::OFFLINE().to_owned()),
        GifProblem::BadKey => Some(s::BAD_KEY().to_owned()),
        GifProblem::Refused | GifProblem::BadAnswer => Some(s::REFUSED().to_owned()),
        GifProblem::NoKey | GifProblem::Off | GifProblem::BadRelay => None,
    }
}

/// The GIF picker, when `state.chat.picker` is `Picker::Gif`. See the module's header.
pub fn picker(ctx: &Context, state: &mut UiState, env: &mut Env<'_>) {
    if state.chat.picker != Some(Picker::Gif) {
        return;
    }
    let Some(chat) = state.chat.open.as_ref().map(|c| c.chat) else {
        state.chat.picker = None;
        return;
    };
    let id = Id::unique("chat.gif.picker");
    let search_id = id.with("search");
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let mut ps: PickerUi = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let fresh = ps.pass == 0 || ps.pass + 1 < pass;
    ps.pass = pass;

    let search_on = env.bridge.prefs().chat.gif_search;
    let g = &mut state.chat.gif;
    let no_key = g
        .answer
        .as_ref()
        .is_some_and(|a| a.problem == Some(GifProblem::NoKey));
    let no_relay = g
        .answer
        .as_ref()
        .is_some_and(|a| a.problem == Some(GifProblem::BadRelay));
    let can_search = search_on && !no_key && !no_relay;
    if fresh {
        g.query.clear();
        if search_on {
            // Giphy's trending list (kept by the service for ten minutes).
            let q = env.bridge.query_id();
            g.asked = Some(String::new());
            g.pending = Some(q);
            env.bridge
                .send(UiCommand::Chat(ChatCommand::GifTrending { reply_to: q }));
            ps.focus = 3;
        }
    }
    if ps.focus > 0 {
        ps.focus -= 1;
        if !ctx.memory(|mem| mem.has_focus(search_id)) {
            ctx.memory_mut(|mem| mem.request_focus(search_id));
        }
    }

    let screen = ctx.content_rect();
    let anchor = emoji::anchor(ctx).unwrap_or(Rect::from_center_size(screen.center(), Vec2::ZERO));
    let rtl = i18n::dir() == Dir::Rtl;
    let (rect, up) = emoji::place(screen, anchor, vec2(m::W, m::H), rtl);

    // Keys: Esc closes; Enter searches now, or sends the first result once it is shown.
    let mut close = false;
    let mut enter = false;
    ctx.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
            close = true;
        }
    });
    if ctx.memory(|mem| mem.has_focus(search_id))
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter))
    {
        enter = true;
    }

    // The sections.
    let g = &state.chat.gif;
    let q = gifs::clean_query(&g.query);
    let answered = g
        .answer
        .as_ref()
        .filter(|a| a.query == q && g.pending.is_none());
    let mut sections: Vec<Section> = Vec::new();
    if can_search && !q.is_empty() {
        match answered {
            None => sections.push(Section {
                label: String::new(),
                cells: Vec::new(),
                note: Some(s::SEARCHING().to_owned()),
                y: 0.0,
            }),
            Some(a) if a.gifs.is_empty() => sections.push(Section {
                label: String::new(),
                cells: Vec::new(),
                note: Some(
                    a.problem
                        .and_then(problem_text)
                        .unwrap_or_else(|| s::none_for(&q)),
                ),
                y: 0.0,
            }),
            Some(a) => sections.push(Section {
                label: s::RESULTS().to_owned(),
                cells: a.gifs.iter().cloned().map(Cell::Found).collect(),
                note: None,
                y: 0.0,
            }),
        }
    } else {
        let saved = &state.chat.saved_gifs;
        if !saved.is_empty() {
            sections.push(Section {
                label: s::SAVED().to_owned(),
                cells: saved.iter().cloned().map(Cell::Saved).collect(),
                note: None,
                y: 0.0,
            });
        } else if !can_search {
            sections.push(Section {
                label: String::new(),
                cells: Vec::new(),
                note: Some(s::NO_SAVED().to_owned()),
                y: 0.0,
            });
        }
        if can_search && let Some(a) = answered {
            // Giphy's list is shown as given; only the preview's stand-ins
            // leave out the saved ones, as chat.html does.
            let saved = &state.chat.saved_gifs;
            let fresh_gifs: Vec<Cell> = a
                .gifs
                .iter()
                .filter(|g| !(g.is_stand_in() && saved.contains(&g.id)))
                .cloned()
                .map(Cell::Found)
                .collect();
            let note = a.problem.and_then(problem_text);
            if !fresh_gifs.is_empty() || note.is_some() {
                sections.push(Section {
                    label: s::TRENDING().to_owned(),
                    cells: fresh_gifs,
                    note,
                    y: 0.0,
                });
            }
        }
    }
    let mut y = 0.0;
    for sec in &mut sections {
        sec.y = y;
        y += sec.height();
    }
    let content_h = y + m::GRID_PAD;
    let first: Option<Cell> = sections.iter().find_map(|x| x.cells.first().cloned());

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

    let mut picked: Option<Cell> = None;
    let mut want_previews: Vec<String> = Vec::new();
    let previews = &state.chat.gif.previews;
    let mut query = state.chat.gif.query.clone();
    let area = egui::Area::new(id)
        .order(Order::Foreground)
        .fixed_pos(rect.min)
        .constrain(false)
        .show(ctx, |ui| {
            ui.with_visual_transform(transform, |ui| {
                ui.multiply_opacity(alpha);
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                ui.interact(rect, id.with("plate"), Sense::click());
                crate::a11y::labelled(ui.ctx(), id, egui::Role::Dialog, s::SEARCH_NAME());
                let painter = ui.painter().clone();
                let radius = CornerRadius::same(m::RADIUS);
                painter.add(p.shade_pop.shadow().as_shape(rect, radius));
                painter.rect_filled(rect, radius, p.raised);

                // Search (the shared picker field).
                let pad = m::TOP_PAD;
                let sr = Rect::from_min_max(
                    rect.min + vec2(pad[1], pad[0]),
                    pos2(rect.right() - pad[1], rect.top() + pad[0] + m::SEARCH_H),
                );
                let hint = if search_on {
                    s::SEARCH_NAME()
                } else {
                    s::SEARCH_OFF()
                };
                emoji::search_field(ui, sr, search_id, &mut query, hint, can_search);
                let body_top = sr.bottom() + pad[2];

                // The grid.
                let foot_top = rect.bottom() - m::FOOT_H;
                let body =
                    Rect::from_min_max(pos2(rect.left(), body_top), pos2(rect.right(), foot_top));
                let mut child = ui.new_child(
                    UiBuilder::new()
                        .max_rect(body)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_clip_rect(body.intersect(ui.clip_rect()));
                let cs = cell_size();
                egui::ScrollArea::vertical()
                    .id_salt(id.with(("scroll", q.clone())))
                    .auto_shrink([false, false])
                    .show_viewport(&mut child, |ui, vp| {
                        ui.set_height(content_h);
                        let origin = ui.min_rect().min;
                        let cell_x = |col: usize| {
                            let k = col as f32;
                            if rtl {
                                origin.x + m::W - m::GRID_PAD - (k + 1.0) * cs.x - k * m::GAP
                            } else {
                                origin.x + m::GRID_PAD + k * (cs.x + m::GAP)
                            }
                        };
                        for (si, sec) in sections.iter().enumerate() {
                            if sec.y > vp.bottom() || sec.y + sec.height() < vp.top() {
                                continue;
                            }
                            let mut top = sec.y;
                            if !sec.label.is_empty() {
                                let (x, align) = if rtl {
                                    (origin.x + m::W - m::GRID_PAD - m::HEAD_X, Align2::RIGHT_TOP)
                                } else {
                                    (origin.x + m::GRID_PAD + m::HEAD_X, Align2::LEFT_TOP)
                                };
                                ui.painter().text(
                                    pos2(x, origin.y + top + m::HEAD_TOP),
                                    align,
                                    &sec.label,
                                    Type::MicroStrong.font_id(),
                                    p.t3,
                                );
                                top += m::HEAD_H;
                            }
                            for (k, cell) in sec.cells.iter().enumerate() {
                                let cy = top + (k / m::COLS) as f32 * (cs.y + m::GAP);
                                if cy + cs.y < vp.top() || cy > vp.bottom() {
                                    continue;
                                }
                                let r = Rect::from_min_size(
                                    pos2(cell_x(k % m::COLS), origin.y + cy),
                                    cs,
                                );
                                let resp = ui.interact(r, id.with(("cell", si, k)), Sense::click());
                                let shown = if resp.hovered() || resp.has_focus() {
                                    Rect::from_center_size(r.center(), r.size() * m::PRESS)
                                } else {
                                    r
                                };
                                let title =
                                    draw_cell(ui, shown, cell, previews, &mut want_previews);
                                crate::a11y::button(&resp, &s::gif_named(&title));
                                if resp.has_focus() {
                                    ui.painter().rect_stroke(
                                        shown,
                                        CornerRadius::same(m::CELL_RADIUS),
                                        Stroke::new(stroke::FIELD_FOCUS, p.accent_mark),
                                        StrokeKind::Outside,
                                    );
                                }
                                if resp.clicked() {
                                    picked = Some(cell.clone());
                                }
                            }
                            if let Some(note) = &sec.note {
                                let ny = top
                                    + sec.cells.len().div_ceil(m::COLS) as f32 * (cs.y + m::GAP)
                                    + m::NONE_PAD;
                                let g =
                                    Type::Body.galley(ui, note, p.t3, m::W - 2.0 * m::FOOT_PAD, 3);
                                let at =
                                    pos2(origin.x + m::W / 2.0 - g.size().x / 2.0, origin.y + ny);
                                ui.painter().galley(at, g, p.t3);
                            }
                        }
                    });

                // Footer: what is sent, and "Powered by GIPHY" (Giphy's terms, 5A).
                painter.hline(
                    rect.x_range(),
                    foot_top + stroke::BORDER / 2.0,
                    Stroke::new(stroke::BORDER, p.line),
                );
                let cy = foot_top + m::FOOT_H / 2.0;
                let fp = m::FOOT_PAD;
                let line = if !search_on {
                    s::FOOTER_OFF()
                } else if no_key {
                    s::NO_KEY()
                } else if no_relay {
                    s::BAD_RELAY()
                } else {
                    s::FOOTER()
                };
                let mut text_w = m::W - 2.0 * fp;
                if can_search {
                    let mark =
                        Type::MicroStrong.galley(ui, s::POWERED_BY(), p.t2, f32::INFINITY, 1);
                    let mx = if rtl {
                        rect.left() + fp
                    } else {
                        rect.right() - fp - mark.size().x
                    };
                    let mr = Rect::from_min_size(pos2(mx, cy - mark.size().y / 2.0), mark.size());
                    text_w -= mark.size().x + m::MARK_GAP;
                    painter.galley(mr.min, mark, p.t2);
                    let resp = ui.interact(mr, id.with("attribution"), Sense::hover());
                    crate::a11y::item(&resp, egui::Role::Label, s::POWERED_BY());
                }
                let fg = Type::Caption.galley(ui, line, p.t3, text_w, 2);
                let x = if rtl {
                    rect.right() - fp - fg.size().x
                } else {
                    rect.left() + fp
                };
                let at = pos2(x, cy - fg.size().y / 2.0);
                let fr = Rect::from_min_size(at, fg.size());
                painter.galley(at, fg, p.t3);
                let resp = ui.interact(fr, id.with("footer"), Sense::hover());
                crate::a11y::item(&resp, egui::Role::Label, line);
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(stroke::BORDER, p.line),
                    StrokeKind::Inside,
                );
            });
        });

    // The search: after a pause, or at once with Enter (which sends the first result when
    // the results for these words are already shown).
    let g = &mut state.chat.gif;
    if query != g.query {
        g.query = query;
        g.typed_at = now;
    }
    let q = gifs::clean_query(&g.query);
    if can_search && g.asked.as_deref() != Some(q.as_str()) {
        let due = now - g.typed_at >= m::DEBOUNCE;
        if enter || due {
            let reply_to = env.bridge.query_id();
            g.asked = Some(q.clone());
            g.pending = Some(reply_to);
            env.bridge.send(UiCommand::Chat(if q.is_empty() {
                ChatCommand::GifTrending { reply_to }
            } else {
                ChatCommand::GifSearch { query: q, reply_to }
            }));
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                m::DEBOUNCE - (now - g.typed_at),
            ));
        }
    } else if enter && picked.is_none() {
        picked = first;
    }
    for url in want_previews {
        if g.previews.contains_key(&url) {
            continue;
        }
        let reply_to = env.bridge.query_id();
        g.previews.insert(url.clone(), GifPicture::Asked);
        g.preview_q.insert(reply_to, url.clone());
        env.bridge
            .send(UiCommand::Chat(ChatCommand::GifPreview { url, reply_to }));
    }

    let outside = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|at| !area.response.rect.contains(at) && !anchor.contains(at))
    });
    if let Some(cell) = picked {
        let gif = match cell {
            Cell::Saved(name) => Some(GifPick::Saved(PathBuf::from(name))),
            Cell::Found(g) if g.sendable() || g.is_stand_in() => Some(GifPick::Found(g)),
            Cell::Found(_) => None,
        };
        match gif {
            Some(gif) => {
                env.bridge
                    .send(UiCommand::Chat(ChatCommand::SendGif { chat, gif }));
                close = true;
            }
            None => state.toast(s::TOO_LARGE(), ToastKind::Warning, None, env.mono),
        }
    }
    if close || outside {
        state.chat.picker = None;
        if !outside {
            state.chat.focus_composer = true;
        }
        state.chat.gif.query.clear();
        ps = PickerUi::default();
    }
    ctx.data_mut(|d| d.insert_temp(id, ps));
}

/// Draw one cell; returns the GIF's title (for its accessible name). Giphy pictures not
/// asked for yet are added to `want`.
fn draw_cell(
    ui: &Ui,
    r: Rect,
    cell: &Cell,
    previews: &HashMap<String, GifPicture>,
    want: &mut Vec<String>,
) -> String {
    let painter = ui.painter();
    let corner = CornerRadius::same(m::CELL_RADIUS);
    match cell {
        Cell::Saved(name) => {
            let anim = Path::new(name)
                .is_absolute()
                .then(|| {
                    picture(ui.ctx(), &file_key(name), || {
                        Some(Source::File(PathBuf::from(name)))
                    })
                })
                .flatten();
            let words = words_of(name);
            match anim {
                Some(a) => paint_anim(ui, r, &a, true, corner),
                None => paint_stand_in(ui, painter, r, name, &words),
            }
            words.to_lowercase()
        }
        Cell::Found(g) if g.is_stand_in() => {
            paint_stand_in(ui, painter, r, &g.id, &g.title);
            g.title.to_lowercase()
        }
        Cell::Found(g) => {
            let key = format!("p:{}", g.preview_url);
            let anim = picture(ui.ctx(), &key, || match previews.get(&g.preview_url) {
                Some(GifPicture::Ready(b)) => Some(Source::Bytes(Arc::clone(&b.0))),
                _ => None,
            });
            match anim {
                Some(a) => paint_anim(ui, r, &a, true, corner),
                None => {
                    if !previews.contains_key(&g.preview_url) {
                        want.push(g.preview_url.clone());
                    }
                    painter.rect_filled(r, corner, theme::theme(ui.ctx()).palette.o1);
                }
            }
            g.title.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_are_two_columns_of_16_by_10() {
        let c = cell_size();
        assert!((c.x - 175.0).abs() < 0.01, "{c:?}");
        assert!((c.x / c.y - 1.6).abs() < 0.01);
    }

    #[test]
    fn words_for_files_and_stand_ins() {
        assert_eq!(words_of("letsgo"), "LET'S GO");
        assert_eq!(words_of("/data/chat/files/hello-hi-662-3.gif"), "HELLO HI");
    }

    #[test]
    fn the_stand_in_paper_follows_the_prototype_hash() {
        // chat.html: PAPER[hash('gif' + k) % PAPER.length] with FNV-1a.
        let want = palette::fnv1a("gifgg") as usize % palette::PAPER_COUNT;
        assert_eq!(stand_in_paper("gg"), palette::paper(want));
    }
}

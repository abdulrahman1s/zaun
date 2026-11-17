//! Pictures, videos and files in a conversation (round 3): the decoder of still pictures
//! (previews that come inside a message, and received files), off the UI thread, kept as
//! textures. Owner: the conversation agent. The gif agent's decoder (`gif.rs`) is the
//! pattern: one worker thread, a slot per picture, least recently drawn dropped first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, mpsc};

use easyplay_core::model::{FileState, Media};
use egui::{
    Align2, Color32, ColorImage, Context, CornerRadius, Id, Pos2, Rect, Sense, Shape, Stroke,
    TextureHandle, TextureOptions, Ui, pos2, vec2,
};

use crate::strings::chat::conversation as s;
use crate::theme::{self, Icon, Type, icons};

/// Pictures kept decoded at most.
const KEEP: usize = 96;
/// Decoded pixels kept at most over all pictures.
const KEEP_BYTES: usize = 256 * 1024 * 1024;
/// The largest picture file read from disk.
const MAX_FILE: u64 = 64 * 1024 * 1024;
/// Pictures are scaled down to at most this many pixels on their longer side.
const MAX_SIDE: u32 = 2048;

/// A decoded picture.
#[derive(Clone)]
pub struct Picture {
    /// Its texture.
    pub texture: TextureHandle,
    /// Its size in pixels (after scaling down).
    pub size: [usize; 2],
}

enum Slot {
    Loading,
    Ready(Picture),
    Failed,
}

struct Entry {
    slot: Slot,
    used: u64,
    bytes: usize,
}

/// Where a picture comes from.
pub enum Source {
    /// Bytes in hand (a preview inside a message).
    Bytes(Arc<[u8]>),
    /// A file on disk (a received picture).
    File(PathBuf),
}

type Decoded = Result<([usize; 2], Vec<u8>), String>;
type Done = (String, Decoded);

struct Decoder {
    slots: HashMap<String, Entry>,
    jobs: Option<mpsc::Sender<(String, Source)>>,
    done_tx: mpsc::Sender<Done>,
    done: mpsc::Receiver<Done>,
}

type Shared = Arc<Mutex<Decoder>>;

fn decoder(ctx: &Context) -> Shared {
    ctx.data_mut(|d| {
        d.get_temp_mut_or_insert_with(Id::unique("chat.media.decoder"), || {
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

/// Read a picture file on the worker thread (never the UI thread).
fn read_file(path: &Path) -> Result<Vec<u8>, String> {
    use easyplay_core::fsx::Fs;
    easyplay_core::fsx::RealFs::new()
        .read(path, MAX_FILE)
        .map_err(|e| e.to_string())
}

/// Decode `bytes` (PNG, JPEG, WebP, BMP, GIF's first frame), scaled down to [`MAX_SIDE`].
pub(crate) fn decode(bytes: &[u8]) -> Decoded {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(512 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    let img = if img.width().max(img.height()) > MAX_SIDE {
        img.resize(MAX_SIDE, MAX_SIDE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok((size, rgba.into_raw()))
}

fn worker(ctx: Context, jobs: mpsc::Receiver<(String, Source)>, done: mpsc::Sender<Done>) {
    for (key, src) in jobs {
        let bytes = match src {
            Source::Bytes(b) => Ok(b.to_vec()),
            Source::File(p) => read_file(&p),
        };
        let out = bytes.and_then(|b| decode(&b));
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
                .name("chat-media-decode".into())
                .spawn(move || worker(c, rx, done));
            if spawned.is_ok() {
                self.jobs = Some(tx);
            }
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
                bytes: 0,
            },
        );
    }

    fn collect(&mut self, ctx: &Context) {
        while let Ok((key, out)) = self.done.try_recv() {
            let (slot, bytes) = match out {
                Ok((size, rgba)) => {
                    let bytes = rgba.len();
                    let img = ColorImage::from_rgba_unmultiplied(size, &rgba);
                    let texture =
                        ctx.load_texture(format!("chat-media-{key}"), img, TextureOptions::LINEAR);
                    (Slot::Ready(Picture { texture, size }), bytes)
                }
                Err(e) => {
                    tracing::debug!(error = %e, "chat_picture_undecodable");
                    (Slot::Failed, 0)
                }
            };
            let used = self.slots.get(&key).map_or(0, |e| e.used);
            self.slots.insert(key, Entry { slot, used, bytes });
        }
        let mut bytes: usize = self.slots.values().map(|e| e.bytes).sum();
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
                if used + 1 >= now {
                    break;
                }
                if let Some(e) = self.slots.remove(&k) {
                    bytes -= e.bytes;
                }
            }
        }
    }
}

/// What became of a picture asked for.
pub enum Got {
    /// Decoded.
    Ready(Picture),
    /// Still decoding.
    Loading,
    /// It cannot be read or decoded.
    Failed,
}

/// The picture under `key`; asks for it (from `source`) the first time.
pub fn picture(ctx: &Context, key: &str, source: impl FnOnce() -> Option<Source>) -> Got {
    let shared = decoder(ctx);
    let mut d = shared.lock().unwrap_or_else(PoisonError::into_inner);
    d.collect(ctx);
    let pass = ctx.cumulative_pass_nr();
    match d.slots.get_mut(key) {
        Some(e) => {
            e.used = pass;
            match &e.slot {
                Slot::Ready(p) => Got::Ready(p.clone()),
                Slot::Loading => Got::Loading,
                Slot::Failed => Got::Failed,
            }
        }
        None => match source() {
            Some(src) => {
                d.ask(ctx, key, src);
                Got::Loading
            }
            None => Got::Failed,
        },
    }
}

/// A preview that came inside a message: decoded at once (previews are small), cached.
fn preview(ctx: &Context, key: &str, bytes: &[u8]) -> Option<Picture> {
    let shared = decoder(ctx);
    let mut d = shared.lock().unwrap_or_else(PoisonError::into_inner);
    d.collect(ctx);
    let pass = ctx.cumulative_pass_nr();
    if let Some(e) = d.slots.get_mut(key) {
        e.used = pass;
        return match &e.slot {
            Slot::Ready(p) => Some(p.clone()),
            _ => None,
        };
    }
    if bytes.len() > MAX_INLINE_PREVIEW {
        d.ask(ctx, key, Source::Bytes(Arc::from(bytes)));
        return None;
    }
    let (slot, size) = match decode(bytes) {
        Ok((size, rgba)) => {
            let img = ColorImage::from_rgba_unmultiplied(size, &rgba);
            let texture =
                ctx.load_texture(format!("chat-media-{key}"), img, TextureOptions::LINEAR);
            (Slot::Ready(Picture { texture, size }), rgba.len())
        }
        Err(_) => (Slot::Failed, 0),
    };
    let got = match &slot {
        Slot::Ready(p) => Some(p.clone()),
        _ => None,
    };
    d.slots.insert(
        key.to_owned(),
        Entry {
            slot,
            used: pass,
            bytes: size,
        },
    );
    got
}

/// Previews up to this size decode on the spot; larger ones on the worker.
const MAX_INLINE_PREVIEW: usize = 256 * 1024;

// ---- Drawing --------------------------------------------------------------------------------

/// The measures (round 3 brief; `.gif` and `.sc` for the rest).
mod m {
    /// A picture: at most 360 wide and 320 high, radius 10.
    pub const MAX: [f32; 2] = [360.0, 320.0];
    pub const R: u8 = 10;
    /// Without a preview: a plate this size.
    pub const PLATE: [f32; 2] = [320.0, 180.0];
    /// 4 above and under.
    pub const MARGIN: f32 = 4.0;
    /// The round button in the middle, its glyph, and the progress ring.
    pub const BTN: f32 = 44.0;
    pub const BTN_ICON: f32 = 18.0;
    pub const RING_R: f32 = 19.0;
    pub const RING_W: f32 = 3.0;
    /// Black behind the button and the chips over a picture.
    pub const SCRIM: u8 = 150;
    /// An offered picture is dimmed this much.
    pub const DIM: u8 = 90;
    /// Chips over a picture (size, duration): 6 in, padded 6, 18 high, radius 4.
    pub const CHIP_IN: f32 = 6.0;
    pub const CHIP_PAD: f32 = 6.0;
    pub const CHIP_H: f32 = 18.0;
    pub const CHIP_R: u8 = 4;
    /// A file: a row on o1, radius 10, at most 360 wide, 56 high, padded 10; a 36 tile.
    pub const FILE_W: f32 = 360.0;
    pub const FILE_H: f32 = 56.0;
    pub const FILE_PAD: f32 = 10.0;
    pub const TILE: f32 = 36.0;
    pub const TILE_R: u8 = 8;
    pub const GAP: f32 = 12.0;
    /// The viewer: the picture 48 in from the window's edges.
    pub const VIEW_IN: f32 = 48.0;
    pub const VIEW_SCRIM: u8 = 220;
}

/// What a click on media asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Fetch the offered file.
    Receive,
    /// Stop fetching or sending it.
    Cancel,
    /// Open it with the system's app.
    Open,
    /// Show the picture over the whole window.
    View,
}

fn paint_icon(painter: &egui::Painter, at: Pos2, icon: Icon, size: f32, color: Color32) {
    painter.text(
        at,
        Align2::CENTER_CENTER,
        icon.text(false),
        icons::font(size),
        color,
    );
}

/// A picture over `rect`, cropped to fill it, with rounded corners.
fn paint_picture(ui: &Ui, rect: Rect, pic: &Picture, corner: CornerRadius, tint: Color32) {
    let [w, h] = pic.size;
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let want = rect.width() / rect.height().max(1.0);
    let have = w / h;
    let uv = if have > want {
        let k = want / have;
        Rect::from_min_max(pos2((1.0 - k) / 2.0, 0.0), pos2((1.0 + k) / 2.0, 1.0))
    } else {
        let k = have / want;
        Rect::from_min_max(pos2(0.0, (1.0 - k) / 2.0), pos2(1.0, (1.0 + k) / 2.0))
    };
    ui.painter().add(
        egui::epaint::RectShape::filled(rect, corner, tint).with_texture(pic.texture.id(), uv),
    );
}

/// The size a picture of `size` pixels takes: its aspect, as large as fits in 360 × 320
/// (and `max`). Previews are small thumbnails, so they are scaled up to it.
fn fit(size: [usize; 2], max: [f32; 2]) -> egui::Vec2 {
    let (w, h) = (size[0].max(1) as f32, size[1].max(1) as f32);
    let k = (max[0] / w).min(max[1] / h).max(f32::EPSILON);
    vec2((w * k).round().max(1.0), (h * k).round().max(1.0))
}

/// A progress ring: the track and `percent` of the circle, clockwise from the top.
fn ring(painter: &egui::Painter, c: Pos2, percent: u8) {
    painter.circle_stroke(
        c,
        m::RING_R,
        Stroke::new(m::RING_W, Color32::from_white_alpha(70)),
    );
    let n = 48;
    let end = f32::from(percent.min(100)) / 100.0;
    let pts: Vec<Pos2> = (0..=n)
        .map(|k| {
            let a =
                -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * end * k as f32 / n as f32;
            c + vec2(a.cos(), a.sin()) * m::RING_R
        })
        .collect();
    if end > 0.0 {
        painter.add(Shape::line(pts, Stroke::new(m::RING_W, Color32::WHITE)));
    }
}

/// A chip over a picture: white words on black 60%.
fn chip(ui: &Ui, at: Pos2, right: bool, words: &str) {
    let g = Type::MicroStrong.galley(ui, words, Color32::WHITE, f32::INFINITY, 1);
    let w = g.size().x + m::CHIP_PAD * 2.0;
    let x = if right { at.x - w } else { at.x };
    let r = Rect::from_min_size(pos2(x, at.y - m::CHIP_H), vec2(w, m::CHIP_H));
    ui.painter().rect_filled(
        r,
        CornerRadius::same(m::CHIP_R),
        Color32::from_black_alpha(m::SCRIM),
    );
    let gs = g.size();
    ui.painter().galley(
        pos2(r.left() + m::CHIP_PAD, r.center().y - gs.y / 2.0),
        g,
        Color32::WHITE,
    );
}

/// The round button in the middle: download, or a progress ring with ×, or play.
fn middle(
    ui: &mut Ui,
    id: Id,
    rect: Rect,
    state: &FileState,
    mine: bool,
    video: bool,
) -> Option<Act> {
    let c = rect.center();
    let btn = Rect::from_center_size(c, vec2(m::BTN, m::BTN));
    let (icon, act, label) = match state {
        FileState::Offered | FileState::Cancelled if !mine => {
            (Some(icons::DOWNLOAD), Act::Receive, s::DOWNLOAD())
        }
        FileState::Receiving { .. } | FileState::Sending { .. } => {
            (Some(icons::CLOSE), Act::Cancel, s::CANCEL())
        }
        _ if video => (Some(icons::PLAY), Act::Open, s::OPEN()),
        _ => return None,
    };
    let resp = ui.interact(btn, id.with("middle"), Sense::click());
    crate::a11y::button(&resp, label);
    let hover = resp.hovered();
    ui.painter().circle_filled(
        c,
        m::BTN / 2.0,
        Color32::from_black_alpha(if hover { 200 } else { m::SCRIM }),
    );
    if let FileState::Receiving { percent } | FileState::Sending { percent } = state {
        ring(ui.painter(), c, *percent);
    }
    if let Some(icon) = icon {
        let off = if icon == icons::PLAY {
            vec2(1.5, 0.0)
        } else {
            vec2(0.0, 0.0)
        };
        paint_icon(ui.painter(), c + off, icon, m::BTN_ICON, Color32::WHITE);
    }
    let resp = crate::widgets::tooltip::attach(resp, label);
    resp.clicked().then_some(act)
}

/// A picture: the preview at once, the full picture once received. Offered: dimmed, with
/// the download button and the size; receiving or sending: a progress ring.
pub fn image(ui: &mut Ui, key: &str, media: &Media, mine: bool) -> Option<Act> {
    let ctx = ui.ctx().clone();
    let pv = media
        .preview
        .as_deref()
        .and_then(|b| preview(&ctx, &format!("pv:{key}"), b));
    let full = match &media.state {
        FileState::Received(path) => {
            let p = path.clone();
            match picture(&ctx, &format!("f:{}", path.display()), || {
                Some(Source::File(p))
            }) {
                Got::Ready(pic) => Some(pic),
                Got::Loading | Got::Failed => None,
            }
        }
        _ => None,
    };
    let shown = full.or(pv);
    let max = [ui.available_width().min(m::MAX[0]), m::MAX[1]];
    let size = shown
        .as_ref()
        .map_or(vec2(m::PLATE[0].min(max[0]), m::PLATE[1]), |p| {
            fit(p.size, max)
        });
    ui.add_space(m::MARGIN);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let corner = CornerRadius::same(m::R);
    let p = theme::theme(ui.ctx()).palette.clone();
    match &shown {
        Some(pic) => paint_picture(ui, rect, pic, corner, Color32::WHITE),
        None => {
            ui.painter().rect_filled(rect, corner, p.o2);
            paint_icon(
                ui.painter(),
                rect.center() - vec2(0.0, 28.0),
                icons::IMAGE,
                22.0,
                p.t3,
            );
        }
    }
    let waiting = matches!(
        media.state,
        FileState::Offered | FileState::Cancelled | FileState::Receiving { .. } | FileState::Failed
    ) && !mine;
    if waiting {
        ui.painter()
            .rect_filled(rect, corner, Color32::from_black_alpha(m::DIM));
    }
    let id = Id::unique(("chat.media", key));
    let mut act = middle(ui, id, rect, &media.state, mine, false);
    if waiting || matches!(media.state, FileState::Sending { .. }) {
        chip(
            ui,
            pos2(rect.left() + m::CHIP_IN, rect.bottom() - m::CHIP_IN),
            false,
            &s::size(media.size),
        );
    }
    crate::a11y::item(
        &resp,
        egui::Role::Image,
        &s::photo_spoken(&s::size(media.size)),
    );
    if act.is_none() && resp.clicked() {
        act = match &media.state {
            FileState::Received(_) => Some(Act::View),
            FileState::Sent => Some(Act::View),
            FileState::Offered | FileState::Cancelled if !mine => Some(Act::Receive),
            _ => None,
        };
    }
    ui.add_space(m::MARGIN);
    act
}

/// A video: the preview (or a dark 16:9 plate) with a play glyph, the length and the size.
pub fn video(ui: &mut Ui, key: &str, media: &Media, mine: bool) -> Option<Act> {
    let ctx = ui.ctx().clone();
    let pv = media
        .preview
        .as_deref()
        .and_then(|b| preview(&ctx, &format!("pv:{key}"), b));
    let max = [ui.available_width().min(m::MAX[0]), m::MAX[1]];
    let size = pv.as_ref().map_or(
        vec2(
            m::PLATE[0].min(max[0]),
            (m::PLATE[0].min(max[0]) * 9.0 / 16.0).round(),
        ),
        |p| fit(p.size, max),
    );
    ui.add_space(m::MARGIN);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let corner = CornerRadius::same(m::R);
    match &pv {
        Some(pic) => paint_picture(ui, rect, pic, corner, Color32::WHITE),
        None => {
            ui.painter()
                .rect_filled(rect, corner, Color32::from_rgb(0x14, 0x15, 0x18));
        }
    }
    if matches!(media.state, FileState::Offered | FileState::Cancelled) && !mine {
        ui.painter()
            .rect_filled(rect, corner, Color32::from_black_alpha(m::DIM));
    }
    let id = Id::unique(("chat.media", key));
    let mut act = middle(ui, id, rect, &media.state, mine, true);
    let bottom = rect.bottom() - m::CHIP_IN;
    let len = media.duration.map(|d| format!("{}:{:02}", d / 60, d % 60));
    if let Some(l) = &len {
        chip(ui, pos2(rect.left() + m::CHIP_IN, bottom), false, l);
    }
    chip(
        ui,
        pos2(rect.right() - m::CHIP_IN, bottom),
        true,
        &s::size(media.size),
    );
    crate::a11y::item(
        &resp,
        egui::Role::Video,
        &s::video_spoken(len.as_deref().unwrap_or_default(), &s::size(media.size)),
    );
    if act.is_none() && resp.clicked() {
        act = match &media.state {
            FileState::Received(_) | FileState::Sent => Some(Act::Open),
            FileState::Offered | FileState::Cancelled if !mine => Some(Act::Receive),
            _ => None,
        };
    }
    ui.add_space(m::MARGIN);
    act
}

/// A file: a row on o1 — a file tile, the name over its size and state, then Download,
/// Cancel or Open.
pub fn file(ui: &mut Ui, media: &Media, mine: bool) -> Option<Act> {
    use crate::widgets::button::{Button, Size};
    let p = theme::theme(ui.ctx()).palette.clone();
    let w = ui.available_width().min(m::FILE_W);
    ui.add_space(m::MARGIN);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, m::FILE_H), Sense::hover());
    let rtl = crate::i18n::dir() == crate::i18n::Dir::Rtl;
    let flip = |r: Rect| {
        if rtl {
            Rect::from_min_size(
                pos2(rect.left() + rect.right() - r.right(), r.top()),
                r.size(),
            )
        } else {
            r
        }
    };
    let corner = CornerRadius::same(m::R);
    ui.painter().rect_filled(rect, corner, p.o1);
    if p.outlined() {
        ui.painter().rect_stroke(
            rect,
            corner,
            Stroke::new(1.0, p.line),
            egui::StrokeKind::Inside,
        );
    }
    let tile = flip(Rect::from_min_size(
        pos2(rect.left() + m::FILE_PAD, rect.center().y - m::TILE / 2.0),
        vec2(m::TILE, m::TILE),
    ));
    ui.painter()
        .rect_filled(tile, CornerRadius::same(m::TILE_R), p.o2);
    paint_icon(ui.painter(), tile.center(), icons::FILE, 18.0, p.t2);
    let (button, act): (Option<Button<'_>>, Option<Act>) = match &media.state {
        FileState::Offered | FileState::Cancelled if !mine => (
            Some(Button::new(s::DOWNLOAD()).size(Size::Sm)),
            Some(Act::Receive),
        ),
        FileState::Receiving { .. } | FileState::Sending { .. } => (
            Some(Button::new(s::CANCEL()).size(Size::Sm)),
            Some(Act::Cancel),
        ),
        FileState::Received(_) => (Some(Button::new(s::OPEN()).size(Size::Sm)), Some(Act::Open)),
        _ => (None, None),
    };
    let mut right = rect.right() - m::FILE_PAD;
    let mut out = None;
    if let (Some(b), Some(a)) = (button, act) {
        let bw = b.width(ui);
        let h = Size::Sm.height();
        let br = flip(Rect::from_min_size(
            pos2(right - bw, rect.center().y - h / 2.0),
            vec2(bw, h),
        ));
        if ui.put(br, b).clicked() {
            out = Some(a);
        }
        right -= bw + m::GAP;
    }
    let tx = rect.left() + m::FILE_PAD + m::TILE + m::GAP;
    let tw = (right - tx).max(1.0);
    let name = Type::BodyStrong.galley(ui, &media.name, p.t1, tw, 1);
    let (note, color) = match &media.state {
        FileState::Receiving { percent } => (s::receiving(*percent), p.t3),
        FileState::Sending { percent } => (s::sending(*percent), p.t3),
        FileState::Failed if mine => (s::MEDIA_NOT_SENT().to_owned(), p.coral),
        FileState::Failed => (s::MEDIA_NOT_RECEIVED().to_owned(), p.coral),
        FileState::Cancelled => (s::CANCELLED().to_owned(), p.t3),
        _ => (s::size(media.size), p.t3),
    };
    let note = match &media.state {
        FileState::Receiving { .. } | FileState::Sending { .. } | FileState::Cancelled => {
            format!("{} · {note}", s::size(media.size))
        }
        _ => note,
    };
    let sub = Type::Caption.galley(ui, &note, color, tw, 1);
    let th = name.size().y + sub.size().y;
    let ty = rect.center().y - th / 2.0;
    let nh = name.size().y;
    let nr = flip(Rect::from_min_size(pos2(tx, ty), name.size()));
    ui.painter().galley(nr.min, name, p.t1);
    let sr = flip(Rect::from_min_size(pos2(tx, ty + nh), sub.size()));
    ui.painter().galley(sr.min, sub, color);
    crate::a11y::item(
        &resp,
        egui::Role::Group,
        &s::file_spoken(&media.name, &s::size(media.size)),
    );
    ui.add_space(m::MARGIN);
    out
}

/// Under media that failed: a coral line.
pub fn failed_line(ui: &mut Ui, mine: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let text = if mine {
        s::MEDIA_NOT_SENT()
    } else {
        s::MEDIA_NOT_RECEIVED()
    };
    let g = Type::Micro.galley(ui, text, p.coral, ui.available_width(), 1);
    let (r, resp) = ui.allocate_exact_size(vec2(g.size().x + 16.0, 16.0), Sense::hover());
    paint_icon(
        ui.painter(),
        pos2(r.left() + 5.5, r.center().y),
        icons::WARNING,
        11.0,
        p.coral,
    );
    let gs = g.size();
    ui.painter()
        .galley(pos2(r.left() + 16.0, r.center().y - gs.y / 2.0), g, p.coral);
    crate::a11y::item(&resp, egui::Role::Status, text);
}

/// What the viewer asks for.
pub enum ViewerAct {
    /// Close it.
    Close,
    /// Open the file with the system's app.
    Open,
}

/// The picture over the whole window: dimmed, the picture fitted 48 in from the edges, an
/// Open button and × at the top right. A click outside the picture closes it.
pub fn viewer(ctx: &Context, key: &str, media: &Media) -> Option<ViewerAct> {
    let screen = ctx.content_rect();
    let mut out = None;
    egui::Area::new(Id::unique("chat.media.viewer"))
        .order(egui::Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, bg) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(
                rect,
                CornerRadius::ZERO,
                Color32::from_black_alpha(m::VIEW_SCRIM),
            );
            let full = match &media.state {
                FileState::Received(path) => {
                    let p = path.clone();
                    match picture(ctx, &format!("f:{}", path.display()), || {
                        Some(Source::File(p))
                    }) {
                        Got::Ready(pic) => Some(pic),
                        _ => None,
                    }
                }
                _ => None,
            };
            let pic = full.or_else(|| {
                media
                    .preview
                    .as_deref()
                    .and_then(|b| preview(ctx, &format!("pv:{key}"), b))
            });
            let area = rect.shrink(m::VIEW_IN);
            if let Some(pic) = &pic {
                let (w, h) = (pic.size[0].max(1) as f32, pic.size[1].max(1) as f32);
                let k = (area.width() / w).min(area.height() / h);
                let r = Rect::from_center_size(area.center(), vec2(w * k, h * k));
                paint_picture(ui, r, pic, CornerRadius::same(4), Color32::WHITE);
            }
            // Top right: Open (a received file) and ×.
            let close = Rect::from_min_size(
                pos2(rect.right() - 16.0 - 36.0, rect.top() + 16.0),
                vec2(36.0, 36.0),
            );
            let cr = ui.interact(close, Id::unique("chat.media.viewer.close"), Sense::click());
            crate::a11y::button(&cr, s::CLOSE());
            ui.painter().circle_filled(
                close.center(),
                18.0,
                Color32::from_white_alpha(if cr.hovered() { 60 } else { 30 }),
            );
            paint_icon(
                ui.painter(),
                close.center(),
                icons::CLOSE,
                16.0,
                Color32::WHITE,
            );
            if cr.clicked() {
                out = Some(ViewerAct::Close);
            }
            if matches!(media.state, FileState::Received(_)) {
                let b = crate::widgets::button::Button::new(s::OPEN()).icon(icons::OPEN);
                let bw = b.width(ui);
                let br = Rect::from_min_size(
                    pos2(close.left() - 8.0 - bw, close.center().y - 15.0),
                    vec2(bw, 30.0),
                );
                if ui.put(br, b).clicked() {
                    out = Some(ViewerAct::Open);
                }
            }
            if out.is_none() && bg.clicked() {
                out = Some(ViewerAct::Close);
            }
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_png_and_rejects_garbage() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(3, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap_or_default();
        let (size, rgba) = decode(&png).unwrap_or_default();
        assert_eq!(size, [3, 2]);
        assert_eq!(rgba.len(), 3 * 2 * 4);
        assert!(decode(b"not a picture").is_err());
    }
}

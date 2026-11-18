//! Dialogs (CSS "9 · OVERLAYS", notes §5 "Dialog", §6 "MOTION").
//!
//! One dialog for everything that asks or explains over a page: the scrim, then a `raised`
//! panel 440 wide (560 for the wide ones), radius 14, 22 padding, a 1 pt `line` outline and
//! the dialog shadow, never closer than 20 to the window's edge. Inside: the title in the
//! display face, an optional sentence of text 8 below it, the body 16 below that (only the
//! body scrolls), and the actions 22 below, right-aligned, 8 apart. An optional close button
//! (×) sits 14 from the top right corner.
//!
//! Motion: the scrim fades in over 220 ms; the panel fades in over 220 ms while it rises 10
//! and grows from 97 % over 300 ms (`ease_out`). Leaving, everything fades over 140 ms while
//! the panel sinks back (`ease_in`). A change of the panel's height (a new step, a detail
//! opened) morphs over 300 ms, and a new step's content arrives (alpha, rise 8).
//!
//! Leaving is painted by [`paint_exits`] from what the dialog last drew, so callers simply
//! stop drawing a dialog when it closes. Menus and toasts use the same machinery
//! ([`track_exit`]).
//!
//! Focus: when an overlay first appears, the widget that had focus (the invoker) is
//! remembered; [`closed`] hands focus back to it. egui's modal layer keeps pointer input and
//! focus inside the topmost dialog. `Esc` closes.

use std::collections::HashMap;
use std::sync::Arc;

use egui::epaint::ClippedShape;
use egui::{
    Align, Context, CornerRadius, Frame, Id, Key, LayerId, Layout, Margin, Modal, Order, Painter,
    Pos2, Rect, ScrollArea, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};

use super::button::{Button, Variant};
use super::icon_button::IconButton;
use crate::a11y;
use crate::strings;
use crate::theme::motion::{ease_in, ease_out, scale_about};
use crate::theme::palette::fade;
use crate::theme::tokens::{control, layout, overlay, radius, stroke, travel};
use crate::theme::{self, Dur, Icon, Type, icons};

#[derive(Clone, Copy, Debug, Default)]
struct Invoker(Option<Id>);

fn invoker_key(overlay: Id) -> Id {
    overlay.with("invoker")
}

/// Remember the focused widget when `overlay` first shows (idempotent while it stays open).
pub fn remember_invoker(ctx: &Context, overlay: Id) {
    let key = invoker_key(overlay);
    if ctx.data(|d| d.get_temp::<Invoker>(key)).is_none() {
        let focused = ctx.memory(|m| m.focused());
        ctx.data_mut(|d| d.insert_temp(key, Invoker(focused)));
    }
}

fn pending_key() -> Id {
    Id::unique("zaun.focus_return")
}

/// The overlay closed: return focus to its invoker. The request is applied by
/// [`apply_focus_return`] at the start of the next frame, once the modal layer is gone
/// (egui keeps focus inside the topmost modal while it is still drawn).
pub fn closed(ctx: &Context, overlay: Id) {
    let key = invoker_key(overlay);
    if let Some(Invoker(Some(id))) = ctx.data_mut(|d| d.remove_temp::<Invoker>(key)) {
        ctx.data_mut(|d| d.insert_temp(pending_key(), Invoker(Some(id))));
        ctx.request_repaint();
    }
}

/// Apply a pending focus return. Call once at the start of every frame (the shell does).
pub fn apply_focus_return(ctx: &Context) {
    // Wait until egui no longer considers a modal layer topmost; focus requests for widgets
    // below a modal are ignored.
    if ctx.memory(|m| m.top_modal_layer()).is_some() {
        if ctx.data(|d| d.get_temp::<Invoker>(pending_key())).is_some() {
            ctx.request_repaint();
        }
        return;
    }
    if let Some(Invoker(Some(id))) = ctx.data_mut(|d| d.remove_temp::<Invoker>(pending_key())) {
        ctx.memory_mut(|m| m.request_focus(id));
    }
}

/// The invoker recorded for `overlay` (tests).
pub fn invoker(ctx: &Context, overlay: Id) -> Option<Id> {
    ctx.data(|d| d.get_temp::<Invoker>(invoker_key(overlay)))
        .and_then(|i| i.0)
}

// ── Leaving ─────────────────────────────────────────────────────────────────────────────

/// How something that floats leaves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExitKind {
    /// A dialog: its first shape is the scrim, which only fades; the panel (centred at
    /// `center`) fades, sinks 10 and shrinks to 97 % over 140 ms.
    Dialog {
        /// The panel's centre.
        center: Pos2,
    },
    /// A menu: fades over 90 ms while it rises 4 and shrinks to 97 % towards `pivot`, the
    /// corner it hangs from.
    Menu {
        /// The corner the menu hangs from.
        pivot: Pos2,
    },
    /// A toast: fades over 140 ms while it sinks 10.
    Toast,
}

#[derive(Clone)]
struct Exit {
    layer: LayerId,
    shapes: Arc<Vec<ClippedShape>>,
    last_pass: u64,
    closing_since: Option<f64>,
    kind: ExitKind,
}

#[derive(Clone, Default)]
struct Exits {
    map: HashMap<Id, Exit>,
    painted_pass: Option<u64>,
}

fn exits_key() -> Id {
    Id::unique("zaun.exits")
}

/// Remember what `layer` drew this pass, so that when `key` stops being drawn its last look
/// can leave as `kind` says. Call after the layer's content is complete.
pub fn track_exit(ctx: &Context, key: Id, layer: LayerId, kind: ExitKind) {
    let shapes: Vec<ClippedShape> = ctx.graphics(|g| {
        g.get(layer)
            .map(|l| l.all_entries().cloned().collect())
            .unwrap_or_default()
    });
    let pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|d| {
        let exits = d.get_temp_mut_or_default::<Exits>(exits_key());
        exits.map.insert(
            key,
            Exit {
                layer,
                shapes: Arc::new(shapes),
                last_pass: pass,
                closing_since: None,
                kind,
            },
        );
    });
}

/// Paint whatever is leaving: everything tracked with [`track_exit`] that was not drawn
/// this pass. Call once at the end of every pass, after every overlay (the toast layer does
/// it; calling it again in the same pass does nothing).
pub fn paint_exits(ctx: &Context) {
    let pass = ctx.cumulative_pass_nr();
    let now = ctx.input(|i| i.time);
    let motion = theme::theme(ctx).motion;
    let mut exits = ctx.data(|d| d.get_temp::<Exits>(exits_key()).unwrap_or_default());
    if exits.painted_pass == Some(pass) {
        return;
    }
    exits.painted_pass = Some(pass);
    let mut done = Vec::new();
    for (key, e) in &mut exits.map {
        if e.last_pass >= pass {
            e.closing_since = None;
            continue;
        }
        // Only what was drawn in the pass just before starts leaving; anything older was
        // not seen leaving (this did not run in between) and is dropped.
        if e.closing_since.is_none() && e.last_pass + 1 < pass {
            done.push(*key);
            continue;
        }
        let since = *e.closing_since.get_or_insert(now);
        let fade_d = match e.kind {
            ExitKind::Menu { .. } => Dur::Quick,
            _ => Dur::Fast,
        };
        let fade_s = motion.fade(fade_d);
        let move_s = motion.movement(fade_d);
        let elapsed = (now - since) as f32;
        if fade_s <= 0.0 || elapsed >= fade_s {
            done.push(*key);
            continue;
        }
        let alpha = 1.0 - elapsed / fade_s;
        let t = if move_s > 0.0 {
            ease_in((elapsed / move_s).clamp(0.0, 1.0))
        } else {
            0.0
        };
        let transform = match e.kind {
            ExitKind::Dialog { center } => scale_about(
                center,
                1.0 - (1.0 - travel::POP_FROM) * t,
                vec2(0.0, travel::TOAST_RISE * t),
            ),
            ExitKind::Menu { pivot } => scale_about(
                pivot,
                1.0 - (1.0 - travel::POP_FROM) * t,
                vec2(0.0, -travel::MENU_DROP * t),
            ),
            ExitKind::Toast => scale_about(Pos2::ZERO, 1.0, vec2(0.0, travel::TOAST_RISE * t)),
        };
        let ghost = LayerId::new(e.layer.order, key.with("exit"));
        let painter = Painter::new(ctx.clone(), ghost, ctx.content_rect());
        for (i, cs) in e.shapes.iter().enumerate() {
            let scrim = i == 0 && matches!(e.kind, ExitKind::Dialog { .. });
            let mut p = painter.with_clip_rect(cs.clip_rect);
            p.set_opacity(alpha);
            let idx = p.add(cs.shape.clone());
            let (tf, clip) = if scrim {
                (cs.transform_after_tessellation, cs.clip_rect)
            } else {
                (
                    transform * cs.transform_after_tessellation,
                    transform.mul_rect(cs.clip_rect),
                )
            };
            ctx.graphics_mut(|g| {
                g.entry(ghost).mutate_shape(idx, |c| {
                    c.transform_after_tessellation = tf;
                    c.clip_rect = clip;
                });
            });
        }
        ctx.request_repaint();
    }
    for k in done {
        exits.map.remove(&k);
    }
    ctx.data_mut(|d| d.insert_temp(exits_key(), exits));
}

// ── Text set as the design's CSS sets it ────────────────────────────────────────────────

/// `text` in `style`, wrapped at `width` with its lines balanced (`text-wrap: balance`):
/// as narrow as it can be without taking another line. Titles and headlines.
pub fn balanced(
    ui: &Ui,
    style: Type,
    text: &str,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let full = style.galley(ui, text, color, width, usize::MAX);
    let rows = full.rows.len();
    if rows < 2 {
        return full;
    }
    let (mut lo, mut hi) = (width / rows as f32, width);
    for _ in 0..10 {
        let mid = (lo + hi) / 2.0;
        if style.galley(ui, text, color, mid, usize::MAX).rows.len() > rows {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    style.galley(ui, text, color, hi.ceil(), usize::MAX)
}

/// `text` in `style`, wrapped at `width` without a lone word on its last line
/// (`text-wrap: pretty`): when the last line would be one word, the paragraph is set a
/// little narrower, keeping its number of lines. Sentences under titles and headlines.
pub fn pretty(
    ui: &Ui,
    style: Type,
    text: &str,
    color: egui::Color32,
    width: f32,
) -> Arc<egui::Galley> {
    let full = style.galley(ui, text, color, width, usize::MAX);
    let lone = |g: &egui::Galley| {
        g.rows
            .last()
            .is_some_and(|r| !r.text().trim().contains(' '))
    };
    let rows = full.rows.len();
    if rows < 2 || !lone(&full) {
        return full;
    }
    let mut w = width;
    while w > width * 0.7 {
        w -= width * 0.02;
        let g = style.galley(ui, text, color, w, usize::MAX);
        if g.rows.len() != rows {
            break;
        }
        if !lone(&g) {
            return g;
        }
    }
    full
}

// ── The dialog ──────────────────────────────────────────────────────────────────────────

/// What a dialog is made of, besides its body and actions.
#[derive(Clone, Copy, Debug)]
pub struct Spec<'a> {
    /// The title (also the dialog's accessible name).
    pub title: &'a str,
    /// One sentence or two under the title, 14 / 20 in `t2`.
    pub text: Option<&'a str>,
    /// 560 wide instead of 440 (Add a game, Artwork, Report).
    pub wide: bool,
    /// Show the close button (×) at the top right.
    pub close_button: bool,
    /// A dialog of several steps: a change of this value makes the new step's content
    /// arrive (alpha, rise 8) while the panel's height morphs.
    pub step: Option<u64>,
    /// There is unsaved input: a click on the scrim does not close.
    pub unsaved: bool,
    /// The dialog has an actions row.
    pub actions: bool,
}

impl<'a> Spec<'a> {
    /// A 440 wide dialog titled `title`, with actions and no close button.
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            text: None,
            wide: false,
            close_button: false,
            step: None,
            unsaved: false,
            actions: true,
        }
    }

    /// The sentence under the title.
    pub fn text(mut self, text: &'a str) -> Self {
        if !text.is_empty() {
            self.text = Some(text);
        }
        self
    }

    /// 560 wide.
    pub fn wide(mut self) -> Self {
        self.wide = true;
        self
    }

    /// With the close button (×).
    pub fn close_button(mut self) -> Self {
        self.close_button = true;
        self
    }

    /// A dialog of several steps, now at `step`.
    pub fn step(mut self, step: u64) -> Self {
        self.step = Some(step);
        self
    }

    /// Unsaved input: the scrim does not close it.
    pub fn unsaved(mut self, unsaved: bool) -> Self {
        self.unsaved = unsaved;
        self
    }

    /// Without an actions row (the keyboard shortcuts).
    pub fn no_actions(mut self) -> Self {
        self.actions = false;
        self
    }

    /// With an actions row or without (a step that has none).
    pub fn with_actions(mut self, actions: bool) -> Self {
        self.actions = actions;
        self
    }
}

/// What a dialog's parts returned this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown<B, R> {
    /// What the body returned.
    pub body: B,
    /// What the actions row returned.
    pub footer: R,
    /// The close button, `Esc` or the scrim asked to close.
    pub close_requested: bool,
    /// This is the topmost dialog.
    pub is_top: bool,
}

fn natural_key(id: Id) -> Id {
    id.with("natural_h")
}

fn footer_key(id: Id) -> Id {
    id.with("footer_h")
}

/// Show a dialog: `body` fills the scrolling part, `footer` the actions row (a
/// right-to-left layout: add the main action first; a left-to-right layout added last
/// takes what is left at the row's start, for "Back"). Returns what they returned and
/// whether a close was asked for; when it was, the caller stops drawing the dialog and it
/// leaves on its own. Focus returns to the invoker.
pub fn show<B, R>(
    ctx: &Context,
    id: Id,
    spec: Spec<'_>,
    body: impl FnOnce(&mut Ui) -> B,
    footer: impl FnOnce(&mut Ui) -> R,
) -> Shown<B, R> {
    remember_invoker(ctx, id);
    let theme = theme::theme(ctx);
    let p = theme.palette.clone();
    let motion = theme.motion;
    let screen = ctx.content_rect();
    let alpha = theme::motion::appear(ctx, id.with("in.alpha"), motion.fade(Dur::Base));
    let travel_t = ease_out(theme::motion::appear(
        ctx,
        id.with("in.move"),
        motion.movement(Dur::Move),
    ));
    let pad = layout::SHEET_PAD;
    let want_w = if spec.wide {
        layout::SHEET_MAX_W
    } else {
        layout::DIALOG_MAX_W
    };
    let w = want_w
        .min(screen.width() - layout::DIALOG_MARGIN * 2.0)
        .max(pad * 4.0);
    let max_h = (screen.height() - layout::DIALOG_MARGIN * 2.0).max(pad * 4.0);
    let cw = w - pad * 2.0;
    let known_natural: Option<f32> = ctx.data(|d| d.get_temp(natural_key(id)));
    let painted_h = known_natural.map(|h| {
        theme::motion::glide(
            ctx,
            id.with("height"),
            h.min(max_h),
            motion.movement(Dur::Move),
        )
    });
    let footer_h: f32 = ctx
        .data(|d| d.get_temp(footer_key(id)))
        .unwrap_or(control::BUTTON);
    let mut close = false;
    let mut center = screen.center();
    let resp = Modal::new(id)
        .backdrop_color(fade(p.scrim, alpha))
        .frame(Frame::NONE)
        .show(ctx, |ui| {
            a11y::labelled(ui.ctx(), ui.scope_id(), egui::Role::Dialog, spec.title);
            let origin = ui.cursor().min;
            let est_h = painted_h.unwrap_or(max_h);
            let est_center = origin + vec2(w / 2.0, est_h / 2.0);
            let scale = 1.0 - (1.0 - travel::POP_FROM) * (1.0 - travel_t);
            let rise = travel::TOAST_RISE * (1.0 - travel_t);
            let transform = scale_about(est_center, scale, vec2(0.0, rise));
            ui.with_visual_transform(transform, |ui| {
                ui.multiply_opacity(alpha);
                let plate = ui.painter().add(Shape::Noop);
                let clip = painted_h
                    .map(|h| Rect::from_min_size(origin, vec2(w, h)).intersect(ui.clip_rect()));
                let mut content = ui.new_child(
                    UiBuilder::new()
                        .id_salt("dialog.content")
                        .max_rect(Rect::from_min_size(
                            origin + vec2(pad, pad),
                            vec2(cw, f32::INFINITY),
                        ))
                        .layout(Layout::top_down(Align::Min)),
                );
                if let Some(c) = clip {
                    content.set_clip_rect(c);
                }
                content.spacing_mut().item_spacing = vec2(0.0, 0.0);
                let (inner, natural_inner, measured_footer) =
                    lay_out(&mut content, id, &spec, cw, max_h, footer_h, body, footer);
                let natural = natural_inner + pad * 2.0;
                let h = painted_h.unwrap_or(natural.min(max_h));
                let panel = Rect::from_min_size(origin, vec2(w, h));
                let corner = CornerRadius::same(radius::LG);
                let shadow = p.shade_dialog.shadow().as_shape(panel, corner);
                ui.painter().set(
                    plate,
                    Shape::Vec(vec![
                        shadow.into(),
                        Shape::rect_filled(panel, corner, p.raised),
                        Shape::rect_stroke(
                            panel,
                            corner,
                            Stroke::new(stroke::BORDER, p.line),
                            StrokeKind::Inside,
                        ),
                    ]),
                );
                if spec.close_button {
                    let s = control::BUTTON;
                    let r = Rect::from_min_size(
                        pos2(
                            panel.right() - overlay::DIALOG_CLOSE_INSET - s,
                            panel.top() + overlay::DIALOG_CLOSE_INSET,
                        ),
                        vec2(s, s),
                    );
                    let x = ui.put(
                        r,
                        IconButton::new(icons::CLOSE, strings::feedback::CLOSE())
                            .size(s)
                            .shortcut(strings::shortcuts::ESC()),
                    );
                    close |= x.clicked();
                }
                ui.allocate_rect(panel, Sense::hover());
                ctx.data_mut(|d| {
                    d.insert_temp(natural_key(id), natural);
                    d.insert_temp(footer_key(id), measured_footer);
                });
                if known_natural.is_none() {
                    ctx.request_repaint();
                }
                center = panel.center();
                inner
            })
            .inner
        });
    if resp.should_close() {
        // A click on the scrim is ignored while there is unsaved input; `Esc` still asks.
        let backdrop = resp.backdrop_response.clicked();
        if !(backdrop && spec.unsaved) {
            close = true;
        }
    }
    track_exit(
        ctx,
        id,
        LayerId::new(Order::Foreground, id),
        ExitKind::Dialog { center },
    );
    if close {
        closed(ctx, id);
    }
    let (b, f) = resp.inner;
    Shown {
        body: b,
        footer: f,
        close_requested: close,
        is_top: resp.is_top_modal,
    }
}

/// Lay out a dialog's title, text, body and actions in `ui` (the text column). Returns what
/// body and footer returned, the natural height of the content, and the actions' height.
#[allow(clippy::too_many_arguments)]
fn lay_out<B, R>(
    ui: &mut Ui,
    id: Id,
    spec: &Spec<'_>,
    cw: f32,
    max_h: f32,
    footer_h: f32,
    body: impl FnOnce(&mut Ui) -> B,
    footer: impl FnOnce(&mut Ui) -> R,
) -> ((B, R), f32, f32) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let motion = theme::theme(ui.ctx()).motion;
    let left = ui.max_rect().left();
    let top = ui.max_rect().top();
    // The title, with room for the close button at its end.
    let title = balanced(
        ui,
        Type::Title,
        spec.title,
        p.t1,
        (cw - overlay::DIALOG_TITLE_END).max(1.0),
    );
    let title_h = title.size().y;
    ui.painter().galley(pos2(left, top), title, p.t1);
    let y = top + title_h;
    // What follows the title arrives with a new step.
    let (step_alpha, step_drop) = match spec.step {
        Some(s) => theme::motion::arrive(
            ui.ctx(),
            id.with(("step", s)),
            motion,
            0.0,
            Dur::Move,
            travel::RISE,
        ),
        None => (1.0, 0.0),
    };
    let start = y;
    let mut rest = ui.new_child(
        UiBuilder::new()
            .id_salt("dialog.rest")
            .max_rect(Rect::from_min_size(pos2(left, y), vec2(cw, f32::INFINITY)))
            .layout(Layout::top_down(Align::Min)),
    );
    rest.spacing_mut().item_spacing = vec2(0.0, 0.0);
    let shift = egui::emath::TSTransform::from_translation(vec2(0.0, step_drop));
    let (out, end, measured_footer) = rest
        .with_visual_transform(shift, |ui| {
            ui.multiply_opacity(step_alpha);
            let mut y = start;
            if let Some(text) = spec.text {
                y += overlay::DIALOG_TEXT_GAP;
                let g = pretty(ui, Type::Prose, text, p.t2, cw);
                let h = g.size().y;
                let r = Rect::from_min_size(pos2(left, y), g.size());
                ui.painter().galley(r.min, g, p.t2);
                let node = ui.interact(r, id.with("dialog.text"), Sense::hover());
                a11y::item(&node, egui::Role::Label, text);
                y += h;
            }
            // The body: the only part that scrolls. It reaches 8 past the text column on both
            // sides, so focus rings inside it are not cut.
            let [side, vpad] = overlay::DIALOG_BODY_PAD;
            let header_h = y - start + title_h;
            let actions_h = if spec.actions {
                overlay::DIALOG_ACTIONS_GAP + footer_h
            } else {
                0.0
            };
            let body_max = (max_h
                - layout::SHEET_PAD * 2.0
                - header_h
                - overlay::DIALOG_BODY_GAP
                - actions_h
                - vpad * 2.0)
                .max(control::BUTTON);
            let body_top = y + overlay::DIALOG_BODY_GAP;
            let mut b = ui.new_child(
                UiBuilder::new()
                    .id_salt("dialog.body")
                    .max_rect(Rect::from_min_size(
                        pos2(left - side, body_top),
                        vec2(cw + side * 2.0, f32::INFINITY),
                    ))
                    .layout(Layout::top_down(Align::Min)),
            );
            let mut inner_h = 0.0;
            let body_out = ScrollArea::vertical()
                .id_salt(id.with("body"))
                .max_height(body_max + vpad * 2.0)
                .auto_shrink([false, true])
                .show(&mut b, |ui| {
                    Frame::NONE
                        .inner_margin(Margin::symmetric(side as i8, vpad as i8))
                        .show(ui, |ui| {
                            ui.set_width(cw);
                            ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                            let top = ui.cursor().min.y;
                            let o = body(ui);
                            inner_h = ui.min_rect().bottom() - top;
                            o
                        })
                        .inner
                })
                .inner;
            if inner_h > 0.5 {
                y = b.min_rect().bottom();
            }
            // The actions.
            let mut fh = 0.0;
            let foot = if spec.actions {
                y += overlay::DIALOG_ACTIONS_GAP;
                let mut f = ui.new_child(
                    UiBuilder::new()
                        .id_salt("dialog.actions")
                        .max_rect(Rect::from_min_size(
                            pos2(left, y),
                            vec2(cw, control::BUTTON),
                        ))
                        .layout(Layout::right_to_left(Align::Center).with_main_wrap(true)),
                );
                f.spacing_mut().item_spacing =
                    vec2(overlay::DIALOG_ACTION_GAP, overlay::DIALOG_ACTION_GAP);
                let o = footer(&mut f);
                fh = f.min_rect().height().max(control::BUTTON);
                y += fh;
                o
            } else {
                let mut f = ui.new_child(
                    UiBuilder::new()
                        .id_salt("dialog.actions")
                        .max_rect(Rect::from_min_size(pos2(left, y), vec2(cw, 0.0)))
                        .invisible(),
                );
                footer(&mut f)
            };
            ((body_out, foot), y, fh)
        })
        .inner;
    let _ = left;
    (out, end - top, measured_footer)
}

/// A dialog action.
#[derive(Clone, Copy, Debug)]
pub struct Action<'a> {
    /// Verb label ("Remove game").
    pub label: &'a str,
    /// Emphasis: `Secondary` (boxed), `Primary` for the main one, `Danger` for a
    /// destructive one.
    pub variant: Variant,
}

/// What happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogOutcome {
    /// Still open.
    Open,
    /// An action was chosen (index into `actions`).
    Chose(usize),
    /// Dismissed with `Esc`, the close button or the scrim: treat as the safe choice.
    Cancelled,
}

/// Show a dialog of title and text. `default` is the index of the action that has focus
/// when it opens (`Enter` activates it; never a destructive one). Actions are laid out
/// right-aligned in the order given, the last rightmost. `icon` is not drawn: dialogs have
/// no icons.
pub fn dialog(
    ctx: &Context,
    id: Id,
    icon: Option<Icon>,
    title: &str,
    body: &str,
    actions: &[Action<'_>],
    default: usize,
) -> DialogOutcome {
    let _ = icon;
    choose(
        ctx,
        id,
        Spec::new(title).text(body),
        actions,
        default,
        |_| {},
    )
}

/// Whether the dialog `id` shows for the first pass (it was not drawn in the pass before):
/// the moment to put focus on its main control. Call it once per pass for a dialog.
pub fn opening(ctx: &Context, id: Id) -> bool {
    let key = id.with("opening");
    let pass = ctx.cumulative_pass_nr();
    let first = ctx
        .data(|d| d.get_temp::<u64>(key))
        .is_none_or(|last| last + 1 < pass);
    ctx.data_mut(|d| d.insert_temp(key, pass));
    first
}

/// A dialog laid out by `spec` whose actions are `actions`: returns which was chosen. The
/// action `default` has focus when the dialog opens and is ringed while it has it.
pub fn choose(
    ctx: &Context,
    id: Id,
    spec: Spec<'_>,
    actions: &[Action<'_>],
    default: usize,
    body: impl FnOnce(&mut Ui),
) -> DialogOutcome {
    let theme = theme::theme(ctx);
    let mut outcome = DialogOutcome::Open;
    // The default action takes focus when the dialog first shows.
    let first = opening(ctx, id);
    let shown = show(ctx, id, spec, body, |ui| {
        for (i, a) in actions.iter().enumerate().rev() {
            let r = ui.add(Button::new(a.label).variant(a.variant));
            if i == default && first {
                r.request_focus();
            }
            if i == default && r.has_focus() {
                // `Enter` chooses this one: always marked.
                super::default_ring(ui.painter(), r.rect, CornerRadius::same(radius::MD), &theme);
            }
            if r.clicked() {
                outcome = DialogOutcome::Chose(i);
            }
        }
    });
    if outcome == DialogOutcome::Open
        && shown.is_top
        && ctx.memory(|m| m.focused()).is_none()
        && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter))
    {
        outcome = DialogOutcome::Chose(default);
    }
    if outcome == DialogOutcome::Open && shown.close_requested {
        outcome = DialogOutcome::Cancelled;
    }
    if outcome != DialogOutcome::Open && !shown.close_requested {
        closed(ctx, id);
    }
    outcome
}

/// The detail block of a dialog (`.pre`): mono 12 / 18 in `t2` on `o1`, radius 8, padding
/// 10 × 8, the text selectable. `gap` puts the design's 8 above it.
pub fn detail_block(ui: &mut Ui, text: &str, gap: bool) {
    let p = theme::theme(ui.ctx()).palette.clone();
    let [top, side, vpad] = overlay::PRE;
    if gap {
        ui.add_space(top);
    }
    Frame::NONE
        .fill(p.o1)
        .corner_radius(CornerRadius::same(radius::ROW))
        .inner_margin(Margin::symmetric(side as i8, vpad as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut job = Type::Mono.job(text, p.t2, ui.available_width(), usize::MAX);
            for s in &mut job.sections {
                s.format.line_height = Some(overlay::PRE_LINE);
            }
            ui.add(egui::Label::new(job).selectable(true));
        });
}

/// Fine print (12 / 17 in `t3`), wrapped to the width at hand.
pub fn fine_print(ui: &mut Ui, text: &str) -> egui::Response {
    let p = theme::theme(ui.ctx()).palette.clone();
    Type::Caption.paragraph_lh(ui, text, p.t3, false, Some(overlay::FINE_LINE))
}

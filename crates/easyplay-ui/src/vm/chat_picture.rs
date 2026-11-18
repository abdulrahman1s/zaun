//! The picture part of *Your profile* and of a group's picture (ADR-0038; chat-calls.html C):
//! the file being read, the crop step's view, and what Save sends. Nothing is sent before
//! Save.

use std::path::PathBuf;

use easyplay_core::error::UserError;
use easyplay_core::ids::QueryId;
use easyplay_core::model::{PictureCrop, PictureSource};
use easyplay_core::service::ChatCommand;

use crate::bridge::{Command, QueryPayload, UiBridge};

/// The zoom's range: 100 % (the picture just covers the area) to 400 %.
pub const ZOOM: [f32; 2] = [1.0, 4.0];

/// Where the crop step looks: the point of the picture under the circle's centre, in pixels
/// of the picture as shown (`PictureSource::{width, height}`), and the zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CropView {
    pub centre: [f32; 2],
    pub zoom: f32,
}

impl CropView {
    /// The whole picture, centred.
    pub fn middle(size: [f32; 2]) -> Self {
        Self {
            centre: [size[0] / 2.0, size[1] / 2.0],
            zoom: 1.0,
        }
    }

    /// Points per picture pixel in an `area`: at zoom 1 the picture just covers it.
    pub fn scale(&self, size: [f32; 2], area: [f32; 2]) -> f32 {
        let cover = (area[0] / size[0].max(1.0)).max(area[1] / size[1].max(1.0));
        cover * self.zoom
    }

    /// Keep the zoom in range and the picture over the whole area.
    pub fn clamp(&mut self, size: [f32; 2], area: [f32; 2]) {
        self.zoom = self.zoom.clamp(ZOOM[0], ZOOM[1]);
        let s = self.scale(size, area);
        for i in 0..2 {
            let half = area[i] / 2.0 / s;
            self.centre[i] = if half * 2.0 >= size[i] {
                size[i] / 2.0
            } else {
                self.centre[i].clamp(half, size[i] - half)
            };
        }
    }

    /// The circle's square, `[x, y, side]` in picture pixels.
    pub fn square(&self, size: [f32; 2], area: [f32; 2], circle: f32) -> [f32; 3] {
        let side = circle / self.scale(size, area);
        [
            self.centre[0] - side / 2.0,
            self.centre[1] - side / 2.0,
            side,
        ]
    }

    /// The crop the core takes (it clamps it to the picture).
    pub fn crop(&self, size: [f32; 2], area: [f32; 2], circle: f32) -> PictureCrop {
        let [x, y, side] = self.square(size, area, circle);
        PictureCrop {
            x: x.max(0.0).round() as u32,
            y: y.max(0.0).round() as u32,
            side: side.round().max(1.0) as u32,
        }
    }
}

/// A picture read and placed.
#[derive(Clone, Debug, PartialEq)]
pub struct Placed {
    pub source: PictureSource,
    pub view: CropView,
    /// The crop step's area the view was placed in (points).
    pub area: [f32; 2],
}

impl Placed {
    pub fn size(&self) -> [f32; 2] {
        [self.source.width as f32, self.source.height as f32]
    }
}

/// What Save does with the picture.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum PictureChange {
    #[default]
    Keep,
    Set(Placed),
    Remove,
}

/// Whose picture a dialog changes: yours (`None`) or a group's.
pub type Whose = Option<i64>;

/// The picture's part of the dialog.
#[derive(Clone, Debug, Default)]
pub struct PictureEdit {
    /// The dialog this belongs to (`None`: none is open). A file chosen for another one, or
    /// after it closed, is dropped.
    pub whose: Option<Whose>,
    /// `LoadPicture` is on its way, asked at this UI time (s).
    pub loading: Option<(QueryId, f64)>,
    /// The crop step is open over this picture.
    pub crop: Option<Placed>,
    pub change: PictureChange,
    /// Why the last file could not be used (the status line).
    pub error: Option<UserError>,
}

impl PictureEdit {
    /// The state of a dialog that just opened for `whose`.
    pub fn new(whose: Whose) -> Self {
        Self {
            whose: Some(whose),
            ..Self::default()
        }
    }

    /// A file was chosen for `whose`: read it for the crop step.
    pub fn load(&mut self, bridge: &UiBridge, whose: Whose, path: PathBuf, now: f64) {
        if self.whose != Some(whose) {
            return;
        }
        let reply_to = bridge.query_id();
        self.loading = Some((reply_to, now));
        self.error = None;
        bridge.send(Command::Chat(ChatCommand::LoadPicture { path, reply_to }));
    }

    /// `LoadPicture`'s answer: the crop step opens, or the status line says why not.
    pub fn on_query(&mut self, id: QueryId, payload: &QueryPayload) -> bool {
        if self.loading.is_none_or(|(q, _)| q != id) {
            return false;
        }
        self.loading = None;
        match payload {
            QueryPayload::PictureSource(source) => {
                let size = [source.width as f32, source.height as f32];
                self.crop = Some(Placed {
                    source: source.clone(),
                    view: CropView::middle(size),
                    area: [0.0, 0.0],
                });
            }
            QueryPayload::Failed(e) => self.error = Some(e.clone()),
            _ => {}
        }
        true
    }

    /// The commands Save sends for the picture of `group` (`None`: yours).
    pub fn commands(&self, group: Option<i64>, circle: f32) -> Option<ChatCommand> {
        match (&self.change, group) {
            (PictureChange::Keep, _) => None,
            (PictureChange::Remove, None) => Some(ChatCommand::ClearPicture),
            (PictureChange::Remove, Some(g)) => Some(ChatCommand::ClearGroupPicture(g)),
            (PictureChange::Set(p), group) => {
                let crop = Some(p.view.crop(p.size(), p.area, circle));
                let path = p.source.path.clone();
                Some(match group {
                    None => ChatCommand::SetPicture { path, crop },
                    Some(group) => ChatCommand::SetGroupPicture { group, path, crop },
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: [f32; 2] = [396.0, 280.0];

    #[test]
    fn at_zoom_one_the_picture_covers_the_area_and_cannot_leave_it() {
        let size = [960.0, 640.0];
        let mut v = CropView::middle(size);
        assert!((v.scale(size, AREA) - 0.4375).abs() < 1e-4);
        v.centre = [0.0, 10_000.0];
        v.clamp(size, AREA);
        let s = v.scale(size, AREA);
        assert!((v.centre[0] - AREA[0] / 2.0 / s).abs() < 1e-3);
        assert!((v.centre[1] - (size[1] - AREA[1] / 2.0 / s)).abs() < 1e-3);
    }

    #[test]
    fn the_crop_is_the_circle_in_picture_pixels() {
        let size = [960.0, 640.0];
        let mut v = CropView::middle(size);
        v.zoom = 2.0;
        v.clamp(size, AREA);
        // 240 pt over 0.875 pt per pixel.
        let c = v.crop(size, AREA, 240.0);
        assert_eq!(c.side, 274);
        assert_eq!((c.x, c.y), (343, 183));
        v.zoom = 9.0;
        v.clamp(size, AREA);
        assert_eq!(v.zoom, ZOOM[1]);
    }
}

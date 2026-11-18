//! Profile pictures as textures. Each arrives once in a snapshot (base64 JPEG or PNG,
//! checked by the service) and is kept while the latest snapshot points at it: the cache
//! holds exactly the pictures on screen or about to be.
use std::collections::HashMap;

use base64::Engine as _;
use egui::{ColorImage, Context, TextureHandle, TextureOptions};
use zaun_core::chat::overlay::OverlaySnapshot;
use zaun_core::chat::overlay::pictures::referenced;

/// The largest side kept (pictures are drawn at 36 points, 72 pixels at scale 2).
const SIDE: u32 = 192;

#[derive(Default)]
pub struct Pictures {
    textures: HashMap<u64, TextureHandle>,
}

impl Pictures {
    /// Take the new pictures of `snap` and forget those it no longer points at.
    pub fn update(&mut self, ctx: &Context, snap: &OverlaySnapshot) {
        for p in &snap.pictures {
            match decode(&p.data) {
                Some(image) => {
                    let name = format!("picture-{:016x}", p.key);
                    let tex = ctx.load_texture(name, image, TextureOptions::LINEAR);
                    self.textures.insert(p.key, tex);
                }
                None => tracing::warn!(key = p.key, "overlay: a picture did not decode"),
            }
        }
        let keep = referenced(snap);
        self.textures.retain(|k, _| keep.contains(k));
    }

    pub fn get(&self, key: Option<u64>) -> Option<&TextureHandle> {
        key.and_then(|k| self.textures.get(&k))
    }
}

fn decode(data: &str) -> Option<ColorImage> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if img.width() > SIDE || img.height() > SIDE {
        img.thumbnail(SIDE, SIDE)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

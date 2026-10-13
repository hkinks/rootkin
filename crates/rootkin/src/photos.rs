//! Decodes package images into egui textures on demand.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use familyx::Package;

const MAX_SIDE: u32 = 512;

#[derive(Default)]
pub struct PhotoCache {
    textures: HashMap<String, Option<TextureHandle>>,
}

impl PhotoCache {
    pub fn clear(&mut self) {
        self.textures.clear();
    }

    /// Texture for a zip part like `Multimedia/abc.png`; `None` if missing or undecodable.
    pub fn get(&mut self, ctx: &egui::Context, pkg: &Package, part: &str) -> Option<TextureHandle> {
        self.textures
            .entry(part.to_string())
            .or_insert_with(|| {
                let bytes = pkg.part(part)?;
                let img = image::load_from_memory(bytes).ok()?.thumbnail(MAX_SIDE, MAX_SIDE).to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                Some(ctx.load_texture(part, color, TextureOptions::LINEAR))
            })
            .clone()
    }
}

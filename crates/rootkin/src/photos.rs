//! Decodes package images into egui textures on demand, and prepares
//! user-chosen images for storing in a tree.

use std::collections::HashMap;

use eframe::egui::{self, ColorImage, Rect, TextureHandle, TextureOptions};
use familyx::model::{Person, Tree};
use familyx::Package;

const MAX_SIDE: u32 = 512;

struct Entry {
    texture: TextureHandle,
    /// Size of the original image, for mapping face rectangles.
    original: [f32; 2],
}

#[derive(Default)]
pub struct PhotoCache {
    entries: HashMap<String, Option<Entry>>,
}

/// Image types we can read when adding a photo.
pub const PICK_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"];

impl PhotoCache {
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    fn entry(&mut self, ctx: &egui::Context, pkg: &Package, part: &str) -> Option<&Entry> {
        self.entries
            .entry(part.to_string())
            .or_insert_with(|| {
                let img = image::load_from_memory(pkg.part(part)?).ok()?;
                let original = [img.width() as f32, img.height() as f32];
                let img = img.thumbnail(MAX_SIDE, MAX_SIDE).to_rgba8();
                let size = [img.width() as usize, img.height() as usize];
                let color = ColorImage::from_rgba_unmultiplied(size, img.as_raw());
                Some(Entry { texture: ctx.load_texture(part, color, TextureOptions::LINEAR), original })
            })
            .as_ref()
    }

    /// Texture for a zip part like `Multimedia/abc.png`.
    pub fn get(&mut self, ctx: &egui::Context, pkg: &Package, part: &str) -> Option<TextureHandle> {
        self.entry(ctx, pkg, part).map(|e| e.texture.clone())
    }

    /// A person's primary photo with a square UV crop around their face
    /// (or the image centre).
    pub fn face(
        &mut self,
        ctx: &egui::Context,
        pkg: &Package,
        tree: &Tree,
        person: &Person,
    ) -> Option<(egui::TextureId, Rect)> {
        let media = tree.primary_photo(person)?;
        let entry = self.entry(ctx, pkg, &media.part)?;
        let [w, h] = entry.original;
        let (cx, cy, side) = match person.face {
            Some(f) if f.x + f.width <= w + 1.0 && f.y + f.height <= h + 1.0 => {
                (f.x + f.width / 2.0, f.y + f.height / 2.0, f.width.max(f.height))
            }
            _ => (w / 2.0, h / 2.0, w.min(h)),
        };
        let half = side / 2.0;
        let uv = Rect::from_min_max(
            egui::pos2(((cx - half) / w).max(0.0), ((cy - half) / h).max(0.0)),
            egui::pos2(((cx + half) / w).min(1.0), ((cy + half) / h).min(1.0)),
        );
        Some((entry.texture.id(), uv))
    }
}

/// Bytes and extension to store for a picked image file: PNG and JPEG are
/// kept as they are, other formats are converted to PNG (the app reads only
/// those two).
pub fn prepare_for_tree(bytes: Vec<u8>, extension: &str) -> Result<(Vec<u8>, &'static str), String> {
    match extension.to_ascii_lowercase().as_str() {
        "png" => Ok((bytes, "png")),
        "jpg" | "jpeg" => Ok((bytes, "jpg")),
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| format!("can't read the image: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
            Ok((out.into_inner(), "png"))
        }
    }
}

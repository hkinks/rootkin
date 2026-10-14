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
        Some((entry.texture.id(), face_uv(entry.original, person.face)))
    }
}

/// A square UV crop of a `w`×`h` image around the face (or the image centre).
/// The square is shrunk to fit and slid back inside the image, so it is never
/// clipped at an edge: a clipped crop would be stretched in a square box.
fn face_uv([w, h]: [f32; 2], face: Option<familyx::model::Face>) -> Rect {
    let (cx, cy, side) = match face {
        Some(f) if f.x + f.width <= w + 1.0 && f.y + f.height <= h + 1.0 => {
            (f.x + f.width / 2.0, f.y + f.height / 2.0, f.width.max(f.height))
        }
        _ => (w / 2.0, h / 2.0, w.min(h)),
    };
    let side = side.min(w).min(h);
    let x = (cx - side / 2.0).clamp(0.0, w - side);
    let y = (cy - side / 2.0).clamp(0.0, h - side);
    Rect::from_min_max(egui::pos2(x / w, y / h), egui::pos2((x + side) / w, (y + side) / h))
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

#[cfg(test)]
mod tests {
    use super::*;
    use familyx::model::Face;

    /// The crop's size in image pixels.
    fn pixels(uv: Rect, [w, h]: [f32; 2]) -> (f32, f32) {
        (uv.width() * w, uv.height() * h)
    }

    #[test]
    fn face_crops_are_square_and_inside_the_image() {
        let size = [400.0, 300.0];
        let cases = [
            None,
            Some(Face { x: 150.0, y: 100.0, width: 80.0, height: 100.0 }),
            // near the top left corner: used to be clipped and squeezed
            Some(Face { x: 0.0, y: 0.0, width: 60.0, height: 90.0 }),
            // near the bottom right corner
            Some(Face { x: 360.0, y: 260.0, width: 40.0, height: 40.0 }),
            // taller than the image
            Some(Face { x: 100.0, y: 0.0, width: 100.0, height: 300.0 }),
        ];
        for face in cases {
            let uv = face_uv(size, face);
            let (pw, ph) = pixels(uv, size);
            assert!((pw - ph).abs() < 0.01, "{face:?}: {pw}x{ph}");
            assert!(uv.min.x >= 0.0 && uv.min.y >= 0.0 && uv.max.x <= 1.0 && uv.max.y <= 1.0, "{face:?}: {uv:?}");
        }
        // the face stays in view when the square slides
        let uv = face_uv(size, Some(Face { x: 0.0, y: 0.0, width: 60.0, height: 90.0 }));
        assert_eq!(uv.min, egui::pos2(0.0, 0.0));
        assert!((uv.max.y * 300.0 - 90.0).abs() < 0.01);
    }
}

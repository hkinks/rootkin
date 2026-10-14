//! Import and export of genealogy formats.
//!
//! Imports build a fresh [`Package`] through the `familyx` edit API and
//! return an [`ImportReport`] listing what could not be represented.
//! Exports are read-only views of a package's [`familyx::model::Tree`].

mod ansel;
mod archive;
mod dates;
pub mod example;
mod families;
pub mod gedcom;
pub mod gramps;
mod report;
mod vocab;

use std::collections::HashMap;
use std::path::Path;

use familyx::model::Tree;
use familyx::Package;

pub use report::ReportGroup;
pub(crate) use report::{Dropped, Findings};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("familyx: {0}")]
    Familyx(#[from] familyx::Error),
    #[error("edit: {0}")]
    Edit(#[from] familyx::edit::EditError),
    #[error("GEDCOM: {0}")]
    Gedcom(String),
    #[error("Gramps XML: {0}")]
    Gramps(String),
    #[error("cannot tell the file format of {0} (expected .ged, .gramps, .gdz or .zip)")]
    UnknownFormat(String),
    #[error("{0}")]
    Archive(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// GEDCOM 5.5.1, the most widely readable version.
    Gedcom,
    /// GEDCOM 7.0 (import treats both versions alike).
    Gedcom7,
    Gramps,
}

impl Format {
    /// By file extension (`.ged`, `.gedcom`, `.gramps`), case-insensitive.
    pub fn from_extension(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "ged" | "gedcom" => Some(Format::Gedcom),
            "gramps" => Some(Format::Gramps),
            _ => None,
        }
    }

    /// By content: gzip magic or a `<database` root means Gramps, a `0 HEAD`
    /// line (or a UTF-16 byte order mark) means GEDCOM.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        if bytes.starts_with(&[0x1f, 0x8b]) {
            return Some(Format::Gramps);
        }
        if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
            return Some(Format::Gedcom);
        }
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
        let head = head.trim_start_matches('\u{feff}').trim_start();
        if head.starts_with('<') {
            return head.contains("<database").then_some(Format::Gramps);
        }
        head.starts_with("0 HEAD").then_some(Format::Gedcom)
    }

    /// Content first (reliable), then the extension.
    pub fn detect(path: Option<&Path>, bytes: &[u8]) -> Option<Self> {
        Self::sniff(bytes).or_else(|| path.and_then(Self::from_extension))
    }
}

/// What an import created and what it had to leave out.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub people: usize,
    pub families: usize,
    pub sources: usize,
    pub photos: usize,
    /// Everything that was left out or worked around, grouped and explained
    /// in plain words.
    pub left_out: Vec<ReportGroup>,
    /// Pairs of people with the same name and a birth year within 2 years.
    pub duplicates: Vec<String>,
    /// Links to records that are not in the file, one line each.
    pub broken_links: Vec<String>,
    /// The tree's name when the file gives one (Ancestry's `_TREE`).
    pub tree_name: Option<String>,
    /// The technical details: every raw note and dropped item, for support
    /// and "Copy details".
    pub warnings: Vec<String>,
}

/// A photo written next to an export; `path` is relative to the exported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaFile {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A photo referenced by an export.
pub(crate) struct MediaRef {
    /// 1-based number for record ids (`@O1@`, `_o1`).
    pub(crate) number: usize,
    pub(crate) path: String,
    pub(crate) ext: String,
    pub(crate) title: String,
}

/// The photos an export refers to, keyed by media id; empty when exporting
/// without media.
#[derive(Default)]
pub(crate) struct ExportMedia {
    pub(crate) refs: HashMap<String, MediaRef>,
    /// In numbering order.
    pub(crate) order: Vec<String>,
    pub(crate) files: Vec<MediaFile>,
}

impl ExportMedia {
    fn collect(pkg: &Package, tree: &Tree, dir: &str) -> Self {
        let mut media = ExportMedia::default();
        let mut used_names = std::collections::HashSet::new();
        for id in tree.people.iter().flat_map(|p| p.media.iter()) {
            if media.refs.contains_key(id) {
                continue;
            }
            let Some(m) = tree.media.get(id) else { continue };
            let Some(bytes) = pkg.part(&m.part) else { continue };
            let ext = m.part.rsplit_once('.').map_or("jpg", |(_, e)| e).to_ascii_lowercase();
            // readable, unique file names
            let stem: String = std::path::Path::new(&m.file_name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "photo".into())
                .chars()
                .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
                .collect();
            let mut name = format!("{stem}.{ext}");
            let mut n = 2;
            while !used_names.insert(name.clone()) {
                name = format!("{stem}-{n}.{ext}");
                n += 1;
            }
            let path = format!("{dir}/{name}");
            media.order.push(id.clone());
            media.refs.insert(
                id.clone(),
                MediaRef { number: media.order.len(), path: path.clone(), ext, title: m.title.clone() },
            );
            media.files.push(MediaFile { path, bytes: bytes.to_vec() });
        }
        media
    }
}

/// Loads a file referenced by an imported tree (path as written in the file).
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

/// Attach a referenced photo to a person, if it can be found and is PNG/JPEG.
/// An online photo (a web address) cannot be fetched: its address is returned
/// for the caller to keep in the person's note.
pub(crate) fn import_photo(
    pkg: &mut Package,
    person: &str,
    path: &str,
    resolve: Resolve,
    dropped: &mut Dropped,
) -> Option<String> {
    let lower = path.trim().to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        dropped.add("online photo");
        return Some(path.trim().to_string());
    }
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_string();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let ext = match ext.as_str() {
        "png" => "png",
        "jpg" | "jpeg" => "jpg",
        _ => {
            dropped.add("photo of a type other than PNG/JPEG");
            return None;
        }
    };
    match resolve(path) {
        Some(bytes) => {
            if pkg.add_photo(person, bytes, ext, &name).is_err() {
                dropped.add("photo that could not be attached");
            }
        }
        None => dropped.add("photo whose file was not found"),
    }
    None
}

pub fn import(bytes: &[u8], format: Format) -> Result<(Package, ImportReport)> {
    import_with(bytes, format, &|_| None)
}

/// Import a zip file (`.gdz`, or a `.zip` holding a `.ged` / `.gramps`).
pub fn import_zip(bytes: &[u8]) -> Result<(Package, ImportReport)> {
    archive::import(bytes)
}

/// Import, loading referenced photo files through `resolve`.
pub fn import_with(bytes: &[u8], format: Format, resolve: Resolve) -> Result<(Package, ImportReport)> {
    match format {
        Format::Gedcom | Format::Gedcom7 => gedcom::import(bytes, resolve),
        Format::Gramps => gramps::import(bytes, resolve),
    }
}

/// Export without photos.
pub fn export(pkg: &Package, format: Format) -> Result<Vec<u8>> {
    export_inner(pkg, format, &ExportMedia::default())
}

/// Export with photos: the result refers to files under `media_dir`
/// (relative to the exported file), returned for the caller to write.
pub fn export_with_media(pkg: &Package, format: Format, media_dir: &str) -> Result<(Vec<u8>, Vec<MediaFile>)> {
    let media = ExportMedia::collect(pkg, &Tree::from_doc(&pkg.doc), media_dir);
    let bytes = export_inner(pkg, format, &media)?;
    Ok((bytes, media.files))
}

fn export_inner(pkg: &Package, format: Format, media: &ExportMedia) -> Result<Vec<u8>> {
    match format {
        Format::Gedcom => Ok(gedcom::export(pkg, false, media)),
        Format::Gedcom7 => Ok(gedcom::export(pkg, true, media)),
        Format::Gramps => gramps::export(pkg, media),
    }
}

/// Import a file, detecting its format from content and extension. Photos
/// are looked up relative to the file (also by bare file name next to it).
/// A zip (`.gdz`, or `.zip` holding a `.ged` / `.gramps`) is read in place.
pub fn import_path(path: impl AsRef<Path>) -> Result<(Package, ImportReport)> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    if archive::is_zip(&bytes) {
        return archive::import(&bytes);
    }
    let format = Format::detect(Some(path), &bytes).ok_or_else(|| Error::UnknownFormat(path.display().to_string()))?;
    let base = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    let resolve = move |reference: &str| {
        let normalized = reference.replace('\\', "/");
        let file_name = normalized.rsplit('/').next().unwrap_or(&normalized).to_string();
        [base.join(&normalized), Path::new(&normalized).to_path_buf(), base.join(&file_name)]
            .into_iter()
            .find_map(|p| std::fs::read(p).ok())
    };
    import_with(&bytes, format, &resolve)
}

/// Export to a file (format from the extension, GEDCOM 5.5.1 for `.ged`),
/// with photos copied into `<name>_media/` next to it.
pub fn export_path(pkg: &Package, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let format = Format::from_extension(path).ok_or_else(|| Error::UnknownFormat(path.display().to_string()))?;
    export_path_as(pkg, path, format)
}

pub fn export_path_as(pkg: &Package, path: impl AsRef<Path>, format: Format) -> Result<()> {
    let path = path.as_ref();
    let stem = path.file_stem().map_or("export".into(), |s| s.to_string_lossy().into_owned());
    let dir = format!("{stem}_media");
    let (bytes, files) = export_with_media(pkg, format, &dir)?;
    let base = path.parent().unwrap_or(Path::new("."));
    for f in &files {
        let target = base.join(&f.path);
        std::fs::create_dir_all(target.parent().expect("media path has a directory"))?;
        std::fs::write(target, &f.bytes)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

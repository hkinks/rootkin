//! Import and export of genealogy formats.
//!
//! Imports build a fresh [`Package`] through the `familyx` edit API and
//! return an [`ImportReport`] listing what could not be represented.
//! Exports are read-only views of a package's [`familyx::model::Tree`].

mod dates;
mod families;
pub mod gedcom;
pub mod gramps;
mod vocab;

use std::path::Path;

use familyx::Package;

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
    #[error("cannot tell the file format of {0} (expected .ged or .gramps)")]
    UnknownFormat(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Gedcom,
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
    /// Human-readable notes on dropped or approximated data.
    pub warnings: Vec<String>,
}

pub fn import(bytes: &[u8], format: Format) -> Result<(Package, ImportReport)> {
    match format {
        Format::Gedcom => gedcom::import(bytes),
        Format::Gramps => gramps::import(bytes),
    }
}

pub fn export(pkg: &Package, format: Format) -> Result<Vec<u8>> {
    match format {
        Format::Gedcom => Ok(gedcom::export(pkg)),
        Format::Gramps => gramps::export(pkg),
    }
}

/// Import a file, detecting its format from content and extension.
pub fn import_path(path: impl AsRef<Path>) -> Result<(Package, ImportReport)> {
    let path = path.as_ref();
    let bytes = std::fs::read(path)?;
    let format = Format::detect(Some(path), &bytes).ok_or_else(|| Error::UnknownFormat(path.display().to_string()))?;
    import(&bytes, format)
}

/// Export to a file; the format comes from the extension.
pub fn export_path(pkg: &Package, path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref();
    let format = Format::from_extension(path).ok_or_else(|| Error::UnknownFormat(path.display().to_string()))?;
    std::fs::write(path, export(pkg, format)?)?;
    Ok(())
}

/// Counts dropped items by a path like `INDI.SOUR`, reported once at the end.
#[derive(Default)]
pub(crate) struct Dropped(std::collections::BTreeMap<String, usize>);

impl Dropped {
    pub(crate) fn add(&mut self, what: impl Into<String>) {
        self.add_n(what, 1);
    }

    pub(crate) fn add_n(&mut self, what: impl Into<String>, n: usize) {
        *self.0.entry(what.into()).or_default() += n;
    }

    pub(crate) fn into_warnings(self, warnings: &mut Vec<String>) {
        for (what, n) in self.0 {
            warnings.push(format!("dropped {n}x {what}"));
        }
    }
}

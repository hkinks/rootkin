//! The `.familyx` OPC (zip) container.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::codec::{AesVersion, SourceFormat, PART_NAMES};
use crate::doc::{Document, Element};
use crate::migrate::schema14::{self, Downgrade};
use crate::model::Id;
use crate::{codec, edit, migrate, protection, Error, Result};

const FAMILY_PART: &str = "family8";
/// The container My Family Tree 12.6 writes.
const APP12_FORMAT: SourceFormat = SourceFormat::Family7(AesVersion::V2);
const FAMILY_CONTENT_TYPE: &str = "application/vnd.chronoplexsoftware.familyx";

/// An opened `.familyx` file: the decoded tree plus every other part verbatim.
///
/// Older files (tree part `family7`, `family5`, `family4`, `family`, `content.xml`,
/// or `family8` with the V1/V2 encryption) are read transparently, but saving
/// always writes the current format: a `family8` V3 part, with the package
/// metadata (`[Content_Types].xml`, `_rels/.rels`) pointing at it. See
/// [`Package::source_format`]. [`Package::save_for_app12`] writes a copy for My Family Tree 12.x. Trees in the old pre-link schema are converted
/// to the current schema on open (see [`crate::migrate`], [`Package::migration`]).
#[derive(Clone)]
pub struct Package {
    pub doc: Document,
    /// All zip entries other than the tree part, in original order.
    parts: Vec<(String, Vec<u8>)>,
    /// Position of the tree part among the entries, so saving keeps the order.
    family_index: usize,
    source: SourceFormat,
    migration: Option<Box<migrate::Report>>,
}

const CONTENT_TYPES: &str = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
<Default Extension=\"png\" ContentType=\"image/png\" /><Default Extension=\"jpg\" ContentType=\"image/jpeg\" />\
<Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\" />\
<Override PartName=\"/family8\" ContentType=\"application/vnd.chronoplexsoftware.familyx\" /></Types>";
const RELS_PART: &str = "_rels/.rels";
const RELATIONSHIP_TYPE: &str = "MyFamilyTree/Relationship";

impl Package {
    /// A new, empty family tree.
    pub fn new() -> Self {
        let mut pkg = Package {
            doc: edit::new_family_document(),
            parts: vec![("[Content_Types].xml".into(), CONTENT_TYPES.as_bytes().to_vec())],
            family_index: 2,
            source: SourceFormat::CURRENT,
            migration: None,
        };
        let rels = "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>\
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\" />";
        pkg.parts.push((RELS_PART.into(), rels.as_bytes().to_vec()));
        pkg.add_relationship(&format!("/{FAMILY_PART}")).expect("fresh rels part is valid");
        pkg
    }

    /// Store a photo in the package and make it the person's primary image.
    /// `extension` is `png` or `jpg`; returns the media id.
    pub fn add_photo(&mut self, person_id: &str, bytes: Vec<u8>, extension: &str, file_name: &str) -> Result<Id> {
        if !matches!(extension, "png" | "jpg") {
            return Err(Error::Format(format!("unsupported photo type {extension} (png or jpg)")));
        }
        let media_id = edit::new_id();
        let part = format!("Multimedia/{}.{extension}", edit::new_id());
        let windows_path = part.replace('/', "\\");
        let title = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);

        let mut props = edit::record("Properties");
        props.push(edit::text_el("Title", title));
        props.push(Element::new("Rating").with_attr("xsi:nil", "true"));
        props.push(edit::record("Date"));
        props.push(edit::text_el("MultimediaType", "Blank"));
        props.push(Element::new("CustomMultimediaType"));
        props.push(edit::text_el("MediaType", "Blank"));
        let mut file = Element::new("PackagedFile")
            .with_attr("xsi:type", "Photo")
            .with_attr("Id", &media_id)
            .with_attr("LastSaved", &edit::local_now())
            .with_attr("Creation", &edit::utc_now());
        file.push(edit::text_el("OriginalFileName", &windows_path));
        file.push(edit::text_el("RelativePath", &windows_path));
        file.push(edit::text_el("FileName", file_name));
        file.push(edit::text_el("IsTranslation", "false"));
        file.push(edit::text_el("Type", "Photo"));
        file.push(props);

        edit::attach_photo(&mut self.doc, person_id, &media_id, file).map_err(|e| Error::Format(e.to_string()))?;
        self.add_relationship(&format!("/{part}"))?;
        self.parts.push((part, bytes));
        Ok(media_id)
    }

    /// Detach a photo from a person and, if nothing else uses it, delete the
    /// file from the package.
    pub fn remove_photo(&mut self, person_id: &str, media_id: &str) -> Result<()> {
        let unused =
            edit::detach_photo(&mut self.doc, person_id, media_id).map_err(|e| Error::Format(e.to_string()))?;
        if unused {
            if let Some(part) = edit::remove_packaged_file(&mut self.doc, media_id) {
                self.parts.retain(|(n, _)| *n != part);
                if let Some((_, data)) = self.parts.iter_mut().find(|(n, _)| n == RELS_PART) {
                    let mut rels = Document::parse(data)?;
                    let target = format!("/{part}");
                    rels.root.remove_children_where(|r| r.attr("Target").as_deref() == Some(target.as_str()));
                    *data = rels.to_bytes();
                }
            }
        }
        Ok(())
    }

    /// Add everyone and everything of `other` to this tree ("import into this tree"): people,
    /// couples and parent links, sources (existing ones are reused), citations, notes and photo
    /// files, all with new ids. Nothing already here changes, and the home person stays.
    ///
    /// Undo by restoring `self.doc`; photo files copied here then remain in the package
    /// unreferenced, as with a removed photo.
    pub fn append(&mut self, other: &Package) -> Result<crate::AppendReport> {
        let (report, parts) = crate::append::append_document(&mut self.doc, &other.doc);
        for part in parts {
            // a missing file leaves a photo record without a picture, as in the other package
            let Some(bytes) = other.part(&part.old) else { continue };
            self.add_relationship(&format!("/{}", part.new))?;
            self.parts.push((part.new, bytes.to_vec()));
        }
        Ok(report)
    }

    /// Attach a photo already in the package to another person.
    pub fn share_photo(&mut self, person_id: &str, media_id: &str) -> Result<()> {
        edit::link_photo(&mut self.doc, person_id, media_id).map_err(|e| Error::Format(e.to_string()))
    }

    fn add_relationship(&mut self, target: &str) -> Result<()> {
        let (_, data) = self
            .parts
            .iter_mut()
            .find(|(n, _)| n == RELS_PART)
            .ok_or_else(|| Error::Format("package has no _rels/.rels".into()))?;
        let mut rels = Document::parse(data)?;
        let mut id = [0u8; 8];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut id);
        let id: String = id.iter().map(|b| format!("{b:02x}")).collect();
        rels.root.push(
            Element::new("Relationship")
                .with_attr("Type", RELATIONSHIP_TYPE)
                .with_attr("Target", target)
                .with_attr("Id", &format!("R{id}")),
        );
        *data = rels.to_bytes();
        Ok(())
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(&std::fs::read(path)?)
    }

    /// The format this package was read from (`SourceFormat::CURRENT` for new
    /// packages). Saving always writes `SourceFormat::CURRENT`.
    pub fn source_format(&self) -> SourceFormat {
        self.source
    }

    /// True if the tree carries a user password (`FamilyId` is not the default
    /// record). The data is readable regardless; the Windows app will prompt.
    pub fn is_password_protected(&self) -> bool {
        protection::document_is_protected(&self.doc)
    }

    /// True if the tree still contains elements of the old schema that
    /// [`migrate`](crate::migrate) could not convert (a database-backed `Story` whose database is unreadable
    /// or a story file missing from the package). They are kept verbatim.
    pub fn has_legacy_schema(&self) -> bool {
        migrate::has_legacy_elements(&self.doc)
    }

    /// What schema migration did when this package was opened; `None` if the
    /// tree already used the current schema.
    pub fn migration(&self) -> Option<&migrate::Report> {
        self.migration.as_deref()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut entries = Vec::with_capacity(zip.len());
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut data)?;
            entries.push((entry.name().to_string(), data));
        }
        // The newest tree part present wins; any others are carried along verbatim.
        let family_index =
            PART_NAMES.iter().find_map(|part| entries.iter().position(|(name, _)| name == part)).ok_or_else(|| {
                Error::Format(
                    "no tree part (expected family8, family7, family5, family4, family or content.xml)".into(),
                )
            })?;
        let (name, blob) = entries.remove(family_index);
        let (xml, source) = codec::decode_part(&name, &blob)?;
        let mut doc = Document::parse(&xml)?;
        if !source.is_current() {
            // Old trees may lack the password record or carry the pre-210000 notation;
            // make it what the current app writes (it does the same on conversion).
            if let Some(family_id) = protection::upgrade_legacy_family_id(doc.root.attr("FamilyId").as_deref()) {
                doc.root.set_attr("FamilyId", &family_id);
            }
        }
        // Old schema (per-person relationships, photos, ...) -> current schema.
        let part = |name: &str| entries.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, d)| d.clone());
        let migration = migrate::migrate(&mut doc, &part).map(Box::new);
        let consumed = migration.as_ref().map(|m| m.consumed_parts.clone()).unwrap_or_default();
        let mut pkg = Package { doc, parts: entries, family_index, source, migration };
        for name in &consumed {
            pkg.drop_part(name)?;
        }
        if source.part_name() != FAMILY_PART {
            pkg.retarget_tree_part(source.part_name(), FAMILY_PART)?;
        }
        Ok(pkg)
    }

    /// Remove a part (matched case-insensitively) that migration absorbed into the tree, with its
    /// content-type override and relationships.
    fn drop_part(&mut self, name: &str) -> Result<()> {
        let Some(pos) = self.parts.iter().position(|(n, _)| n.eq_ignore_ascii_case(name)) else { return Ok(()) };
        let (removed, _) = self.parts.remove(pos);
        if pos < self.family_index {
            self.family_index -= 1;
        }
        let target = format!("/{removed}");
        for part in ["[Content_Types].xml", RELS_PART] {
            if let Some((_, data)) = self.parts.iter_mut().find(|(n, _)| n == part) {
                let mut doc = Document::parse(data)?;
                let before = doc.root.children.len();
                doc.root.remove_children_where(|e| {
                    e.attr("PartName").as_deref() == Some(target.as_str())
                        || e.attr("Target").as_deref() == Some(target.as_str())
                });
                if doc.root.children.len() != before {
                    *data = doc.to_bytes();
                }
            }
        }
        Ok(())
    }

    /// Point `[Content_Types].xml` and `_rels/.rels` at the tree part `new` instead of `old`,
    /// so the package describes the part that saving writes.
    fn retarget_tree_part(&mut self, old: &str, new: &str) -> Result<()> {
        let (old_path, new_path) = (format!("/{old}"), format!("/{new}"));
        if let Some((_, data)) = self.parts.iter_mut().find(|(n, _)| n == "[Content_Types].xml") {
            let mut types = Document::parse(data)?;
            let mut found = false;
            for el in types.root.elements_mut().filter(|e| e.name == "Override") {
                if el.attr("PartName").as_deref() == Some(&old_path)
                    || el.attr("PartName").as_deref() == Some(&new_path)
                {
                    el.set_attr("PartName", &new_path);
                    found = true;
                }
            }
            if !found {
                types.root.push(
                    Element::new("Override")
                        .with_attr("PartName", &new_path)
                        .with_attr("ContentType", FAMILY_CONTENT_TYPE),
                );
            }
            *data = types.to_bytes();
        }
        if let Some((_, data)) = self.parts.iter_mut().find(|(n, _)| n == RELS_PART) {
            let mut rels = Document::parse(data)?;
            let mut found = false;
            for el in rels.root.elements_mut().filter(|e| e.name == "Relationship") {
                if el.attr("Target").as_deref() == Some(&old_path) || el.attr("Target").as_deref() == Some(&new_path) {
                    el.set_attr("Target", &new_path);
                    found = true;
                }
            }
            *data = rels.to_bytes();
            if !found {
                self.add_relationship(&new_path)?;
            }
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        self.write_zip(SourceFormat::CURRENT)
    }

    fn write_zip(&self, format: SourceFormat) -> Result<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let family_blob = codec::encode_part(&self.doc.to_bytes(), format);
        let mut family = Some(family_blob);
        for (i, (name, data)) in self.parts.iter().enumerate() {
            if i == self.family_index {
                write_entry(&mut zip, format.part_name(), family.take().as_deref().unwrap(), options)?;
            }
            write_entry(&mut zip, name, data, options)?;
        }
        if let Some(blob) = family {
            write_entry(&mut zip, format.part_name(), &blob, options)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    /// Save atomically. An existing file is first copied to `<path>.bak`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        write_file(path.as_ref(), &self.to_bytes()?)
    }

    /// A copy for My Family Tree 12.x, which cannot read the current format: the tree in the shape of
    /// schema 14.4 (what 12.6 writes) in a `family7` part with the V2 encryption. The package itself is
    /// unchanged. Returns what the copy could not carry over.
    pub fn to_bytes_for_app12(&self) -> Result<(Vec<u8>, Downgrade)> {
        let mut copy = self.clone();
        let has_part = |name: &str| self.parts.iter().any(|(n, _)| n == name);
        let downgrade = schema14::downgrade(&mut copy.doc, &has_part);
        copy.retarget_tree_part(FAMILY_PART, APP12_FORMAT.part_name())?;
        Ok((copy.write_zip(APP12_FORMAT)?, downgrade))
    }

    /// Save [`Package::to_bytes_for_app12`] atomically, keeping an existing file as `<path>.bak`.
    pub fn save_for_app12(&self, path: impl AsRef<Path>) -> Result<Downgrade> {
        let (bytes, downgrade) = self.to_bytes_for_app12()?;
        write_file(path.as_ref(), &bytes)?;
        Ok(downgrade)
    }

    /// Raw bytes of a non-family part, e.g. `Multimedia/abc.png`.
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, d)| d.as_slice())
    }

    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|(n, _)| n.as_str())
    }
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if path.exists() {
        std::fs::copy(path, backup_path(path))?;
    }
    let tmp = path.with_extension("familyx.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

fn write_entry(
    zip: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    data: &[u8],
    options: zip::write::SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, options)?;
    zip.write_all(data)?;
    Ok(())
}

impl Default for Package {
    fn default() -> Self {
        Self::new()
    }
}

pub fn backup_path(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    s.into()
}

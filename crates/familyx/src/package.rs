//! The `.familyx` OPC (zip) container.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::codec::{SourceFormat, PART_NAMES};
use crate::doc::{Document, Element};
use crate::model::Id;
use crate::{codec, edit, protection, Error, Result};

const FAMILY_PART: &str = "family8";
const FAMILY_CONTENT_TYPE: &str = "application/vnd.chronoplexsoftware.familyx";

/// An opened `.familyx` file: the decoded tree plus every other part verbatim.
///
/// Older files (tree part `family7`, `family5`, `family4`, `family`, `content.xml`,
/// or `family8` with the V1/V2 encryption) are read transparently, but saving
/// always writes the current format: a `family8` V3 part, with the package
/// metadata (`[Content_Types].xml`, `_rels/.rels`) pointing at it. See
/// [`Package::source_format`].
#[derive(Clone)]
pub struct Package {
    pub doc: Document,
    /// All zip entries other than the tree part, in original order.
    parts: Vec<(String, Vec<u8>)>,
    /// Position of the tree part among the entries, so saving keeps the order.
    family_index: usize,
    source: SourceFormat,
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

    /// True if the tree still uses elements of the pre-link schema (per-person
    /// `Relationships`, `Photos`, `Story`, root-level `PhotoCollection`, ...).
    /// The app converts those on load; this crate neither shows nor migrates them
    /// (they are preserved verbatim), so such a tree can look incomplete.
    pub fn has_legacy_schema(&self) -> bool {
        const ROOT: [&str; 5] = [
            "Author",
            "PhotoCollection",
            "AttachmentCollection",
            "AudioVideoCollection",
            "PackagedFilePropertiesCollection",
        ];
        const PERSON: [&str; 9] = [
            "Relationships",
            "Photos",
            "Attachments",
            "AudioVideo",
            "LDSFacts",
            "Story",
            "Name",
            "AlternativeName",
            "ContactAddress",
        ];
        self.doc.root.elements().any(|e| ROOT.contains(&e.name.as_str()))
            || self
                .doc
                .root
                .child("PeopleCollection")
                .is_some_and(|c| c.elements().any(|p| p.elements().any(|e| PERSON.contains(&e.name.as_str()))))
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
        let mut pkg = Package { doc, parts: entries, family_index, source };
        if source.part_name() != FAMILY_PART {
            pkg.retarget_tree_part(source.part_name())?;
        }
        Ok(pkg)
    }

    /// Point `[Content_Types].xml` and `_rels/.rels` at `family8` instead of `old`,
    /// so the package describes the part that saving writes.
    fn retarget_tree_part(&mut self, old: &str) -> Result<()> {
        let (old_path, new_path) = (format!("/{old}"), format!("/{FAMILY_PART}"));
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
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let family_blob = codec::encode(&self.doc.to_bytes());
        let mut family = Some(family_blob);
        for (i, (name, data)) in self.parts.iter().enumerate() {
            if i == self.family_index {
                write_entry(&mut zip, FAMILY_PART, family.take().as_deref().unwrap(), options)?;
            }
            write_entry(&mut zip, name, data, options)?;
        }
        if let Some(blob) = family {
            write_entry(&mut zip, FAMILY_PART, &blob, options)?;
        }
        Ok(zip.finish()?.into_inner())
    }

    /// Save atomically. An existing file is first copied to `<path>.bak`.
    pub fn save(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let bytes = self.to_bytes()?;
        if path.exists() {
            std::fs::copy(path, backup_path(path))?;
        }
        let tmp = path.with_extension("familyx.tmp");
        std::fs::write(&tmp, &bytes)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Raw bytes of a non-family part, e.g. `Multimedia/abc.png`.
    pub fn part(&self, name: &str) -> Option<&[u8]> {
        self.parts.iter().find(|(n, _)| n == name).map(|(_, d)| d.as_slice())
    }

    pub fn part_names(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().map(|(n, _)| n.as_str())
    }
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

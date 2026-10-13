//! The `.familyx` OPC (zip) container.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::doc::{Document, Element};
use crate::model::Id;
use crate::{codec, edit, Error, Result};

const FAMILY_PART: &str = "family8";

/// An opened `.familyx` file: the decoded tree plus every other part verbatim.
pub struct Package {
    pub doc: Document,
    /// All zip entries other than `family8`, in original order.
    parts: Vec<(String, Vec<u8>)>,
    /// Position of `family8` among the entries, so saving keeps the order.
    family_index: usize,
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

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut parts = Vec::with_capacity(zip.len());
        let mut family = None;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut data)?;
            if entry.name() == FAMILY_PART {
                family = Some((i, data));
            } else {
                parts.push((entry.name().to_string(), data));
            }
        }
        let (family_index, blob) = family
            .ok_or_else(|| Error::Format("no family8 part (older .familyx versions are not supported)".into()))?;
        let doc = Document::parse(&codec::decode(&blob)?)?;
        Ok(Package { doc, parts, family_index })
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

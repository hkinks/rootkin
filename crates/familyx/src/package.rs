//! The `.familyx` OPC (zip) container.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use crate::doc::Document;
use crate::{codec, Error, Result};

const FAMILY_PART: &str = "family8";

/// An opened `.familyx` file: the decoded tree plus every other part verbatim.
pub struct Package {
    pub doc: Document,
    /// All zip entries other than `family8`, in original order.
    parts: Vec<(String, Vec<u8>)>,
    /// Position of `family8` among the entries, so saving keeps the order.
    family_index: usize,
}

impl Package {
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
        let (family_index, blob) = family.ok_or_else(|| {
            Error::Format("no family8 part (older .familyx versions are not supported)".into())
        })?;
        let doc = Document::parse(&codec::decode(&blob)?)?;
        Ok(Package { doc, parts, family_index })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
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

pub fn backup_path(path: &Path) -> std::path::PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    s.into()
}

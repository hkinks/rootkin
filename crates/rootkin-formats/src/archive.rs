//! Zipped imports: GEDCOM 7 archives (`.gdz`, `gedcom.ged` plus media files)
//! and plain `.zip` files that hold one `.ged` or `.gramps`.

use std::cell::RefCell;
use std::io::{Cursor, Read};

use familyx::Package;
use zip::ZipArchive;

use crate::{Error, Format, ImportReport, Result};

/// Largest single file read out of an archive (guards against zip bombs).
const MAX_ENTRY: u64 = 1 << 30;

/// Whether the bytes are a zip file (also what a `.familyx` package is).
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// Folders and macOS resource-fork leftovers are not files of interest.
fn is_junk(name: &str) -> bool {
    name.ends_with('/') || name.starts_with("__MACOSX/") || name.rsplit('/').next().is_some_and(|f| f.starts_with("._"))
}

fn normalize(reference: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let reference = reference.replace('\\', "/");
    for part in reference.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn read_entry(zip: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let mut entry = zip.by_name(name).ok()?;
    let mut data = Vec::new();
    entry.by_ref().take(MAX_ENTRY).read_to_end(&mut data).ok()?;
    Some(data)
}

/// Import the family tree file inside a zip, resolving photos from the
/// archive. The first `.ged` / `.gramps` is used (for a `.gdz`, `gedcom.ged`
/// at the root wins); the report says so when there were several.
pub fn import(bytes: &[u8]) -> Result<(Package, ImportReport)> {
    let archive_error = |e: zip::result::ZipError| Error::Archive(format!("cannot read the zip file ({e})"));
    let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(archive_error)?;
    let names: Vec<String> = zip.file_names().map(str::to_string).filter(|n| !is_junk(n)).collect();
    let is_tree = |n: &String| {
        let lower = n.to_ascii_lowercase();
        [".ged", ".gedcom", ".gramps"].iter().any(|ext| lower.ends_with(ext))
    };
    let mut trees: Vec<&String> = names.iter().filter(|n| is_tree(n)).collect();
    // the standard name first, then files nearest the top
    trees.sort_by_key(|n| (!n.eq_ignore_ascii_case("gedcom.ged"), n.matches('/').count()));
    let Some(chosen) = trees.first().map(|n| n.to_string()) else {
        return Err(Error::Archive("the zip file has no .ged or .gramps family tree file inside".into()));
    };
    let data =
        read_entry(&mut zip, &chosen).ok_or_else(|| Error::Archive(format!("cannot read {chosen} in the zip file")))?;
    let format = Format::detect(Some(chosen.as_ref()), &data).ok_or_else(|| Error::UnknownFormat(chosen.clone()))?;

    // media paths are relative to the tree file's folder, or the archive root
    let dir = chosen.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
    let zip = RefCell::new(zip);
    let lookup = |reference: &str| {
        let wanted = normalize(reference);
        let file_name = wanted.rsplit('/').next().unwrap_or(&wanted).to_string();
        let beside = normalize(&format!("{dir}/{wanted}"));
        let by_name = names.iter().find(|n| n.rsplit('/').next() == Some(file_name.as_str()));
        let found = [Some(beside.as_str()), Some(wanted.as_str()), by_name.map(String::as_str)]
            .into_iter()
            .flatten()
            .find_map(|n| read_entry(&mut zip.borrow_mut(), n));
        found
    };
    let (pkg, mut report) = crate::import_with(&data, format, &lookup)?;
    if trees.len() > 1 {
        report
            .warnings
            .insert(0, format!("the zip file holds {} family trees; only {chosen} was imported", trees.len()));
    }
    Ok((pkg, report))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let mut w = zip::ZipWriter::new(&mut out);
        for (name, data) in files {
            w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
        out.into_inner()
    }

    const GED: &str = "0 HEAD\n0 @I1@ INDI\n1 NAME Anna /Kask/\n1 OBJE\n2 FILE media/anna.jpg\n0 TRLR\n";

    #[test]
    fn normalizes_references() {
        assert_eq!(normalize(".\\media\\a.jpg"), "media/a.jpg");
        assert_eq!(normalize("a/../b.jpg"), "b.jpg");
    }

    #[test]
    fn gdz_with_media_in_a_folder() {
        let photo = b"\x89PNG fake image bytes".to_vec();
        let zip = zip_of(&[("gedcom.ged", GED.as_bytes()), ("media/anna.jpg", &photo[..])]);
        let (pkg, report) = import(&zip).unwrap();
        assert_eq!(report.people, 1);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let tree = familyx::model::Tree::from_doc(&pkg.doc);
        assert_eq!(tree.people[0].media.len(), 1);
        let media = tree.media.values().next().unwrap();
        assert_eq!(pkg.part(&media.part), Some(&photo[..]));
    }

    #[test]
    fn several_trees_warn_and_pick_the_first() {
        let zip = zip_of(&[
            ("__MACOSX/._x.ged", b"junk"),
            ("old/b.ged", b"0 HEAD\n0 @I1@ INDI\n1 NAME B /B/\n0 @I2@ INDI\n1 NAME C /C/\n0 TRLR\n"),
            ("a.ged", b"0 HEAD\n0 @I1@ INDI\n1 NAME A /A/\n0 TRLR\n"),
        ]);
        let (_, report) = import(&zip).unwrap();
        assert_eq!(report.people, 1);
        assert!(report.warnings[0].contains("a.ged was imported"), "{:?}", report.warnings);
    }

    #[test]
    fn zip_without_a_tree_is_an_error() {
        let zip = zip_of(&[("notes.txt", b"hello")]);
        let err = import(&zip).err().unwrap().to_string();
        assert!(err.contains("no .ged or .gramps"), "{err}");
    }
}

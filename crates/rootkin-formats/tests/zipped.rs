//! `.gdz` and `.zip` imports through the file path API.

use std::io::Write;

use familyx::model::Tree;
use familyx::Package;
use rootkin_formats::{export_with_media, import_path, Format};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../familyx/tests/data/example.familyx");

fn write_zip(path: &std::path::Path, files: &[(String, Vec<u8>)]) {
    let mut w = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    for (name, data) in files {
        w.start_file(name.as_str(), zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
}

fn photos(pkg: &Package) -> usize {
    Tree::from_doc(&pkg.doc).people.iter().map(|p| p.media.len()).sum()
}

#[test]
fn gdz_archive_with_photos_roundtrips() {
    let original = Package::open(EXAMPLE).unwrap();
    let (ged, media) = export_with_media(&original, Format::Gedcom7, "media").unwrap();
    let mut files = vec![("gedcom.ged".to_string(), ged)];
    files.extend(media.into_iter().map(|m| (m.path, m.bytes)));
    let dir = std::env::temp_dir().join(format!("rootkin-gdz-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("family.gdz");
    write_zip(&path, &files);

    let (pkg, report) = import_path(&path).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.people, Tree::from_doc(&original.doc).people.len());
    assert!(photos(&original) >= 3);
    assert_eq!(photos(&pkg), photos(&original));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn zip_holding_a_ged_and_gramps_file() {
    let original = Package::open(EXAMPLE).unwrap();
    let ged = rootkin_formats::export(&original, Format::Gedcom).unwrap();
    let gramps = rootkin_formats::export(&original, Format::Gramps).unwrap();
    let dir = std::env::temp_dir().join(format!("rootkin-zip-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("download.zip");
    write_zip(&path, &[("Tree/export.ged".into(), ged), ("tree.gramps".into(), gramps)]);
    let (_, report) = import_path(&path).unwrap();
    assert!(report.warnings[0].contains("2 family trees"), "{:?}", report.warnings);
    std::fs::remove_dir_all(&dir).unwrap();
}

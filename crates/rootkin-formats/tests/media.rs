//! Photos travel with GEDCOM and Gramps exports as files next to the export.

use familyx::model::Tree;
use familyx::Package;
use rootkin_formats::{export_path_as, import_path, Format};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../familyx/tests/data/example.familyx");

fn photo_owners(pkg: &Package) -> Vec<(String, usize)> {
    let tree = Tree::from_doc(&pkg.doc);
    let mut v: Vec<(String, usize)> =
        tree.people.iter().filter(|p| !p.media.is_empty()).map(|p| (p.display_name(), p.media.len())).collect();
    v.sort();
    v
}

fn roundtrip(format: Format, file: &str) {
    let pkg = Package::open(EXAMPLE).unwrap();
    let dir = std::env::temp_dir().join(format!("rootkin-media-{}-{file}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join(file);
    export_path_as(&pkg, &out, format).unwrap();
    let stem = out.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(std::fs::read_dir(dir.join(format!("{stem}_media"))).unwrap().count(), 3);

    let (back, report) = import_path(&out).unwrap();
    assert!(!report.warnings.iter().any(|w| w.contains("photo")), "{:?}", report.warnings);
    assert_eq!(photo_owners(&back), photo_owners(&pkg));
    let tree = Tree::from_doc(&back.doc);
    for p in &tree.people {
        if let Some(m) = tree.primary_photo(p) {
            assert!(back.part(&m.part).is_some_and(|b| b.starts_with(b"\x89PNG")));
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn photos_roundtrip_through_gedcom() {
    roundtrip(Format::Gedcom, "tree.ged");
}

#[test]
fn photos_roundtrip_through_gedcom7() {
    roundtrip(Format::Gedcom7, "tree7.ged");
}

#[test]
fn photos_roundtrip_through_gramps() {
    roundtrip(Format::Gramps, "tree.gramps");
}

#[test]
fn gedcom7_header_and_values() {
    let pkg = Package::open(EXAMPLE).unwrap();
    let text = String::from_utf8(rootkin_formats::export(&pkg, Format::Gedcom7).unwrap()).unwrap();
    assert!(text.contains("2 VERS 7.0"));
    assert!(!text.contains("1 CHAR"));
    assert!(!text.contains(" CONC "));
    assert!(text.contains("2 PEDI ADOPTED"));
}

//! Tests against the real (photo-obfuscated) example file in the repo root.

use familyx::{codec, model::Tree, Package};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../db.obfuscated.familyx");

/// The fixture holds personal data and is not in git; tests skip without it.
macro_rules! require_fixture {
    () => {
        if !std::path::Path::new(FIXTURE).exists() {
            eprintln!("skipped: {FIXTURE} not present");
            return;
        }
    };
}

fn fixture_family8() -> Vec<u8> {
    let bytes = std::fs::read(FIXTURE).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut zip.by_name("family8").unwrap(), &mut out).unwrap();
    out
}

#[test]
fn unmodified_document_serializes_byte_identical() {
    require_fixture!();
    let xml = codec::decode(&fixture_family8()).unwrap();
    let doc = familyx::doc::Document::parse(&xml).unwrap();
    assert!(doc.to_bytes() == xml, "XML round-trip is not byte-exact");
}

#[test]
fn package_save_and_reopen_preserves_everything() {
    require_fixture!();
    let pkg = Package::open(FIXTURE).unwrap();
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert!(reopened.doc == pkg.doc);
    assert!(pkg.part_names().eq(reopened.part_names()));
    for name in pkg.part_names() {
        assert_eq!(pkg.part(name), reopened.part(name), "{name}");
    }
}

#[test]
fn tree_reads_people_and_links() {
    require_fixture!();
    let pkg = Package::open(FIXTURE).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    assert_eq!(tree.people.len(), 115);
    assert_eq!(tree.spouse_links.len(), 45);
    assert_eq!(tree.parent_links.len(), 126);
    assert_eq!(tree.media.len(), 48);

    let home = tree.home_person.as_deref().expect("home person");
    let person = tree.person(home).expect("home person exists");
    assert!(!person.display_name().is_empty());
    for m in tree.media.values() {
        assert!(pkg.part(&m.part).is_some(), "missing media part {}", m.part);
    }
    for l in &tree.parent_links {
        assert!(tree.person(&l.parent).is_some() && tree.person(&l.child).is_some());
    }
}

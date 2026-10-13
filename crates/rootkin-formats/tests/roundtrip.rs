//! The fictional example tree survives GEDCOM and Gramps round trips.

use std::collections::BTreeSet;

use familyx::model::{Date, Fact, Tree};
use familyx::Package;
use rootkin_formats::{export, import, Format};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../familyx/tests/data/example.familyx");

fn facts(facts: &[Fact]) -> BTreeSet<(String, String, String, String)> {
    facts.iter().map(|f| (f.tag.clone(), format!("{:?}", f.date), f.place.clone(), f.details.clone())).collect()
}

fn names(tree: &Tree, id: &str) -> Vec<(String, String, String, String)> {
    let p = tree.person(id).unwrap();
    p.names
        .iter()
        .map(|n| (n.given.clone(), n.surname.clone(), n.name_type.clone(), n.is_primary.to_string()))
        .collect()
}

/// Everything the formats promise to carry, keyed by display name (unique in the example).
fn summary(tree: &Tree) -> BTreeSet<String> {
    let name = |id: &str| tree.person(id).unwrap().display_name();
    let mut out = BTreeSet::new();
    for p in &tree.people {
        out.insert(format!("person {} {:?} {:?}", name(&p.id), p.gender, names(tree, &p.id)));
        out.insert(format!("facts {} {:?}", name(&p.id), facts(&p.facts)));
    }
    for l in &tree.spouse_links {
        let mut pair = [name(&l.person1), name(&l.person2)];
        pair.sort();
        out.insert(format!("couple {pair:?} {:?}", facts(&l.facts)));
    }
    for l in &tree.parent_links {
        out.insert(format!("parent {} -> {} [{}]", name(&l.parent), name(&l.child), l.modifier));
    }
    out
}

fn roundtrip(format: Format) {
    let original = Package::open(EXAMPLE).unwrap();
    let before = Tree::from_doc(&original.doc);
    let distinct: BTreeSet<_> = before.people.iter().map(|p| p.display_name()).collect();
    assert_eq!(distinct.len(), before.people.len(), "example names must be unique for this test");

    let bytes = export(&original, format).unwrap();
    let (pkg, report) = import(&bytes, format).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(report.people, before.people.len());
    assert!(familyx::edit::check(&pkg.doc).is_empty());

    let after = Tree::from_doc(&pkg.doc);
    assert_eq!(after.people.len(), before.people.len());
    assert_eq!(after.spouse_links.len(), before.spouse_links.len());
    assert_eq!(after.parent_links.len(), before.parent_links.len());
    let (a, b) = (summary(&before), summary(&after));
    assert_eq!(a.difference(&b).collect::<Vec<_>>(), Vec::<&String>::new(), "lost in {format:?} round trip");
    assert_eq!(b.difference(&a).collect::<Vec<_>>(), Vec::<&String>::new(), "added in {format:?} round trip");
    assert!(after.parent_links.iter().any(|l| l.modifier == "Adopted"));
    assert!(after.spouse_links.iter().any(|l| l.facts.iter().any(|f| f.tag == "MARR" && f.date != Date::default())));
}

#[test]
fn gedcom_roundtrip_preserves_the_example_tree() {
    roundtrip(Format::Gedcom);
}

#[test]
fn gramps_roundtrip_preserves_the_example_tree() {
    roundtrip(Format::Gramps);
}

#[test]
fn format_detection() {
    let pkg = Package::open(EXAMPLE).unwrap();
    let ged = export(&pkg, Format::Gedcom).unwrap();
    let gramps = export(&pkg, Format::Gramps).unwrap();
    assert_eq!(Format::sniff(&ged), Some(Format::Gedcom));
    assert_eq!(Format::sniff(&gramps), Some(Format::Gramps));
    let mut gz = flate2_decode(&gramps);
    assert_eq!(Format::sniff(&gz), Some(Format::Gramps));
    // uncompressed XML imports too
    gz.insert(0, b' ');
    let (_, report) = import(&gz, Format::Gramps).unwrap();
    assert_eq!(report.people, Tree::from_doc(&pkg.doc).people.len());
    assert_eq!(Format::from_extension("x.GED".as_ref()), Some(Format::Gedcom));
    assert_eq!(Format::from_extension("x.gramps".as_ref()), Some(Format::Gramps));
    assert_eq!(Format::from_extension("x.txt".as_ref()), None);
}

fn flate2_decode(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(bytes), &mut out).unwrap();
    out
}

#[test]
fn gedcom_export_structure() {
    let pkg = Package::open(EXAMPLE).unwrap();
    let text = String::from_utf8(export(&pkg, Format::Gedcom).unwrap()).unwrap();
    for needle in ["0 HEAD", "1 SOUR ROOTKIN", "1 CHAR UTF-8", "2 VERS 5.5.1", "2 FORM LINEAGE-LINKED", "0 TRLR"] {
        assert!(text.contains(needle), "missing {needle}");
    }
    assert!(text.contains("1 NAME Arthur /Lindqvist/"));
    assert!(text.contains("2 DATE 14 MAR 1890"));
    assert!(text.contains("2 PEDI adopted"));
    assert!(text.lines().all(|l| l.len() <= 255));
}

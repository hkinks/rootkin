//! A hand-written, fictional GEDCOM with ranges, custom tags, CONC/CONT,
//! a mother-only family and notes.

use familyx::model::{Gender, Person, Tree};
use rootkin_formats::{import, Format, ImportReport};

const FIXTURE: &str = include_str!("data/handwritten.ged");

fn run(bytes: &[u8]) -> (Tree, ImportReport) {
    let (pkg, report) = import(bytes, Format::Gedcom).unwrap();
    assert!(familyx::edit::check(&pkg.doc).is_empty());
    (Tree::from_doc(&pkg.doc), report)
}

fn by_name<'a>(tree: &'a Tree, name: &str) -> &'a Person {
    tree.people.iter().find(|p| p.display_name() == name).unwrap_or_else(|| panic!("no {name}"))
}

fn check(tree: &Tree, report: &ImportReport) {
    assert_eq!((report.people, report.families), (4, 2));
    assert_eq!(tree.people.len(), 4);

    let oskar = by_name(tree, "Oskar Valdmann");
    assert_eq!(oskar.gender, Gender::Male);
    let birth = oskar.fact("BIRT").unwrap();
    assert_eq!((birth.date.descriptor.as_str(), birth.date.year), ("Before", Some(1850)));
    assert_eq!(birth.place, "Kuressaare, Saaremaa");
    let death = oskar.fact("DEAT").unwrap();
    assert_eq!(death.date.descriptor, "Range");
    assert_eq!((death.date.year, death.date.end_year), (Some(1900), Some(1905)));

    // CONC joins without a separator, CONT with a newline
    let occu = &oskar.fact("OCCU").unwrap().details;
    assert!(occu.starts_with("Cooper and cask maker"));
    assert!(occu.len() > 200 && !occu.contains("  "), "{occu}");
    assert!(occu.ends_with("\nsecond paragraph"));

    let liisa = by_name(tree, "Liisa Tamm");
    assert_eq!(liisa.gender, Gender::Female);
    let birth = liisa.fact("BIRT").unwrap();
    assert_eq!((birth.date.descriptor.as_str(), birth.date.year, birth.date.month), ("About", Some(1855), Some(3)));
    let resi = liisa.fact("RESI").unwrap();
    assert_eq!((resi.date.descriptor.as_str(), resi.date.end_year), ("Span", Some(1890)));
    assert_eq!(resi.place, "Haapsalu");
    assert_eq!(resi.details, "The family lived above the harbour\nand moved in spring.");

    // F1: a married couple with one child
    let mari = by_name(tree, "Mari Valdmann");
    let mut parents = tree.parents(&mari.id);
    parents.sort();
    let mut expected = vec![oskar.id.as_str(), liisa.id.as_str()];
    expected.sort();
    assert_eq!(parents, expected);
    let couple = tree.spouse_links_of(&oskar.id);
    assert_eq!(couple.len(), 1);
    assert_eq!(couple[0].facts[0].tag, "MARR");
    assert_eq!(couple[0].facts[0].date.year, Some(1878));

    // F2: only a mother, adopted child, marriage fact dropped (no spouse)
    let jaan = by_name(tree, "Jaan");
    assert_eq!(tree.parents(&jaan.id), vec![mari.id.as_str()]);
    let link = tree.parent_links.iter().find(|l| l.child == jaan.id).unwrap();
    assert_eq!(link.modifier, "Adopted");
    assert!(tree.spouse_links_of(&mari.id).is_empty());
    assert_eq!(tree.parent_links.iter().find(|l| l.child == mari.id).unwrap().modifier, "");

    let has = |needle: &str| report.warnings.iter().any(|w| w.contains(needle));
    assert!(has("INDI._CUSTOM"), "{:?}", report.warnings);
    assert!(has("INDI.NOTE"));
    assert!(has("photo"), "the OBJE file does not exist or is not PNG/JPEG");
    assert!(has("INDI.BIRT.SOUR"));
    assert!(has("FAM._MARR_LICENCE"));
    assert!(has("FAM.MARR without two spouses"));
    assert!(has("SOUR record"));
    assert!(has("INDI.NAME._MARNM"));
}

#[test]
fn imports_handwritten_gedcom() {
    let (tree, report) = run(FIXTURE.as_bytes());
    check(&tree, &report);
}

#[test]
fn tolerates_crlf_bom_and_utf16() {
    let crlf = FIXTURE.replace('\n', "\r\n");
    let (tree, report) = run(crlf.as_bytes());
    check(&tree, &report);

    let mut bom = vec![0xef, 0xbb, 0xbf];
    bom.extend(crlf.as_bytes());
    let (tree, report) = run(&bom);
    check(&tree, &report);

    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend(FIXTURE.encode_utf16().flat_map(|u| u.to_le_bytes()));
    let (tree, report) = run(&utf16);
    check(&tree, &report);
}

#[test]
fn rejects_non_gedcom() {
    assert!(import(b"hello world", Format::Gedcom).is_err());
}

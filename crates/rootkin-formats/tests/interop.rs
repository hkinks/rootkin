//! Regressions found by round-tripping through Gramps and by validating the
//! exports (see docs/interop.md). Fictional data only.

use familyx::model::Tree;
use rootkin_formats::{export, import, Format};

fn gramps_text(pkg: &familyx::Package) -> String {
    use std::io::Read;
    let gz = export(pkg, Format::Gramps).unwrap();
    let mut xml = String::new();
    flate2::read::GzDecoder::new(&gz[..]).read_to_string(&mut xml).unwrap();
    xml
}

fn tree_of(bytes: &[u8], format: Format) -> Tree {
    Tree::from_doc(&import(bytes, format).unwrap().0.doc)
}

#[test]
fn gramps_date_quality_survives_a_round_trip() {
    let ged = b"0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE EST 1901\n1 DEAT\n2 DATE CAL 3 MAR 1950\n0 TRLR\n";
    let (pkg, _) = import(ged, Format::Gedcom).unwrap();
    let xml = gramps_text(&pkg);
    // `type` only takes before / after / about; the DTD has `quality` for these
    assert!(xml.contains(r#"<dateval val="1901" quality="estimated"/>"#), "{xml}");
    assert!(xml.contains(r#"<dateval val="1950-03-03" quality="calculated"/>"#), "{xml}");
    let tree = tree_of(xml.as_bytes(), Format::Gramps);
    let p = &tree.people[0];
    assert_eq!(p.fact("BIRT").unwrap().date.descriptor, "Estimated");
    assert_eq!(p.fact("DEAT").unwrap().date.descriptor, "Calculated");
}

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

#[test]
fn custom_event_types_keep_their_name_through_gramps() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<database xmlns="http://gramps-project.org/xml/1.7.1/">
  <events>
    <event handle="_e1" id="E1"><type>Moved to the coast</type><dateval val="1950"/><description>for the air</description></event>
    <event handle="_e2" id="E2"><type>Event</type><description>plain</description></event>
  </events>
  <people><person handle="_a" id="I1"><gender>F</gender><name><first>Ada</first><surname>Test</surname></name>
    <eventref hlink="_e1" role="Primary"/><eventref hlink="_e2" role="Primary"/></person></people>
</database>"#;
    let tree = tree_of(xml.as_bytes(), Format::Gramps);
    let facts = &tree.people[0].facts;
    let moved = facts.iter().find(|f| f.custom_tag == "Moved to the coast").expect("named event");
    assert_eq!((moved.tag.as_str(), moved.details.as_str(), moved.date.year), ("EVEN", "for the air", Some(1950)));
    assert!(facts.iter().any(|f| f.tag == "EVEN" && f.custom_tag.is_empty() && f.details == "plain"));

    // and back out: the name is the Gramps event type again
    let (pkg, _) = import(xml.as_bytes(), Format::Gramps).unwrap();
    let out = gramps_text(&pkg);
    assert!(out.contains("<type>Moved to the coast</type>"), "{out}");
    assert!(out.contains("<type>Event</type>"), "{out}");
}

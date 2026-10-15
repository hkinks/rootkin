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

#[test]
fn name_title_suffix_and_nickname_survive_gedcom_and_gramps() {
    let ged = concat!(
        "0 HEAD\n0 @I1@ INDI\n1 NAME Dr. Karl Johan /Lindqvist/ Jr.\n2 NPFX Dr.\n2 GIVN Karl Johan\n2 NICK Kalle\n",
        "1 NAME Karl /Berg/ III\n2 TYPE aka\n0 TRLR\n"
    );
    let check = |tree: &Tree| {
        let names = &tree.people[0].names;
        let primary = names.iter().find(|n| n.is_primary).unwrap();
        assert_eq!(
            (primary.given.as_str(), primary.title.as_str(), primary.suffix.as_str(), primary.nickname.as_str()),
            ("Karl Johan", "Dr.", "Jr.", "Kalle")
        );
        let other = names.iter().find(|n| !n.is_primary).unwrap();
        assert_eq!((other.surname.as_str(), other.suffix.as_str()), ("Berg", "III"));
    };
    let (pkg, report) = import(ged.as_bytes(), Format::Gedcom).unwrap();
    check(&Tree::from_doc(&pkg.doc));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);

    let again = export(&pkg, Format::Gedcom).unwrap();
    let text = String::from_utf8(again.clone()).unwrap();
    assert!(
        text.contains("1 NAME Dr. Karl Johan /Lindqvist/ Jr.\n2 TYPE")
            || text.contains("1 NAME Dr. Karl Johan /Lindqvist/ Jr.\n2 GIVN"),
        "{text}"
    );
    assert!(text.contains("2 NPFX Dr."), "{text}");
    check(&tree_of(&again, Format::Gedcom));

    let via_gramps = gramps_text(&pkg);
    assert!(via_gramps.contains("<title>Dr.</title>"), "{via_gramps}");
    check(&tree_of(via_gramps.as_bytes(), Format::Gramps));
}

const FICTIONAL_V7_SOURCE: &str = concat!(
    "0 HEAD\n0 @I1@ INDI\n1 NAME Ida /Test/\n",
    "1 NAME Ida /Maiden/\n2 TYPE maiden\n",
    "1 NAME Idy /Test/\n2 TYPE Adopted\n",
    "1 NOTE write to ida@@example.test\n",
    "1 BIRT\n2 DATE (some spring day)\n",
    "0 TRLR\n"
);

#[test]
fn gedcom7_export_uses_only_what_the_spec_allows() {
    let (pkg, _) = import(FICTIONAL_V7_SOURCE.as_bytes(), Format::Gedcom).unwrap();
    let text = String::from_utf8(export(&pkg, Format::Gedcom7).unwrap()).unwrap();
    // name types are the upper-case enumeration, other words go in a PHRASE
    assert!(text.contains("2 TYPE MAIDEN\n"), "{text}");
    assert!(text.contains("2 TYPE OTHER\n3 PHRASE Adopted\n"), "{text}");
    // free-text dates are an empty DATE with a PHRASE
    assert!(text.contains("2 DATE\n3 PHRASE some spring day\n"), "{text}");
    // only a leading @ is doubled in 7.0
    assert!(text.contains("1 NOTE write to ida@example.test\n"), "{text}");
    // 5.5.1 still doubles and keeps the parenthesised date and lower-case type
    let old = String::from_utf8(export(&pkg, Format::Gedcom).unwrap()).unwrap();
    assert!(old.contains("2 TYPE maiden\n") && old.contains("2 DATE (some spring day)\n"), "{old}");
    assert!(old.contains("1 NOTE write to ida@@example.test\n"), "{old}");
}

#[test]
fn gedcom7_name_types_and_date_phrases_come_back_in() {
    let (pkg, _) = import(FICTIONAL_V7_SOURCE.as_bytes(), Format::Gedcom).unwrap();
    let v7 = export(&pkg, Format::Gedcom7).unwrap();
    let tree = tree_of(&v7, Format::Gedcom);
    let p = &tree.people[0];
    let types: Vec<&str> = p.names.iter().filter(|n| !n.is_primary).map(|n| n.name_type.as_str()).collect();
    assert_eq!(types, ["Maiden", "Adopted"]);
    assert_eq!(p.fact("BIRT").unwrap().date.text, "some spring day");
}

#[test]
fn person_notes_survive_gramps_both_ways() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<database xmlns="http://gramps-project.org/xml/1.7.1/">
  <people><person handle="_a" id="I1"><gender>M</gender><name><first>Ola</first><surname>Test</surname></name>
    <noteref hlink="_n1"/><noteref hlink="_n2"/></person></people>
  <notes>
    <note handle="_n1" id="N1" type="Person Note"><text>Farmer on the island.</text></note>
    <note handle="_n2" id="N2" type="Research"><text>Check the 1910 census.</text></note>
  </notes>
</database>"#;
    let (pkg, report) = import(xml.as_bytes(), Format::Gramps).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let note = tree.people[0].notes.iter().map(|n| n.plain_text()).collect::<Vec<_>>().join("|");
    assert_eq!(note, "Farmer on the island.\n\nCheck the 1910 census.");
    assert!(report.warnings.iter().all(|w| !w.contains("notes")), "{:?}", report.warnings);

    let out = gramps_text(&pkg);
    assert!(out.contains(r#"type="Person Note""#) && out.contains("Farmer on the island."), "{out}");
    let again = tree_of(out.as_bytes(), Format::Gramps);
    assert_eq!(again.people[0].notes[0].plain_text(), note.replace('|', "\n"));
}

const TWO_COUPLES: &str = concat!(
    "0 HEAD\n",
    "0 @I1@ INDI\n1 NAME Ann /Test/\n1 SEX F\n1 FAMS @F1@\n",
    "0 @I2@ INDI\n1 NAME Bob /Test/\n1 SEX M\n1 FAMS @F1@\n",
    "0 @I3@ INDI\n1 NAME Cy /Test/\n1 SEX M\n1 FAMS @F2@\n",
    "0 @I4@ INDI\n1 NAME Di /Test/\n1 SEX F\n1 FAMS @F2@\n",
    "0 @F1@ FAM\n1 HUSB @I2@\n1 WIFE @I1@\n1 MARR\n2 TYPE Civil marriage\n2 DATE 1950\n",
    "0 @F2@ FAM\n1 HUSB @I3@\n1 WIFE @I4@\n1 EVEN\n2 TYPE Living together\n",
    "0 TRLR\n"
);

fn couple_tags(tree: &Tree) -> Vec<String> {
    tree.spouse_links.iter().flat_map(|l| l.facts.iter().map(|f| f.tag.clone())).collect()
}

#[test]
fn relationship_types_of_couples_survive_gedcom() {
    let (pkg, _) = import(TWO_COUPLES.as_bytes(), Format::Gedcom).unwrap();
    assert_eq!(couple_tags(&Tree::from_doc(&pkg.doc)), ["MARR_Civil", "MARR_LivingTogether"]);
    for format in [Format::Gedcom, Format::Gedcom7] {
        let out = export(&pkg, format).unwrap();
        let text = String::from_utf8(out.clone()).unwrap();
        // the civil marriage is still a MARR for other programs
        assert!(text.contains("1 MARR\n2 TYPE Civil marriage\n2 DATE 1950"), "{text}");
        assert_eq!(couple_tags(&tree_of(&out, Format::Gedcom)), ["MARR_Civil", "MARR_LivingTogether"]);
    }
}

#[test]
fn relationship_types_map_onto_gramps_family_types() {
    let (pkg, _) = import(TWO_COUPLES.as_bytes(), Format::Gedcom).unwrap();
    let xml = gramps_text(&pkg);
    assert!(xml.contains(r#"<rel type="Civil Union"/>"#) && xml.contains(r#"<rel type="Unmarried"/>"#), "{xml}");
    assert_eq!(couple_tags(&tree_of(xml.as_bytes(), Format::Gramps)), ["MARR_Civil", "MARR_LivingTogether"]);
}

#[test]
fn a_married_gramps_family_without_a_marriage_event_stays_married() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<database xmlns="http://gramps-project.org/xml/1.7.1/">
  <people>
    <person handle="_a" id="I1"><gender>M</gender><name><first>Al</first><surname>Test</surname></name><parentin hlink="_f"/></person>
    <person handle="_b" id="I2"><gender>F</gender><name><first>Bea</first><surname>Test</surname></name><parentin hlink="_f"/></person>
  </people>
  <families><family handle="_f" id="F1"><rel type="Married"/><father hlink="_a"/><mother hlink="_b"/></family></families>
</database>"#;
    let tree = tree_of(xml.as_bytes(), Format::Gramps);
    assert_eq!(couple_tags(&tree), ["MARR"]);
    let (pkg, _) = import(xml.as_bytes(), Format::Gramps).unwrap();
    assert!(gramps_text(&pkg).contains(r#"<rel type="Married"/>"#));
}

#[test]
fn addresses_and_interpreted_date_words_are_kept() {
    let ged = concat!(
        "0 HEAD\n0 @I1@ INDI\n1 NAME Ida /Test/\n",
        "1 RESI\n2 ADDR Storgatan 5\n3 CITY Uppsala\n3 CTRY Sweden\n2 DATE ABT 1950\n",
        "1 BIRT\n2 DATE INT 1930 (about spring)\n",
        "0 TRLR\n"
    );
    let (pkg, report) = import(ged.as_bytes(), Format::Gedcom).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let p = &tree.people[0];
    assert_eq!(p.fact("RESI").unwrap().details, "Address: Storgatan 5, Uppsala, Sweden");
    let birth = p.fact("BIRT").unwrap();
    assert_eq!((birth.date.descriptor.as_str(), birth.date.year), ("About", Some(1930)));
    assert_eq!(birth.details, "Date as written: about spring");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

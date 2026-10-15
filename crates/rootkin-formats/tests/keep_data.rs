//! GEDCOM details people care about: married names, shared notes, date
//! phrases, named events, online photos and placeholder names.

use familyx::model::{Person, Tree};
use rootkin_formats::{export, import, Format, ImportReport};

fn run(text: &str) -> (Tree, ImportReport) {
    let (pkg, report) = import(text.as_bytes(), Format::Gedcom).unwrap();
    assert!(familyx::edit::check(&pkg.doc).is_empty());
    (Tree::from_doc(&pkg.doc), report)
}

fn person<'a>(tree: &'a Tree, name: &str) -> &'a Person {
    tree.people.iter().find(|p| p.display_name() == name).unwrap_or_else(|| panic!("no {name}"))
}

fn note_of(p: &Person) -> String {
    p.notes.iter().map(|n| n.plain_text()).collect::<Vec<_>>().join("\n")
}

#[test]
fn married_name_becomes_an_alternate_name() {
    let (tree, report) = run("0 HEAD\n0 @I1@ INDI\n1 NAME Liisa /Tamm/\n2 _MARNM Valdmann\n1 SEX F\n0 TRLR\n");
    let p = person(&tree, "Liisa Tamm");
    let married: Vec<_> = p.names.iter().filter(|n| n.name_type == "Married").collect();
    assert_eq!(married.len(), 1);
    assert_eq!((married[0].given.as_str(), married[0].surname.as_str()), ("Liisa", "Valdmann"));
    assert!(!married[0].is_primary);
    assert!(!report.warnings.iter().any(|w| w.contains("_MARNM")), "{:?}", report.warnings);
}

#[test]
fn married_name_in_slashes_and_on_an_alternate_name() {
    let (tree, _) = run("0 HEAD\n0 @I1@ INDI\n1 NAME Anna /Kask/\n2 _MARNM Anna /Mägi/\n0 TRLR\n");
    let p = person(&tree, "Anna Kask");
    assert!(p.names.iter().any(|n| n.name_type == "Married" && n.surname == "Mägi"));
}

#[test]
fn married_names_roundtrip_through_gedcom() {
    let (pkg, _) =
        import(b"0 HEAD\n0 @I1@ INDI\n1 NAME Liisa /Tamm/\n2 _MARNM Valdmann\n0 TRLR\n", Format::Gedcom).unwrap();
    for format in [Format::Gedcom, Format::Gedcom7] {
        let text = String::from_utf8(export(&pkg, format).unwrap()).unwrap();
        let ty = if format == Format::Gedcom7 { "MARRIED" } else { "married" };
        assert!(text.contains(&format!("1 NAME Liisa /Valdmann/\n2 TYPE {ty}")), "{text}");
        let (back, report) = import(text.as_bytes(), format).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let tree = Tree::from_doc(&back.doc);
        let p = person(&tree, "Liisa Tamm");
        assert_eq!(p.names.len(), 2);
        assert!(p.names.iter().any(|n| n.name_type == "Married" && n.surname == "Valdmann"));
    }
}

#[test]
fn shared_notes_become_person_and_event_notes() {
    let (tree, report) = run(concat!(
        "0 HEAD\n1 GEDC\n2 VERS 7.0\n",
        "0 @N1@ SNOTE Lived on the island all her life.\n",
        "0 @N2@ SNOTE Born during a storm.\n",
        "0 @I1@ INDI\n1 NAME Liisa /Tamm/\n1 SNOTE @N1@\n",
        "1 BIRT\n2 DATE 1 MAR 1900\n2 SNOTE @N2@\n",
        "0 TRLR\n"
    ));
    let p = person(&tree, "Liisa Tamm");
    assert_eq!(note_of(p), "Lived on the island all her life.");
    assert_eq!(p.fact("BIRT").unwrap().details, "Born during a storm.");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn plain_person_notes_are_kept_and_exported() {
    let (pkg, report) = import(
        b"0 HEAD\n0 @N1@ NOTE Shared text\n0 @I1@ INDI\n1 NAME A /B/\n1 NOTE First\n1 NOTE @N1@\n0 TRLR\n",
        Format::Gedcom,
    )
    .unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let tree = Tree::from_doc(&pkg.doc);
    assert_eq!(note_of(person(&tree, "A B")), "First\n\nShared text");
    let text = String::from_utf8(export(&pkg, Format::Gedcom).unwrap()).unwrap();
    assert!(text.contains("1 NOTE First\n2 CONT\n2 CONT Shared text"), "{text}");
}

#[test]
fn date_phrase_fills_an_empty_or_unreadable_date() {
    let (tree, report) = run(concat!(
        "0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n",
        "1 BIRT\n2 DATE\n3 PHRASE sometime in the spring of 1901\n",
        "1 DEAT\n2 DATE garbled words\n3 PHRASE the winter after the war\n",
        "0 TRLR\n"
    ));
    let p = person(&tree, "A B");
    let birth = &p.fact("BIRT").unwrap().date;
    assert_eq!((birth.descriptor.as_str(), birth.text.as_str()), ("TextOnly", "sometime in the spring of 1901"));
    let death = &p.fact("DEAT").unwrap().date;
    assert_eq!(death.text, "the winter after the war");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn date_phrase_is_added_to_the_note_when_the_date_parses() {
    let (tree, _) =
        run("0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n1 BIRT\n2 DATE 12 MAR 1901\n3 PHRASE about lunchtime\n0 TRLR\n");
    let birth = person(&tree, "A B").fact("BIRT").unwrap().clone();
    assert_eq!((birth.date.year, birth.date.month, birth.date.day), (Some(1901), Some(3), Some(12)));
    assert_eq!(birth.details, "Date as written: about lunchtime");
}

#[test]
fn typed_events_become_named_events() {
    let (tree, report) = run(concat!(
        "0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n",
        "1 EVEN\n2 TYPE Arrival in Tallinn\n2 DATE 1920\n2 NOTE By ship\n",
        "1 FACT Blue eyes\n2 TYPE Eye colour\n",
        "1 EVEN\n2 NOTE no type at all\n",
        "0 TRLR\n"
    ));
    let p = person(&tree, "A B");
    let arrival = p.facts.iter().find(|f| f.custom_tag == "Arrival in Tallinn").expect("custom event");
    assert_eq!((arrival.tag.as_str(), arrival.details.as_str(), arrival.date.year), ("EVEN", "By ship", Some(1920)));
    let eyes = p.facts.iter().find(|f| f.custom_tag == "Eye colour").expect("custom fact");
    assert_eq!(eyes.details, "Blue eyes");
    // an untyped EVEN stays a plain event
    assert!(p.facts.iter().any(|f| f.tag == "EVEN" && f.custom_tag.is_empty() && f.details == "no type at all"));
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[test]
fn named_events_roundtrip_through_gedcom() {
    let (pkg, _) =
        import(b"0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n1 EVEN\n2 TYPE Arrival\n2 NOTE By ship\n0 TRLR\n", Format::Gedcom)
            .unwrap();
    let text = String::from_utf8(export(&pkg, Format::Gedcom).unwrap()).unwrap();
    assert_eq!(text.matches("2 TYPE Arrival").count(), 1, "{text}");
    let (back, _) = import(text.as_bytes(), Format::Gedcom).unwrap();
    let tree = Tree::from_doc(&back.doc);
    let fact = person(&tree, "A B").facts.iter().find(|f| f.custom_tag == "Arrival").unwrap().clone();
    assert_eq!(fact.details, "By ship");
}

#[test]
fn online_photos_are_kept_in_the_note() {
    let (tree, report) = run(concat!(
        "0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n",
        "1 OBJE\n2 FILE https://example.com/photos/a.jpg\n3 FORM jpg\n",
        "0 TRLR\n"
    ));
    assert_eq!(note_of(person(&tree, "A B")), "Online photo: https://example.com/photos/a.jpg");
    assert!(!report.warnings.iter().any(|w| w.contains("PNG/JPEG")), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("online photo")), "{:?}", report.warnings);
}

#[test]
fn living_placeholder_has_no_name_and_is_living() {
    let (tree, _) = run("0 HEAD\n0 @I1@ INDI\n1 NAME Living /Kask/\n1 BIRT\n2 DATE 1850\n0 @I2@ INDI\n1 NAME Livingston /Kask/\n0 TRLR\n");
    let living =
        tree.people.iter().find(|p| p.primary_name().is_some_and(|n| n.given.is_empty())).expect("placeholder");
    assert!(living.is_living);
    assert_eq!(living.display_name(), "Kask");
    assert!(tree.people.iter().any(|p| p.display_name() == "Livingston Kask"));
}

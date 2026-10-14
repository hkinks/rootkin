//! Schema migration of old-format trees. The fixtures (`tests/data/old/legacy_*.xml`, fictional) are
//! hand-written after the app's pre-link classes (per-person `Relationships`, `Name`, `Photos`, `Story`,
//! old `CustomDate` members, LDS facts, witnesses, obsolete `Biology` members, ...); their encrypted forms
//! were produced by `make_vectors.py`.
//!
//! The expected results follow the app's conversion chain (docs/design.md, "Schema migration"). Most of them
//! were checked against a differential run of the app's own chain on small synthetic inputs; the places where
//! this crate deliberately keeps data the app would lose are called out in the tests.

use familyx::doc::{Document, Element};
use familyx::model::{Gender, Tree};
use familyx::{codec, edit, migrate, Package};
use std::io::{Cursor, Write};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/old");

fn zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in parts {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// A package whose tree part is `blob`, stored under the old part name `part`.
fn wrap(blob: &[u8], part: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let types = format!(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Override PartName=\"/{part}\" ContentType=\"application/vnd.chronoplexsoftware.familyx\" /></Types>"
    );
    let rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Type=\"MyFamilyTree/Relationship\" Target=\"/{part}\" Id=\"R1\" /></Relationships>"
    );
    let mut parts: Vec<(&str, &[u8])> =
        vec![("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", rels.as_bytes()), (part, blob)];
    parts.extend_from_slice(extra);
    zip(&parts)
}

/// A package whose tree part is the fixture `file`, stored under the old part name `part`.
fn package(file: &str, part: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    wrap(&std::fs::read(format!("{DATA}/{file}")).unwrap(), part, extra)
}

/// A `family4` package with this tree XML.
fn inline(xml: &str) -> Package {
    let blob = codec::encode_part(format!("\u{feff}{xml}").as_bytes(), codec::SourceFormat::Family4);
    Package::from_bytes(&wrap(&blob, "family4", &[])).unwrap()
}

fn named<'a>(tree: &'a Tree, display: &str) -> &'a familyx::model::Person {
    tree.people.iter().find(|p| p.display_name() == display).unwrap_or_else(|| panic!("no {display}"))
}

fn assert_save_roundtrip(pkg: &Package) -> Package {
    let saved = pkg.to_bytes().unwrap();
    let reopened = Package::from_bytes(&saved).unwrap();
    assert_eq!(reopened.source_format(), codec::SourceFormat::CURRENT);
    assert!(reopened.migration().is_none(), "a migrated tree must not need migrating again");
    assert!(reopened.doc == pkg.doc, "saved and reopened document differs");
    let (a, b) = (Tree::from_doc(&pkg.doc), Tree::from_doc(&reopened.doc));
    assert_eq!(a.people, b.people);
    assert_eq!(a.spouse_links, b.spouse_links);
    assert_eq!(a.parent_links, b.parent_links);
    reopened
}

// ---- reading the converted XML

fn person_el<'a>(doc: &'a Document, id: &str) -> &'a Element {
    doc.root
        .child("PeopleCollection")
        .unwrap()
        .children_named("Person")
        .find(|p| p.attr("Id").as_deref() == Some(id))
        .unwrap_or_else(|| panic!("no person {id}"))
}

/// The `n`-th fact with this tag.
fn fact_el<'a>(person: &'a Element, tag: &str, n: usize) -> &'a Element {
    person
        .child("Facts")
        .into_iter()
        .flat_map(|f| f.children_named("Fact"))
        .filter(|f| f.child_text("Tag").as_deref() == Some(tag))
        .nth(n)
        .unwrap_or_else(|| panic!("no fact {tag} #{n}"))
}

fn fact_tags(person: &Element) -> Vec<String> {
    person
        .child("Facts")
        .into_iter()
        .flat_map(|f| f.children_named("Fact"))
        .filter_map(|f| f.child_text("Tag"))
        .collect()
}

/// `(PersonId, PersonName, Role, CustomRole)` of a fact's associations, missing parts empty.
fn associations(fact: &Element) -> Vec<(String, String, String, String)> {
    let text = |a: &Element, n: &str| a.child_text(n).unwrap_or_default();
    fact.child("Associations")
        .into_iter()
        .flat_map(|l| l.children_named("Association"))
        .map(|a| (text(a, "PersonId"), text(a, "PersonName"), text(a, "Role"), text(a, "CustomRole")))
        .collect()
}

/// No old element is left anywhere in the tree.
fn assert_converted(pkg: &Package) {
    assert!(!pkg.has_legacy_schema(), "old elements are left");
    assert!(edit::check(&pkg.doc).is_empty(), "{:?}", edit::check(&pkg.doc));
    let old = [
        "Relationships",
        "LDSFacts",
        "FactType",
        "Witnesses",
        "HairColor",
        "LeftEyeColor",
        "RightEyeColor",
        "BloodGroup",
        "HaplogroupY",
        "HaplogroupM",
        "FamilyCrest",
        "Event",
        "RoleInEvent",
        "StartDate",
        "IsDateRange",
        "Addresses",
        "Line1",
    ];
    fn walk(el: &Element, old: &[&str]) {
        assert!(!old.contains(&el.name.as_str()), "old element {} left", el.name);
        for c in el.elements() {
            walk(c, old);
        }
    }
    walk(&pkg.doc.root, &old);
}

// ---- the relationship era (version 10.0)

#[test]
fn relationship_era_tree_gets_links() {
    for (file, part) in [("legacy_rel.family5", "family5"), ("legacy_rel.family7_v2", "family7")] {
        let extra: [(&str, &[u8]); 2] = [("Multimedia/harold.png", b"\x89PNG"), ("Multimedia/crest.png", b"crest")];
        let pkg = Package::from_bytes(&package(file, part, &extra)).unwrap();
        let report = pkg.migration().expect("legacy tree is migrated");
        assert_eq!(report.from_version, "10.0", "{file}");
        assert_eq!((report.spouse_links, report.parent_links), (2, 4), "{file}");
        assert_eq!(pkg.doc.root.child_text("FileProperties/Version").as_deref(), Some("14.8"));

        let tree = Tree::from_doc(&pkg.doc);
        assert_eq!(tree.people.len(), 6, "{file}");
        let id = |n: &str| named(&tree, n).id.clone();
        let (harold, edith, walter, joan, mary) =
            (id("Harold Ashby"), id("Edith Ashby"), id("Walter Ashby"), id("Joan Ashby"), id("Mary Okafor"));

        // couples: first-listed person is person 1; the missing reciprocal on Mary's side is irrelevant
        assert_eq!(tree.spouse_links.len(), 2);
        let hl = tree.spouse_links.iter().find(|l| l.person1 == harold && l.person2 == edith).unwrap();
        assert_eq!(hl.status, "Current");
        assert_eq!(hl.facts.len(), 1, "the empty Divorce placeholder is not a fact");
        assert_eq!((hl.facts[0].tag.as_str(), hl.facts[0].date.year), ("MARR", Some(1925)));
        assert_eq!(hl.facts[0].place, "York, England");
        let wl = tree.spouse_links.iter().find(|l| l.person1 == walter && l.person2 == mary).unwrap();
        assert_eq!(wl.status, "Former", "SpouseModifier Former");
        let tags: Vec<&str> = wl.facts.iter().map(|f| f.tag.as_str()).collect();
        assert_eq!(tags, ["MARR", "DIV", "NCHI"]);

        // parents: Walter's came from his Parent relations, Joan's from hers, Harold's Child entry is the same link.
        // Tom, Walter's godparent, is no parent (below).
        let mut parents: Vec<(String, String, String)> =
            tree.parent_links.iter().map(|l| (l.parent.clone(), l.child.clone(), l.modifier.clone())).collect();
        parents.sort();
        let mut expected = vec![
            (harold.clone(), walter.clone(), String::new()),
            (edith.clone(), walter.clone(), String::new()),
            (harold.clone(), joan.clone(), String::new()),
            (edith.clone(), joan.clone(), "Adopted".to_string()),
        ];
        expected.sort();
        assert_eq!(parents, expected, "{file}");
        assert!(tree.parents(&walter).len() == 2 && tree.spouses(&mary) == [walter.as_str()]);

        // facts: old Place and StartDate/Descriptor
        let birth = named(&tree, "Harold Ashby").fact("BIRT").unwrap();
        assert_eq!((birth.date.year, birth.date.month, birth.date.day), (Some(1901), Some(5), Some(3)));
        assert_eq!(birth.place, "Leeds, England");
        let birth = named(&tree, "Edith Ashby").fact("BIRT").unwrap();
        assert_eq!((birth.date.year, birth.date.month, birth.date.day), (Some(1905), Some(3), Some(2)));
        assert_eq!(birth.date.descriptor, "About");
        assert_eq!(birth.place, "York, England");
        assert_eq!(named(&tree, "Joan Ashby").gender, Gender::Female);
        assert!(named(&tree, "Joan Ashby").is_living);

        // Nothing is left in the old shape. Joan's relation to a person that does not exist is dropped, as
        // the app drops it.
        assert_converted(&pkg);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_eq!(report.dropped.len(), 1, "{:?}", report.dropped);

        // the redundant records the app needs
        let doc_person = |pid: &str| person_el(&pkg.doc, pid);
        let couple = &hl.id;
        for child in [&walter, &joan] {
            let links = doc_person(child).child("FamilyChildLinks").expect("FamilyChildLinks");
            assert!(links
                .children_named("FamilyChildLink")
                .any(|l| l.attr("FamilyId").as_deref() == Some(couple.as_str())));
        }
        for spouse in [&harold, &edith] {
            let links = doc_person(spouse).child("FamilySpouseLinks").expect("FamilySpouseLinks");
            assert!(links
                .children_named("FamilySpouseLink")
                .any(|l| l.attr("FamilyId").as_deref() == Some(couple.as_str())));
        }

        // photos that were already PackagedFiles are untouched; the crest is one more
        assert_eq!(tree.media.len(), 2);
        assert_eq!(pkg.part("Multimedia/harold.png"), Some(&b"\x89PNG"[..]));

        let reopened = assert_save_roundtrip(&pkg);
        assert!(edit::check(&reopened.doc).is_empty());
    }
}

#[test]
fn relationship_era_roles_biology_witnesses_and_crest() {
    let extra: [(&str, &[u8]); 2] = [("Multimedia/harold.png", b"\x89PNG"), ("Multimedia/crest.png", b"crest")];
    let pkg = Package::from_bytes(&package("legacy_rel.family7_v2", "family7", &extra)).unwrap();
    let report = pkg.migration().unwrap();
    assert_eq!((report.associations, report.facts), (3, 3));
    let tree = Tree::from_doc(&pkg.doc);
    let id = |n: &str| named(&tree, n).id.clone();
    let (harold, edith, walter, tom) =
        (id("Harold Ashby"), id("Edith Ashby"), id("Walter Ashby"), id("Tom Brightwell"));

    // Tom is Walter's godparent: an association on Walter's birth fact, not a parent link
    let walter_el = person_el(&pkg.doc, &walter);
    let birth = fact_el(walter_el, "BIRT", 0);
    assert_eq!(associations(birth), [(tom.clone(), String::new(), "GodParent".to_string(), String::new())]);
    assert!(tree.parent_links.iter().all(|l| l.parent != tom));
    assert!(birth.child("Associations").is_some() && birth.elements().next().unwrap().name == "Associations");

    // obsolete Biology members become facts (not for "Unknown"), after the existing facts; haplogroups DNA entries
    let harold_el = person_el(&pkg.doc, &harold);
    assert_eq!(
        fact_tags(harold_el),
        ["BIRT", "DEAT", "RESI", "FACT_BloodGroup", "FACT_LeftEyeColor", "FACT_HairColor"]
    );
    let details = |tag: &str| fact_el(harold_el, tag, 0).child_text("Details").unwrap();
    assert_eq!((details("FACT_BloodGroup").as_str(), details("FACT_LeftEyeColor").as_str()), ("A+", "blue"));
    assert_eq!(details("FACT_HairColor"), "grey");
    let biology = harold_el.child("Biology").unwrap();
    let names: Vec<&str> = biology.elements().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["Gender", "YDNA"], "the unknown mitochondrial haplogroup is no entry");
    assert_eq!(biology.path("YDNA/YDNA/Haplogroup").unwrap().text(), "R1b");
    assert!(biology.path("YDNA/YDNA/STRs").is_some() && biology.path("YDNA/YDNA/SNPs").is_some());

    // FactType Family -> IsFamily (file version 10 is past the 7.0 the app needs for that)
    let resi = fact_el(harold_el, "RESI", 0);
    assert_eq!(resi.child_text("IsFamily").as_deref(), Some("true"));
    assert!(resi.child("FactType").is_none());

    // witnesses -> associations of the same fact; an unknown type is a custom role
    let harold_birth = fact_el(harold_el, "BIRT", 0);
    assert_eq!(
        associations(harold_birth),
        [
            (edith.clone(), "Edith".to_string(), "Sponsor".to_string(), String::new()),
            (String::new(), String::new(), "Custom".to_string(), "Neighbour of the family".to_string()),
        ]
    );
    assert!(harold_birth.child("Witnesses").is_none());
    assert_eq!(harold_birth.child_text("Associations/Association/Role").as_deref(), Some("Sponsor"));

    // the family crest: a photo attached to the first person of every separate tree (Tom has no relations)
    let crest = tree.media.values().find(|m| m.file_name == "crest.png").expect("crest media");
    assert_eq!(pkg.part(&crest.part), Some(&b"crest"[..]));
    let with_crest: Vec<String> = tree
        .people
        .iter()
        .filter(|p| person_el(&pkg.doc, &p.id).child_text("FamilyCrestId").as_deref() == Some(crest.id.as_str()))
        .map(|p| p.display_name())
        .collect();
    assert_eq!(with_crest, ["Harold Ashby", "Tom Brightwell"]);
    for name in &with_crest {
        assert!(named(&tree, name).media.contains(&crest.id), "{name}");
    }
    assert!(pkg.doc.root.path("FamilyProperties/FamilyCrest").is_none());
    assert_eq!(report.media, 1, "only the crest was converted");

    assert_converted(&pkg);
    assert_save_roundtrip(&pkg);
}

#[test]
fn a_crest_missing_from_the_package_is_dropped() {
    let pkg = Package::from_bytes(&package("legacy_rel.family5", "family5", &[])).unwrap();
    assert!(pkg.migration().unwrap().dropped.iter().any(|d| d.contains("crest")));
    assert!(pkg.doc.root.path("FamilyProperties/FamilyCrest").is_none());
    assert!(!pkg.has_legacy_schema());
}

// ---- the oldest schema (version 1.0)

#[test]
fn oldest_schema_tree_is_converted() {
    let extra: [(&str, &[u8]); 4] = [
        ("Multimedia/ada.png", b"png-ada"),
        ("Multimedia/ada-late.png", b"png-late"),
        ("Multimedia/group.png", b"png-group"),
        ("Stories/ada.rtf", b"{\\rtf1\\ansi Ada wrote letters.}"),
    ];
    for (file, part) in [("legacy_old.family", "family"), ("legacy_old.family4", "family4")] {
        let pkg = Package::from_bytes(&package(file, part, &extra)).unwrap();
        let report = pkg.migration().unwrap();
        assert_eq!(report.from_version, "1.0");
        let tree = Tree::from_doc(&pkg.doc);
        assert_eq!(tree.people.len(), 4, "{file}");
        // GUID ids became 22-character ids everywhere, including the home person reference
        assert!(tree.people.iter().all(|p| p.id.len() == 22), "{file}");
        let ada = named(&tree, "Ada Whitcombe");
        assert_eq!(tree.home_person.as_deref(), Some(ada.id.as_str()));

        // names: `Name` is primary, `AlternativeName` keeps its type
        assert_eq!(ada.names.len(), 2);
        assert!(ada.names[0].is_primary && !ada.names[1].is_primary);
        assert_eq!((ada.names[1].given.as_str(), ada.names[1].name_type.as_str()), ("Augusta", "Birth"));

        // dates and places
        let birth = ada.fact("BIRT").unwrap();
        assert_eq!((birth.date.year, birth.date.month, birth.date.day), (Some(1815), Some(12), Some(10)));
        assert_eq!(birth.place, "Dunmow, Essex");
        assert_eq!(birth.notes.len(), 1, "the plain `Note` string became a note");
        assert_eq!(birth.notes[0].text, "Born at the rectory.");
        let death = ada.fact("DEAT").unwrap();
        assert_eq!((death.date.descriptor.as_str(), death.date.text.as_str()), ("TextOnly", "winter 1852"));
        let occupation = named(&tree, "William Whitcombe").fact("OCCU").unwrap();
        // a date range is a Span for the app (Range only for a "Between" start descriptor)
        assert_eq!(occupation.date.descriptor, "Span");
        assert_eq!(
            (occupation.date.year, occupation.date.end_year, occupation.date.end_month),
            (Some(1830), Some(1850), Some(12))
        );
        let address = ada.fact("FACT_ContactAddress").unwrap();
        assert_eq!(address.place, "12 Mill Lane\nDunmow\nCM6 1AA\nEngland");

        // photos -> PackagedFile + Multimedia, avatar -> PrimaryImageId; root collections too
        assert_eq!(tree.media.len(), 3);
        assert_eq!(ada.media.len(), 2);
        let primary = tree.primary_photo(ada).expect("avatar");
        assert_eq!((primary.part.as_str(), primary.file_name.as_str()), ("Multimedia/ada.png", "ada.png"));
        assert!(tree.media.values().all(|m| pkg.part(&m.part).is_some()), "{file}");
        let group = tree.media.values().find(|m| m.file_name == "group.png").unwrap();
        assert_eq!(group.title, "Family reunion", "PackagedFilePropertiesCollection entry applied");
        assert_eq!(report.media, 3);

        // story file -> story note; the app reads every story file as rich text
        assert_eq!(ada.notes.len(), 1);
        assert_eq!(ada.notes[0].format, "text/richtext");
        assert!(ada.notes[0].plain_text().contains("Ada wrote letters"));
        let story = person_el(&pkg.doc, &ada.id).path("Notes/Notes/Note/IsStoryNote").unwrap().text();
        assert_eq!(story, "true");

        // relationships; the marriage fact carries the old Descriptor
        assert_eq!(tree.spouse_links.len(), 1);
        let marriage = &tree.spouse_links[0].facts[0];
        assert_eq!(
            (marriage.tag.as_str(), marriage.date.descriptor.as_str(), marriage.date.year),
            ("MARR", "Before", Some(1835))
        );
        let (william, anne) = (named(&tree, "William Whitcombe").id.clone(), named(&tree, "Anne Whitcombe").id.clone());
        assert_eq!(tree.parents(&anne).len(), 2);
        // the modifier is the one on the child's `Parent` entry: William's `Child` entry has none, but Anne's
        // entry for him says Foster (checked against the app's conversion)
        let modifier_of =
            |p: &str| tree.parent_links.iter().find(|l| l.parent == p && l.child == anne).unwrap().modifier.clone();
        assert_eq!((modifier_of(&ada.id).as_str(), modifier_of(&william).as_str()), ("", "Foster"));

        assert!(pkg.doc.root.child("Author").is_none() && pkg.doc.root.child("AuthorCollection").is_some());
        assert!(pkg.doc.root.child("PhotoCollection").is_none());

        assert_converted(&pkg);
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert_save_roundtrip(&pkg);
    }
}

#[test]
fn oldest_schema_lds_facts_citations_addresses_and_roles() {
    let extra: [(&str, &[u8]); 1] = [("Stories/ada.rtf", b"{\\rtf1\\ansi Ada.}")];
    let pkg = Package::from_bytes(&package("legacy_old.family4", "family4", &extra)).unwrap();
    let report = pkg.migration().unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let id = |n: &str| named(&tree, n).id.clone();
    let (ada, william, anne, hugh) =
        (id("Ada Whitcombe"), id("William Whitcombe"), id("Anne Whitcombe"), id("Hugh Parry"));
    let ada_el = person_el(&pkg.doc, &ada);

    // LDS facts: appended after the existing facts, then the biology facts, then the contact address
    assert_eq!(
        fact_tags(ada_el),
        [
            "BIRT",
            "DEAT",
            "BAPL",
            "ENDL",
            "SLGC",
            "BAPL",
            "FACT_BloodGroup",
            "FACT_LeftEyeColor",
            "FACT_HairColor",
            "FACT_ContactAddress"
        ]
    );
    let lds = |tag: &str, n: usize| fact_el(ada_el, tag, n);
    for f in [lds("BAPL", 0), lds("ENDL", 0), lds("SLGC", 0), lds("BAPL", 1)] {
        assert_eq!(f.child_text("ReligiousAffiliation").as_deref(), Some("LDS Church"));
        assert!(f.child("Details").is_none(), "the app does not carry the details of an LDS fact");
    }
    let temple = |f: &Element| f.child_text("PlaceDetails/Description");
    assert_eq!(temple(lds("BAPL", 0)).as_deref(), Some("Salt Lake Temple"));
    assert_eq!(temple(lds("ENDL", 0)).as_deref(), Some("London England Temple"), "alias LD");
    assert_eq!(temple(lds("SLGC", 0)).as_deref(), Some("Salt Lake Temple"), "a name prefix finds the temple");
    assert_eq!(temple(lds("BAPL", 1)), None, "temple code BLANK");
    let status = |f: &Element| f.child_text("DateStatus");
    assert_eq!(status(lds("BAPL", 0)).as_deref(), Some("Completed"));
    assert_eq!(status(lds("ENDL", 0)).as_deref(), Some("Pre1970"));
    assert_eq!(status(lds("SLGC", 0)).as_deref(), Some("Bic"));
    assert_eq!(status(lds("BAPL", 1)), None);
    assert_eq!(
        lds("BAPL", 0).child_text("FactDate/StartYear").as_deref(),
        Some("1850"),
        "the old date is converted too"
    );
    // schema order of the new members
    let order: Vec<&str> = lds("BAPL", 0).elements().map(|e| e.name.as_str()).collect();
    assert_eq!(order, ["Tag", "ReligiousAffiliation", "FactDate", "PlaceDetails", "DateStatus"]);

    // the sealing of the couple becomes a family fact of the couple (the app's chain loses it; not worth losing)
    let link = &tree.spouse_links[0];
    let tags: Vec<&str> = link.facts.iter().map(|f| f.tag.as_str()).collect();
    assert_eq!(tags, ["MARR", "SLGS"]);
    assert_eq!(link.facts[1].place, "London England Temple");
    assert_eq!(report.facts, 4 + 1 + 3, "four LDS facts, one sealing, three biology facts");

    // obsolete Biology members: the unknown/NotYetKnown ones are no facts, an empty element is a fact without details
    assert_eq!(fact_el(ada_el, "FACT_BloodGroup", 0).child_text("Details"), None);
    assert_eq!(fact_el(ada_el, "FACT_LeftEyeColor", 0).child_text("Details").as_deref(), Some("green"));
    assert_eq!(fact_el(ada_el, "FACT_HairColor", 0).child_text("Details").as_deref(), Some("auburn"));
    let biology = ada_el.child("Biology").unwrap();
    assert!(biology.child("YDNA").is_none(), "HaplogroupY was Unknown");
    assert_eq!(biology.path("MtDNA/MtDNA/Haplogroup").unwrap().text(), "H1");

    // citations: a single `Citation` on a fact or a name moves into the citation collection; its old members convert
    let birth = ada.as_str();
    let birth = tree.person(birth).unwrap().fact("BIRT").unwrap();
    assert_eq!(birth.citations.len(), 1);
    assert_eq!(birth.citations[0].page, "p. 12");
    assert_eq!(tree.sources.len(), 1);
    assert_eq!(birth.citations[0].source, tree.sources[0].id);
    let citation = fact_el(ada_el, "BIRT", 0).path("FactCitationCollection/Citations/Citation").unwrap();
    let child = |n: &str| citation.child_text(n);
    assert_eq!(child("EventType").as_deref(), Some("BIRT"), "Event is read case-insensitively");
    assert_eq!(child("CustomEventType").as_deref(), Some("Baptised soon after"));
    assert_eq!(child("Role").as_deref(), Some("Child"), "RoleInEvent CitationRoleCHIL");
    assert_eq!(citation.child_text("ActualTexts/FlatNotes/FlatNote/Text").as_deref(), Some("born 10 Dec"));
    let order: Vec<&str> = citation.elements().map(|e| e.name.as_str()).collect();
    assert_eq!(order, ["Source", "Page", "EventType", "CustomEventType", "Role", "ActualTexts"]);
    let name = ada_el.path("PersonNameCollection/Names/Name").unwrap();
    assert_eq!(name.child_text("NameCitationCollection/Citations/Citation/Page").as_deref(), Some("name entry"));
    assert_eq!(
        name.elements().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        ["Names", "Surname", "NameCitationCollection", "IsPrimary"]
    );

    // addresses: a string, and the first of a list with its lines joined into Details
    let death = fact_el(ada_el, "DEAT", 0);
    assert_eq!(death.child_text("FullAddress/Details").as_deref(), Some("Churchyard Lane, Dunmow"));
    let occupation = fact_el(person_el(&pkg.doc, &william), "OCCU", 0);
    assert_eq!(
        occupation.child_text("FullAddress/Details").as_deref(),
        Some("W. Whitcombe\nSurveyors Office\nChelmsford")
    );
    assert_eq!(occupation.child_text("FullAddress/Phone").as_deref(), Some("0123"));
    assert!(occupation.child("Addresses").is_none() && death.child("Address").is_none());

    // a witness becomes an association of the same fact
    assert_eq!(associations(death), [(String::new(), "Rev. Smith".to_string(), "Clergy".to_string(), String::new())]);

    // Hugh is Anne's sponsor (a Child relation with a role, mirrored by Anne's Parent relation): one association
    assert_eq!(
        associations(fact_el(person_el(&pkg.doc, &anne), "BIRT", 0)),
        [(hugh.clone(), String::new(), "Sponsor".to_string(), String::new())]
    );
    assert!(tree.parent_links.iter().all(|l| l.parent != hugh));
    assert_eq!(report.associations, 2, "the sponsor and the witness");

    assert_converted(&pkg);
    assert_save_roundtrip(&pkg);
}

// ---- rules of single constructs

#[test]
fn old_parent_modifiers_follow_the_apps_rules() {
    // Pairs: the modifier of the child's `Parent` entry decides; an unknown text (a misspelt godparent) is a
    // natural parent; "foster" matches case-insensitively; only the exact roles become associations.
    let person = |n: u32, name: &str, rel: &str| {
        format!(
            "<Person Id=\"P{n}\"><Name><Names>{name}</Names><Surname>T</Surname></Name>\
             <Relationships>{rel}</Relationships></Person>"
        )
    };
    let rel = |kind: &str, other: u32, modifier: &str| {
        format!(
            "<Relationship xsi:type=\"{kind}Relationship\"><PersonId>P{other}</PersonId>\
             <RelationshipType>{kind}</RelationshipType><ParentChildModifier>{modifier}</ParentChildModifier></Relationship>"
        )
    };
    let mut people = String::new();
    // parent n*2+1, child n*2+2: (child entry, parent entry)
    let cases = [("Foster", "Adopted"), ("Natural", "foster"), ("Natural", "Godparent"), ("Natural", "GodParent")];
    for (n, (child_entry, parent_entry)) in cases.iter().enumerate() {
        let (p, c) = (n as u32 * 2 + 1, n as u32 * 2 + 2);
        people += &person(p, &format!("p{n}"), &rel("Child", c, child_entry));
        people += &person(c, &format!("c{n}"), &rel("Parent", p, parent_entry));
    }
    let xml = format!(
        "<Family xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><PeopleCollection>{people}</PeopleCollection>\
         <FileProperties><Version>10.0</Version></FileProperties></Family>"
    );
    let pkg = inline(&xml);
    let tree = Tree::from_doc(&pkg.doc);
    let modifier = |p: u32, c: u32| {
        tree.parent_links
            .iter()
            .find(|l| l.parent == format!("P{p}") && l.child == format!("P{c}"))
            .map(|l| l.modifier.clone())
    };
    assert_eq!(modifier(1, 2).as_deref(), Some("Adopted"));
    assert_eq!(modifier(3, 4).as_deref(), Some("Foster"));
    assert_eq!(modifier(5, 6).as_deref(), Some(""), "unknown text counts as natural");
    assert_eq!(modifier(7, 8), Some(String::new()), "the Child entry says Natural, so the exact GodParent role loses");
    assert_eq!(tree.parent_links.len(), 4);
    assert!(!pkg.has_legacy_schema());
}

#[test]
fn fact_type_family_counts_from_version_7() {
    let xml = |version: &str| {
        format!(
            "<Family><PeopleCollection><Person Id=\"p\"><Facts><Fact><Tag>RESI</Tag><FactType>Family</FactType></Fact>\
             <Fact><Tag>OCCU</Tag><FactType>Individual</FactType></Fact></Facts></Person></PeopleCollection>\
             <FileProperties><Version>{version}</Version></FileProperties></Family>"
        )
    };
    for (version, is_family) in [("6.0", false), ("7.0", true), ("11.0", true)] {
        let pkg = inline(&xml(version));
        let p = person_el(&pkg.doc, "p");
        assert_eq!(fact_el(p, "RESI", 0).child_text("IsFamily").is_some(), is_family, "{version}");
        assert!(fact_el(p, "OCCU", 0).child("IsFamily").is_none());
        assert!(!pkg.has_legacy_schema(), "{version}");
    }
}

#[test]
fn witnesses_map_their_type_to_a_role() {
    let xml = "<Family><PeopleCollection><Person Id=\"p\"><Facts><Fact><Tag>BIRT</Tag><Witnesses>\
        <FactWitness><Type>godparent</Type></FactWitness>\
        <FactWitness><Type>Surprise</Type></FactWitness>\
        <FactWitness><PersonName>No type</PersonName></FactWitness>\
        <FactWitness><Role>Friend</Role><PersonId>q</PersonId></FactWitness>\
        </Witnesses></Fact></Facts></Person></PeopleCollection>\
        <FileProperties><Version>11.0</Version></FileProperties></Family>";
    let pkg = inline(xml);
    let roles: Vec<String> =
        associations(fact_el(person_el(&pkg.doc, "p"), "BIRT", 0)).into_iter().map(|a| a.2).collect();
    assert_eq!(roles, ["GodParent", "Custom", "Witness", "Friend"]);
}

#[test]
fn unversioned_trees_are_recognised_by_their_old_members() {
    let old = "<Family><PeopleCollection><Person Id=\"p\"><Biology><HairColor>red</HairColor></Biology>\
               </Person></PeopleCollection></Family>";
    let pkg = inline(old);
    assert!(pkg.migration().is_some());
    assert_eq!(fact_tags(person_el(&pkg.doc, "p")), ["FACT_HairColor"]);
    assert!(!pkg.has_legacy_schema());
    let plain = "<Family><PeopleCollection><Person Id=\"p\"><Biology><Gender>Male</Gender></Biology>\
                 </Person></PeopleCollection></Family>";
    assert!(inline(plain).migration().is_none());
}

// ---- database-backed stories (`stories7`, a LiteDB file; fixtures made by `make_stories7.cs` with LiteDB 4.1.4)

fn stories_db(name: &str) -> Vec<u8> {
    std::fs::read(format!("{DATA}/{name}")).unwrap()
}

/// `legacy_stories.xml` (version 8.0) in a `family4` package with the story database as part `stories7`.
fn stories_package(xml: &str, db: &[u8]) -> Vec<u8> {
    let blob = codec::encode_part(format!("\u{feff}{xml}").as_bytes(), codec::SourceFormat::Family4);
    let types = "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
                 <Override PartName=\"/family4\" ContentType=\"application/vnd.chronoplexsoftware.familyx\" />\
                 <Override PartName=\"/stories7\" ContentType=\"application/octet-stream\" /></Types>";
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
                <Relationship Type=\"MyFamilyTree/Relationship\" Target=\"/family4\" Id=\"R1\" />\
                <Relationship Type=\"MyFamilyTree/Stories\" Target=\"/stories7\" Id=\"R2\" /></Relationships>";
    zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("family4", &blob),
        ("stories7", db),
    ])
}

fn note_texts(pkg: &Package, name: &str) -> Vec<(String, String)> {
    let tree = Tree::from_doc(&pkg.doc);
    named(&tree, name).notes.iter().map(|n| (n.format.clone(), n.text.clone())).collect()
}

#[test]
fn database_backed_stories_become_story_notes() {
    let xml = std::fs::read_to_string(format!("{DATA}/legacy_stories.xml")).unwrap();
    let pkg = Package::from_bytes(&stories_package(&xml, &stories_db("stories7"))).unwrap();
    let report = pkg.migration().unwrap();
    assert_eq!(report.notes, 3, "the empty story makes no note: {:?}", report.warnings);
    assert_eq!(report.consumed_parts, ["stories7"]);
    // the annotations are not converted, which is said
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("annotations"), "{:?}", report.warnings);
    assert!(!pkg.has_legacy_schema());
    assert!(pkg.part("stories7").is_none() && pkg.part_names().all(|n| n != "stories7"));
    assert!(edit::check(&pkg.doc).is_empty());

    // same note as the file-based path makes: text/richtext, IsStoryNote
    let ada = note_texts(&pkg, "Ada Whitcombe");
    assert_eq!(ada.len(), 1);
    assert_eq!(ada[0].0, "text/richtext");
    assert!(ada[0].1.starts_with("{\\rtf1") && ada[0].1.contains("Ada grew up at the rectory in Dunmow."));
    let ada_id =
        Tree::from_doc(&pkg.doc).people.iter().find(|p| p.display_name() == "Ada Whitcombe").unwrap().id.clone();
    assert_eq!(person_el(&pkg.doc, &ada_id).path("Notes/Notes/Note/IsStoryNote").unwrap().text(), "true");
    assert!(person_el(&pkg.doc, &ada_id).child("Story").is_none());

    // non-ASCII text survives byte for byte, plain text is kept as it is
    let william = note_texts(&pkg, "William Whitcombe");
    assert_eq!(
        william[0].1,
        "William was a surveyor.\r\nUnicode: \u{c5}se, K\u{f6}ping, \u{6c34}\u{8eca}, na\u{ef}ve caf\u{e9} \u{2615}, \u{1d11e}.\r\nPlain text kept as is."
    );

    // a story of two chunks (more than 255 KiB) is joined
    let anne = note_texts(&pkg, "Anne Whitcombe");
    assert!(anne[0].1.len() > 255 * 1024, "{}", anne[0].1.len());
    assert!(anne[0].1.starts_with("{\\rtf1") && anne[0].1.ends_with("\\par}"));
    assert!(anne[0].1.contains("Paragraph 2599: \u{c5}se walked to K\u{f6}ping"));

    // an empty file is skipped like the app does
    assert!(note_texts(&pkg, "Hugh Parry").is_empty());

    // saved packages carry neither the database nor its metadata, and are stable
    let saved = pkg.to_bytes().unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
    assert!((0..zip.len()).all(|i| !zip.by_index(i).unwrap().name().contains("stories7")));
    let mut types = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("[Content_Types].xml").unwrap(), &mut types).unwrap();
    assert!(!types.contains("stories7"), "{types}");
    let mut rels = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("_rels/.rels").unwrap(), &mut rels).unwrap();
    assert!(!rels.contains("stories7") && rels.contains("/family8"), "{rels}");
    let reopened = assert_save_roundtrip(&pkg);
    assert!(!reopened.has_legacy_schema());
    assert_eq!(note_texts(&reopened, "Anne Whitcombe"), note_texts(&pkg, "Anne Whitcombe"));
}

#[test]
fn story_database_passwords_by_version_and_without_password() {
    let story = |version: &str, id: &str| {
        format!(
            "<Family><PeopleCollection><Person Id=\"p\"><Name><Names>Hugh</Names><Surname>Parry</Surname>\
             <IsPrimary>true</IsPrimary></Name><Story><FileId>{id}</FileId></Story></Person></PeopleCollection>\
             <FileProperties><Version>{version}</Version></FileProperties></Family>"
        )
    };
    for (version, db, id, text) in [
        ("7.0", "stories7.a", "22222222-aaaa-4bbb-8ccc-000000000001", "Hugh stood godfather to Anne."),
        ("7.5", "stories7.a", "22222222-aaaa-4bbb-8ccc-000000000001", "Hugh stood godfather to Anne."),
        // the stored hash identifies the password, whatever the version says
        ("12.0", "stories7.a", "22222222-aaaa-4bbb-8ccc-000000000001", "Hugh stood godfather to Anne."),
        ("14.1", "stories7.plain", "33333333-aaaa-4bbb-8ccc-000000000001", "Unprotected database."),
    ] {
        let pkg = Package::from_bytes(&stories_package(&story(version, id), &stories_db(db))).unwrap();
        assert!(!pkg.has_legacy_schema(), "{version} {db}: {:?}", pkg.migration().unwrap().warnings);
        let notes = note_texts(&pkg, "Hugh Parry");
        assert!(notes.len() == 1 && notes[0].1.contains(text), "{version} {db}: {notes:?}");
    }
}

#[test]
fn database_backed_stories_with_missing_ids_keep_the_database() {
    // Ada's id is in the database, the second person's is not
    let xml = "<Family><PeopleCollection>\
        <Person Id=\"a\"><Name><Names>Ada</Names><Surname>W</Surname><IsPrimary>true</IsPrimary></Name>\
        <Story><FileId>11111111-aaaa-4bbb-8ccc-000000000001</FileId></Story></Person>\
        <Person Id=\"b\"><Name><Names>Bea</Names><Surname>W</Surname><IsPrimary>true</IsPrimary></Name>\
        <Story><FileId>no-such-file</FileId><AnnotationsFileId>11111111-aaaa-4bbb-8ccc-000000000002</AnnotationsFileId>\
        </Story></Person></PeopleCollection><FileProperties><Version>9.0</Version></FileProperties></Family>";
    let pkg = Package::from_bytes(&stories_package(xml, &stories_db("stories7"))).unwrap();
    let report = pkg.migration().unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("no-such-file") && w.contains("not in the story database")));
    assert!(report.consumed_parts.is_empty());
    assert!(pkg.part("stories7").is_some(), "the database stays while a story is unresolved");
    assert!(pkg.has_legacy_schema());
    assert_eq!(note_texts(&pkg, "Ada W").len(), 1);
    let story = person_el(&pkg.doc, "b").child("Story").unwrap();
    assert_eq!(story.child_text("FileId").as_deref(), Some("no-such-file"));
    assert!(story.child("AnnotationsFileId").is_some());
    let reopened = assert_save_roundtrip_keeping_legacy(&pkg);
    assert!(reopened.part("stories7").is_some());
}

/// Like [`assert_save_roundtrip`], for trees that still carry old elements.
fn assert_save_roundtrip_keeping_legacy(pkg: &Package) -> Package {
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert!(reopened.migration().is_none());
    assert!(reopened.doc == pkg.doc);
    assert!(reopened.has_legacy_schema());
    reopened
}

#[test]
fn unreadable_story_databases_keep_the_stories() {
    let xml = "<Family><PeopleCollection><Person Id=\"p\"><Story><FileId>f1</FileId>\
               <AnnotationsFileId>f2</AnnotationsFileId></Story></Person></PeopleCollection>\
               <FileProperties><Version>8.0</Version></FileProperties></Family>";
    // no database part
    let pkg = inline(xml);
    let report = pkg.migration().unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("stories7 is not in the package")), "{:?}", report.warnings);
    assert!(pkg.has_legacy_schema());
    assert_eq!(person_el(&pkg.doc, "p").child("Story").unwrap().elements().count(), 2);
    assert_save_roundtrip_keeping_legacy(&pkg);
    // garbage instead of a database
    let pkg = Package::from_bytes(&stories_package(xml, b"not a database")).unwrap();
    assert!(pkg.migration().unwrap().warnings.iter().any(|w| w.contains("cannot be read")));
    assert!(pkg.has_legacy_schema() && pkg.part("stories7").is_some());
    // a truncated database
    let db = stories_db("stories7");
    let pkg = Package::from_bytes(&stories_package(xml, &db[..db.len() / 2])).unwrap();
    assert!(pkg.has_legacy_schema(), "{:?}", pkg.migration().unwrap().warnings);
}

// ---- general

#[test]
fn migrated_tree_stays_editable() {
    let mut pkg = Package::from_bytes(&package("legacy_rel.family5", "family5", &[])).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let joan = named(&tree, "Joan Ashby").id.clone();
    let mary = named(&tree, "Mary Okafor").id.clone();
    let child = edit::add_person(&mut pkg.doc, "Kit", "Ashby", Gender::Male);
    edit::link_parent(&mut pkg.doc, &child, &joan).unwrap();
    edit::link_spouse(&mut pkg.doc, &joan, &mary).unwrap();
    assert!(edit::check(&pkg.doc).is_empty());
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert_eq!(Tree::from_doc(&reopened.doc).people.len(), 7);
}

#[test]
fn current_schema_trees_are_left_alone() {
    let pkg = Package::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/example.familyx")).unwrap();
    assert!(pkg.migration().is_none());
    let mut doc = pkg.doc.clone();
    assert!(migrate::migrate(&mut doc, &|_| None).is_none());
    assert!(doc == pkg.doc);
}

#[test]
fn migration_is_idempotent() {
    for (file, part) in [("legacy_old.family", "family"), ("legacy_rel.family5", "family5")] {
        // (the story file must be in the package, or the story stays as an old element)
        let extra: [(&str, &[u8]); 2] = [("Stories/ada.rtf", b"{\\rtf1 x}"), ("Multimedia/crest.png", b"crest")];
        let pkg = Package::from_bytes(&package(file, part, &extra)).unwrap();
        assert!(!pkg.has_legacy_schema(), "{file}");
        // the version bump makes a second run a no-op ...
        let mut again = pkg.doc.clone();
        assert!(migrate::migrate(&mut again, &|_| None).is_none(), "{file}");
        assert!(again == pkg.doc);
        // ... and so does the absence of old elements, even if the version is lost
        let props = again.root.child_mut("FileProperties").unwrap();
        props.remove_children_where(|e| e.name == "Version");
        let before = again.clone();
        assert!(migrate::migrate(&mut again, &|_| None).is_none(), "{file}");
        assert!(again == before);
    }
}

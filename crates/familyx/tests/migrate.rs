//! Schema migration of old-format trees. The fixtures (`tests/data/old/legacy_*.xml`, fictional) are
//! hand-written after the app's pre-link classes (per-person `Relationships`, `Name`, `Photos`, `Story`,
//! old `CustomDate` members, ...); their encrypted forms were produced by `make_vectors.py`.
//! The expected results follow the app's conversion chain (see docs/design.md, "Schema migration").

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

/// A package whose tree part is the fixture `file`, stored under the old part name `part`.
fn package(file: &str, part: &str, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let blob = std::fs::read(format!("{DATA}/{file}")).unwrap();
    let types = format!(
        "<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Override PartName=\"/{part}\" ContentType=\"application/vnd.chronoplexsoftware.familyx\" /></Types>"
    );
    let rels = format!(
        "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Type=\"MyFamilyTree/Relationship\" Target=\"/{part}\" Id=\"R1\" /></Relationships>"
    );
    let mut parts: Vec<(&str, &[u8])> =
        vec![("[Content_Types].xml", types.as_bytes()), ("_rels/.rels", rels.as_bytes()), (part, &blob)];
    parts.extend_from_slice(extra);
    zip(&parts)
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

#[test]
fn relationship_era_tree_gets_links() {
    for (file, part) in [("legacy_rel.family5", "family5"), ("legacy_rel.family7_v2", "family7")] {
        let pkg = Package::from_bytes(&package(file, part, &[("Multimedia/harold.png", b"\x89PNG")])).unwrap();
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

        // parents: Walter's came from his Parent relations, Joan's from hers, Harold's Child entry is the same link
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

        // data that cannot be converted is kept
        assert_eq!(report.warnings.len(), 2, "{:?}", report.warnings);
        assert!(pkg.has_legacy_schema());
        let leftover = |name: &str| {
            let p = pkg
                .doc
                .root
                .child("PeopleCollection")
                .unwrap()
                .children_named("Person")
                .find(|p| p.attr("Id").as_deref() == Some(id(name).as_str()))
                .unwrap();
            p.child("Relationships").map(|r| r.elements().count())
        };
        assert_eq!(leftover("Walter Ashby"), Some(1), "the godparent entry stays");
        assert_eq!(leftover("Joan Ashby"), Some(1), "the dangling spouse entry stays");
        assert_eq!(leftover("Harold Ashby"), None);

        // the redundant records the app needs
        assert!(edit::check(&pkg.doc).is_empty(), "{:?}", edit::check(&pkg.doc));
        let doc_person = |pid: &str| {
            pkg.doc
                .root
                .child("PeopleCollection")
                .unwrap()
                .children_named("Person")
                .find(|p| p.attr("Id").as_deref() == Some(pid))
                .unwrap()
        };
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

        // photos that were already PackagedFiles are untouched
        assert_eq!(tree.media.len(), 1);
        assert_eq!(pkg.part("Multimedia/harold.png"), Some(&b"\x89PNG"[..]));

        let reopened = assert_save_roundtrip(&pkg);
        assert!(edit::check(&reopened.doc).is_empty());
    }
}

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
        assert_eq!(tree.people.len(), 3, "{file}");
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
        assert_eq!(occupation.date.descriptor, "Range");
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

        // story file -> story note
        assert_eq!(ada.notes.len(), 1);
        assert_eq!(ada.notes[0].format, "text/richtext");
        assert!(ada.notes[0].plain_text().contains("Ada wrote letters"));
        let story = pkg
            .doc
            .root
            .child("PeopleCollection")
            .unwrap()
            .children_named("Person")
            .find(|p| p.attr("Id").as_deref() == Some(ada.id.as_str()))
            .unwrap()
            .path("Notes/Notes/Note/IsStoryNote")
            .unwrap()
            .text();
        assert_eq!(story, "true");

        // relationships; the marriage fact carries the old Descriptor
        assert_eq!(tree.spouse_links.len(), 1);
        let marriage = &tree.spouse_links[0].facts[0];
        assert_eq!(
            (marriage.tag.as_str(), marriage.date.descriptor.as_str(), marriage.date.year),
            ("MARR", "Before", Some(1835))
        );
        let anne = named(&tree, "Anne Whitcombe");
        assert_eq!(tree.parents(&anne.id).len(), 2);
        // the child's `Natural` entry decides, like the app (the Parent entry saying Foster is its mirror)
        assert!(tree.parent_links.iter().all(|l| l.modifier.is_empty()));

        // LDSFacts have no conversion here and stay
        assert!(pkg.has_legacy_schema());
        assert!(pkg.doc.root.child("PeopleCollection").unwrap().elements().any(|p| p.child("LDSFacts").is_some()));
        assert!(pkg.doc.root.child("Author").is_none() && pkg.doc.root.child("AuthorCollection").is_some());
        assert!(pkg.doc.root.child("PhotoCollection").is_none());

        assert!(edit::check(&pkg.doc).is_empty(), "{:?}", edit::check(&pkg.doc));
        assert_save_roundtrip(&pkg);
    }
}

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
    let pkg = Package::from_bytes(&package("legacy_old.family", "family", &[])).unwrap();
    let mut again = pkg.doc.clone();
    // leftovers (LDSFacts, ...) keep the schema "legacy", but the version bump makes a second run a no-op on the data
    migrate::migrate(&mut again, &|_| None);
    assert!(again == pkg.doc);
}

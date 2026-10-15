//! Adding one tree to another and merging duplicate people (fictional data only).

use std::collections::HashSet;

use familyx::doc::{Document, Element};
use familyx::edit::{self, FactInput};
use familyx::merge::{merge_people, MergeOptions, Side};
use familyx::model::{Date, Gender, Id, Tree};
use familyx::Package;

fn born(year: i32, place: &str) -> FactInput {
    FactInput { date: Date::year(year), place: place.into(), details: String::new() }
}

fn source(doc: &mut Document, title: &str) -> Id {
    edit::add_source(doc, &edit::SourceInput { title: title.into(), ..Default::default() }).unwrap()
}

/// The cousin's file: Jaan and Mari Kask with a daughter, a source, a note and a photo.
fn cousins_tree() -> (Package, Id, Id, Id) {
    let mut pkg = Package::new();
    let jaan = edit::add_person(&mut pkg.doc, "Jaan", "Kask", Gender::Male);
    let mari = edit::add_person(&mut pkg.doc, "Mari", "Kask", Gender::Female);
    let liis = edit::add_person(&mut pkg.doc, "Liis", "Kask", Gender::Female);
    edit::link_parent(&mut pkg.doc, &liis, &jaan).unwrap();
    edit::link_parent(&mut pkg.doc, &liis, &mari).unwrap();
    let couple = edit::link_spouse(&mut pkg.doc, &jaan, &mari).unwrap();
    let marriage = edit::add_couple_fact(&mut pkg.doc, &couple, "MARR", &born(1900, "Tartu")).unwrap();
    let birth = edit::add_fact(&mut pkg.doc, &jaan, "BIRT", &born(1875, "Otepää")).unwrap();
    let register = source(&mut pkg.doc, "Parish register");
    edit::add_citation(&mut pkg.doc, &birth, &register, "p. 4").unwrap();
    edit::add_citation(&mut pkg.doc, &marriage, &register, "p. 9").unwrap();
    edit::add_citation(&mut pkg.doc, &jaan, &register, "").unwrap();
    edit::set_person_note(&mut pkg.doc, &jaan, "Farmer near Otepää").unwrap();
    pkg.add_photo(&jaan, vec![1, 2, 3, 4], "png", "jaan.png").unwrap();
    (pkg, jaan, mari, liis)
}

#[test]
fn append_adds_people_with_fresh_ids_and_correct_references() {
    let mut mine = Package::new();
    let ott = edit::add_person(&mut mine.doc, "Ott", "Ilves", Gender::Male);
    edit::set_home_person(&mut mine.doc, &ott).unwrap();
    // the same source is already here: it must be reused, not copied
    source(&mut mine.doc, "Parish register");

    let (theirs, jaan, mari, liis) = cousins_tree();
    let theirs_before = theirs.doc.clone();
    let report = mine.append(&theirs).unwrap();

    assert_eq!(report.people.len(), 3);
    assert_eq!((report.couples, report.parent_links, report.sources, report.photos), (1, 2, 0, 1));
    assert_eq!(theirs.doc, theirs_before, "the other package is not changed");
    assert_eq!(edit::check(&mine.doc), Vec::<String>::new());

    let tree = Tree::from_doc(&mine.doc);
    assert_eq!(tree.people.len(), 4);
    assert_eq!(tree.home_person.as_deref(), Some(ott.as_str()));
    assert_eq!(tree.sources.len(), 1);
    let ids: HashSet<&str> = tree.people.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), 4);
    for old in [&jaan, &mari, &liis] {
        assert!(tree.person(old).is_none(), "ids are new");
    }
    let new_of = |name: &str| tree.people.iter().find(|p| p.display_name() == name).unwrap();
    let (j, m, l) = (new_of("Jaan Kask"), new_of("Mari Kask"), new_of("Liis Kask"));
    assert_eq!(tree.parents(&l.id).len(), 2);
    assert!(tree.parents(&l.id).contains(&j.id.as_str()));
    assert_eq!(tree.spouses(&j.id), vec![m.id.as_str()]);
    let couple = &tree.spouse_links[0];
    assert_eq!(couple.facts[0].citations[0].source, tree.sources[0].id, "citation points at the reused source");
    // the child belongs to the couple
    let child = mine
        .doc
        .root
        .child("PeopleCollection")
        .unwrap()
        .elements()
        .find(|p| p.attr("Id").as_deref() == Some(l.id.as_str()))
        .unwrap();
    assert_eq!(
        child.child("FamilyChildLinks").unwrap().elements().next().unwrap().attr("FamilyId").unwrap(),
        couple.id
    );

    assert_eq!(j.facts[0].citations[0].page, "p. 4");
    assert_eq!(j.citations.len(), 1);
    assert_eq!(j.notes[0].text, "Farmer near Otepää");
    assert_eq!(j.media.len(), 1);
    let photo = tree.primary_photo(j).expect("photo came along");
    assert_eq!(mine.part(&photo.part), Some(&[1u8, 2, 3, 4][..]));
    assert_ne!(Some(photo.part.as_str()), theirs.part_names().find(|n| n.starts_with("Multimedia/")));

    // it survives saving and opening, and a second import adds another copy
    let mut reopened = Package::from_bytes(&mine.to_bytes().unwrap()).unwrap();
    assert_eq!(Tree::from_doc(&reopened.doc).people.len(), 4);
    reopened.append(&theirs).unwrap();
    assert_eq!(Tree::from_doc(&reopened.doc).people.len(), 7);
    assert_eq!(edit::check(&reopened.doc), Vec::<String>::new());
}

#[test]
fn append_into_an_empty_tree_and_of_an_empty_tree() {
    let (theirs, ..) = cousins_tree();
    let mut empty = Package::new();
    empty.append(&theirs).unwrap();
    assert_eq!(Tree::from_doc(&empty.doc).people.len(), 3);
    assert_eq!(Tree::from_doc(&empty.doc).sources.len(), 1);
    let report = empty.append(&Package::new()).unwrap();
    assert!(report.people.is_empty());
}

#[test]
fn appended_photo_parts_get_safe_names() {
    let mut theirs = Package::new();
    let p = edit::add_person(&mut theirs.doc, "A", "B", Gender::Male);
    theirs.add_photo(&p, vec![9; 3], "png", "a.png").unwrap();
    // a RelativePath whose "extension" is a path
    for path in theirs.doc.root.child_mut("PackagedFileCollection").unwrap().elements_mut() {
        let rel = path.child_text("RelativePath").unwrap();
        path.child_mut("RelativePath").unwrap().set_text(&format!("{rel}.x\\..\\..\\evil"));
    }
    let mut mine = Package::new();
    mine.append(&theirs).unwrap();
    for name in mine.part_names() {
        assert!(!name.contains(".."), "{name}");
    }
    let mut jpeg = Package::new();
    let q = edit::add_person(&mut jpeg.doc, "C", "D", Gender::Male);
    jpeg.add_photo(&q, vec![1], "jpg", "c.JPEG").unwrap();
    for path in jpeg.doc.root.child_mut("PackagedFileCollection").unwrap().elements_mut() {
        let rel = path.child_text("RelativePath").unwrap().replace(".jpg", ".JPEG");
        path.child_mut("RelativePath").unwrap().set_text(&rel);
    }
    let mut mine = Package::new();
    mine.append(&jpeg).unwrap();
    assert!(Tree::from_doc(&mine.doc).media.values().all(|m| m.part.ends_with(".jpg")));
}

/// Two records of Anna Tamm that share a partner and a child, plus what only one of them has.
struct Duplicates {
    doc: Document,
    keep: Id,
    remove: Id,
    partner: Id,
    shared_child: Id,
    own_child: Id,
    parent: Id,
    register: Id,
}

fn duplicates() -> Duplicates {
    let mut doc = edit::new_family_document();
    let keep = edit::add_person(&mut doc, "Anna", "Tamm", Gender::Unknown);
    let remove = edit::add_person(&mut doc, "Anna", "Tamm", Gender::Female);
    let partner = edit::add_person(&mut doc, "Peeter", "Tamm", Gender::Male);
    let shared_child = edit::add_person(&mut doc, "Kati", "Tamm", Gender::Female);
    let own_child = edit::add_person(&mut doc, "Juhan", "Tamm", Gender::Male);
    let parent = edit::add_person(&mut doc, "Mart", "Saar", Gender::Male);
    let other_partner = edit::add_person(&mut doc, "Toomas", "Rebane", Gender::Male);
    let register = source(&mut doc, "Parish register");

    edit::add_fact(&mut doc, &keep, "BIRT", &born(1900, "Tartu")).unwrap();
    edit::add_fact(&mut doc, &remove, "BIRT", &born(1901, "Tartu")).unwrap();
    edit::add_fact(&mut doc, &remove, "DEAT", &born(1980, "Pärnu")).unwrap();
    let same_job =
        edit::add_fact(&mut doc, &keep, "OCCU", &FactInput { details: "Teacher".into(), ..Default::default() })
            .unwrap();
    let same_job_2 =
        edit::add_fact(&mut doc, &remove, "OCCU", &FactInput { details: "Teacher".into(), ..Default::default() })
            .unwrap();
    edit::add_citation(&mut doc, &same_job, &register, "p. 1").unwrap();
    edit::add_citation(&mut doc, &same_job_2, &register, "p. 2").unwrap();
    edit::add_alternate_name(&mut doc, &remove, "Anna", "Saar", "Birth").unwrap();
    edit::set_person_note(&mut doc, &remove, "Born at the mill").unwrap();
    edit::add_citation(&mut doc, &remove, &register, "entry 12").unwrap();
    edit::add_citation(&mut doc, &keep, &register, "entry 12").unwrap();

    // the shared partner: only one record has the marriage
    let couple_remove = edit::link_spouse(&mut doc, &remove, &partner).unwrap();
    edit::add_couple_fact(&mut doc, &couple_remove, "MARR", &born(1925, "Tartu")).unwrap();
    edit::link_spouse(&mut doc, &keep, &partner).unwrap();
    // a partner of the removed record only
    let couple_other = edit::link_spouse(&mut doc, &remove, &other_partner).unwrap();
    edit::set_couple_status(&mut doc, &couple_other, "Divorced").unwrap();

    edit::link_parent(&mut doc, &shared_child, &keep).unwrap();
    edit::link_parent(&mut doc, &shared_child, &remove).unwrap();
    edit::link_parent(&mut doc, &own_child, &remove).unwrap();
    edit::link_parent(&mut doc, &own_child, &partner).unwrap();
    edit::set_parent_modifier(&mut doc, &own_child, &remove, "Adopted").unwrap();
    edit::link_parent(&mut doc, &remove, &parent).unwrap();

    Duplicates { doc, keep, remove, partner, shared_child, own_child, parent, register }
}

fn element_by_id<'a>(doc: &'a mut Document, collection: &str, id: &str) -> &'a mut Element {
    doc.root.child_mut(collection).unwrap().elements_mut().find(|e| e.attr("Id").as_deref() == Some(id)).unwrap()
}

#[test]
fn merge_moves_everything_to_the_kept_person() {
    let mut d = duplicates();
    // photo and home person belong to the record that goes away
    let mut pkg = Package::new();
    std::mem::swap(&mut pkg.doc, &mut d.doc);
    let media = pkg.add_photo(&d.remove, vec![9, 9], "jpg", "anna.jpg").unwrap();
    let mut doc = pkg.doc;
    edit::set_home_person(&mut doc, &d.remove).unwrap();
    edit::touch_file(&mut doc, Some(&d.remove));

    merge_people(&mut doc, &d.keep, &d.remove, MergeOptions::default()).unwrap();

    assert_eq!(edit::check(&doc), Vec::<String>::new());
    let tree = Tree::from_doc(&doc);
    assert!(tree.person(&d.remove).is_none());
    assert_eq!(tree.people.len(), 6);
    let anna = tree.person(&d.keep).unwrap();

    // names and gender: the kept person's name stays primary, the other's becomes an alternate
    assert_eq!(anna.display_name(), "Anna Tamm");
    assert_eq!(anna.gender, Gender::Female, "an unknown gender does not overrule a known one");
    let alternates: Vec<_> = anna.names.iter().filter(|n| !n.is_primary).map(|n| n.display()).collect();
    assert_eq!(alternates, ["Anna Saar"]);

    // events: both births, the first stays primary; the same job is not doubled but gains the citation
    let births: Vec<_> = anna.facts.iter().filter(|f| f.tag == "BIRT").collect();
    assert_eq!(births.len(), 2);
    assert_eq!(births.iter().filter(|f| f.is_primary).count(), 1);
    assert!(births[0].is_primary && births[0].date.year == Some(1900));
    assert!(anna.fact("DEAT").is_some());
    let jobs: Vec<_> = anna.facts.iter().filter(|f| f.tag == "OCCU").collect();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].citations.len(), 2);
    // note, and the citation both already had counted once
    assert_eq!(anna.notes[0].text, "Born at the mill");
    assert_eq!(anna.citations.len(), 1);

    // photo
    assert_eq!(anna.media, std::slice::from_ref(&media));
    assert_eq!(anna.primary_image.as_deref(), Some(media.as_str()));

    // family: the shared partner and child appear once, the marriage moved to the kept couple
    assert_eq!(tree.spouses(&d.keep).iter().filter(|s| **s == d.partner).count(), 1);
    assert_eq!(tree.spouses(&d.keep).len(), 2);
    let couple = tree.spouse_links_of(&d.keep).into_iter().find(|l| l.other(&d.keep) == d.partner).unwrap();
    assert_eq!(couple.facts.len(), 1);
    assert_eq!(couple.facts[0].tag, "MARR");
    let divorced = tree.spouse_links_of(&d.keep).into_iter().find(|l| l.other(&d.keep) != d.partner).unwrap();
    assert_eq!(divorced.status, "Divorced");
    let children = tree.children(&d.keep);
    assert_eq!(children.iter().filter(|c| **c == d.shared_child).count(), 1);
    assert!(children.contains(&d.own_child.as_str()));
    assert_eq!(tree.parents(&d.keep), [d.parent.as_str()]);
    let adopted = tree.parent_links.iter().find(|l| l.parent == d.keep && l.child == d.own_child).unwrap();
    assert_eq!(adopted.modifier, "Adopted");
    assert_eq!(tree.parent_links.len(), 4, "no doubled links");

    // the child with both parents belongs to the (merged) couple
    let family_links = |doc: &mut Document, child: &str| -> Vec<String> {
        element_by_id(doc, "PeopleCollection", child)
            .child("FamilyChildLinks")
            .map(|l| l.elements().filter_map(|e| e.attr("FamilyId").map(|f| f.into_owned())).collect())
            .unwrap_or_default()
    };
    assert_eq!(family_links(&mut doc, &d.own_child), std::slice::from_ref(&couple.id));
    assert_eq!(
        family_links(&mut doc, &d.shared_child),
        Vec::<String>::new(),
        "only one parent is partner of the other"
    );

    // home person and last saved person follow
    assert_eq!(tree.home_person.as_deref(), Some(d.keep.as_str()));
    assert_eq!(doc.root.child_text("FamilyProperties/LastSavedPersonId").as_deref(), Some(d.keep.as_str()));
    let _ = &d.register;

    // and it saves and reopens cleanly
    let mut pkg = Package::new();
    pkg.doc = doc;
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert_eq!(edit::check(&reopened.doc), Vec::<String>::new());
}

#[test]
fn merge_can_take_the_other_name_and_gender() {
    let mut d = duplicates();
    merge_people(&mut d.doc, &d.keep, &d.remove, MergeOptions { name_from: Side::Removed, gender_from: Side::Removed })
        .unwrap();
    let tree = Tree::from_doc(&d.doc);
    let anna = tree.person(&d.keep).unwrap();
    assert_eq!(anna.names.iter().filter(|n| n.is_primary).count(), 1);
    // the removed person's primary name is the same words as the kept one: nothing doubled
    assert_eq!(anna.names.iter().filter(|n| n.display() == "Anna Tamm").count(), 1);
    assert_eq!(anna.gender, Gender::Female);

    // really different names: the choice decides which one is primary
    let mut doc = edit::new_family_document();
    let a = edit::add_person(&mut doc, "Mari", "Kask", Gender::Female);
    let b = edit::add_person(&mut doc, "Maria", "Kask-Tamm", Gender::Male);
    merge_people(&mut doc, &a, &b, MergeOptions { name_from: Side::Removed, gender_from: Side::Kept }).unwrap();
    let tree = Tree::from_doc(&doc);
    let p = tree.person(&a).unwrap();
    assert_eq!(p.display_name(), "Maria Kask-Tamm");
    assert_eq!(p.names.len(), 2);
    assert_eq!(p.gender, Gender::Female);
    assert_eq!(edit::check(&doc), Vec::<String>::new());
}

#[test]
fn merge_keeps_unknown_xml_of_links_and_people() {
    let mut d = duplicates();
    let tree = Tree::from_doc(&d.doc);
    let to_partner =
        tree.spouse_links_of(&d.remove).into_iter().find(|l| l.other(&d.remove) == d.partner).unwrap().id.clone();
    let to_other =
        tree.spouse_links_of(&d.remove).into_iter().find(|l| l.other(&d.remove) != d.partner).unwrap().id.clone();
    let mut extra = Element::new("VendorExtra");
    extra.set_text("keep me");
    element_by_id(&mut d.doc, "RelationshipCollection", &to_partner).push(extra.clone());
    element_by_id(&mut d.doc, "RelationshipCollection", &to_other).push(extra.clone());
    element_by_id(&mut d.doc, "PeopleCollection", &d.remove).push(Element::new("VendorPersonExtra"));

    merge_people(&mut d.doc, &d.keep, &d.remove, MergeOptions::default()).unwrap();
    let xml = String::from_utf8(d.doc.to_bytes()).unwrap();
    assert_eq!(xml.matches("<VendorExtra>keep me</VendorExtra>").count(), 2, "on the merged and on the moved couple");
    assert_eq!(xml.matches("<VendorPersonExtra />").count(), 1);
}

#[test]
fn merging_a_couple_or_parent_and_child_drops_the_link_between_them() {
    let mut doc = edit::new_family_document();
    let a = edit::add_person(&mut doc, "Eva", "Paju", Gender::Female);
    let b = edit::add_person(&mut doc, "Eva", "Paju", Gender::Female);
    let c = edit::add_person(&mut doc, "Liina", "Paju", Gender::Female);
    edit::link_spouse(&mut doc, &a, &b).unwrap();
    edit::link_parent(&mut doc, &c, &a).unwrap();
    edit::link_parent(&mut doc, &a, &b).unwrap();
    merge_people(&mut doc, &a, &b, MergeOptions::default()).unwrap();
    let tree = Tree::from_doc(&doc);
    assert_eq!(tree.people.len(), 2);
    assert!(tree.spouse_links.is_empty());
    assert_eq!(tree.parent_links.len(), 1);
    assert_eq!(edit::check(&doc), Vec::<String>::new());

    assert!(merge_people(&mut doc, &a, &a, MergeOptions::default()).is_err());
    assert!(merge_people(&mut doc, &a, "missing", MergeOptions::default()).is_err());
}

#[test]
fn merging_in_the_example_tree_keeps_it_consistent() {
    let mut pkg = Package::open(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/example.familyx")).unwrap();
    let before = Tree::from_doc(&pkg.doc);
    let home = before.home_person.clone().unwrap();
    // pretend the home person's partner is entered twice
    let partner = before.spouses(&home)[0].to_string();
    let twin = edit::add_person(&mut pkg.doc, "Twin", "Record", Gender::Unknown);
    edit::link_spouse(&mut pkg.doc, &home, &twin).unwrap();
    merge_people(&mut pkg.doc, &partner, &twin, MergeOptions::default()).unwrap();
    assert_eq!(edit::check(&pkg.doc), Vec::<String>::new());
    let after = Tree::from_doc(&pkg.doc);
    assert_eq!(after.people.len(), before.people.len());
    assert_eq!(after.spouses(&home), before.spouses(&home));
}

use familyx::edit::{self, FactInput};
use familyx::model::{Date, Gender, Tree};
use familyx::Package;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/example.familyx");

fn open() -> Package {
    Package::open(FIXTURE).unwrap()
}

#[test]
fn fixture_is_consistent() {
    assert_eq!(edit::check(&open().doc), Vec::<String>::new());
}

#[test]
fn add_person_with_facts_survives_save() {
    let mut pkg = open();
    let id = edit::add_person(&mut pkg.doc, "Ada", "Test", Gender::Female);
    let birth = FactInput { date: Date::ymd(1990, 4, 2), place: "Tartu <& co>".into(), details: String::new() };
    let fact_id = edit::add_fact(&mut pkg.doc, &id, "BIRT", &birth).unwrap();
    edit::set_living(&mut pkg.doc, &id, true).unwrap();

    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    let tree = Tree::from_doc(&reopened.doc);
    assert_eq!(tree.people.len(), 18);
    let p = tree.person(&id).unwrap();
    assert_eq!(p.display_name(), "Ada Test");
    assert_eq!(p.gender, Gender::Female);
    assert!(p.is_living);
    let f = p.fact("BIRT").unwrap();
    assert_eq!(f.id, fact_id);
    assert!(f.is_primary);
    assert_eq!(f.date, birth.date);
    assert_eq!(f.place, "Tartu <& co>");

    let mut doc = reopened.doc;
    let cleared = FactInput {
        date: Date { end_year: Some(1995), ..Date::year(1991).with_descriptor("Range") },
        ..Default::default()
    };
    edit::update_fact(&mut doc, &fact_id, &cleared).unwrap();
    let tree = Tree::from_doc(&doc);
    let f = tree.person(&id).unwrap().fact("BIRT").unwrap();
    assert_eq!((f.date.year, f.date.month, f.place.as_str()), (Some(1991), None, ""));
    assert_eq!((f.date.descriptor.as_str(), f.date.end_year), ("Range", Some(1995)));
    let xml = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(xml.contains("<StartYear>1991</StartYear><EndYear>1995</EndYear><DateDescriptor>Range</DateDescriptor>"));
    edit::delete_fact(&mut doc, &fact_id).unwrap();
    assert!(Tree::from_doc(&doc).person(&id).unwrap().fact("BIRT").is_none());
}

#[test]
fn rename_and_gender_change_existing_person() {
    let mut pkg = open();
    let home = Tree::from_doc(&pkg.doc).home_person.unwrap();
    edit::set_name(&mut pkg.doc, &home, "Renamed", "Person").unwrap();
    edit::set_gender(&mut pkg.doc, &home, Gender::Unknown).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let p = tree.person(&home).unwrap();
    assert_eq!(p.display_name(), "Renamed Person");
    assert_eq!(p.gender, Gender::Unknown);
}

#[test]
fn family_links_stay_consistent() {
    let mut pkg = open();
    let doc = &mut pkg.doc;
    let mum = edit::add_person(doc, "Mum", "X", Gender::Female);
    let dad = edit::add_person(doc, "Dad", "X", Gender::Male);
    let kid = edit::add_person(doc, "Kid", "X", Gender::Unknown);

    // parents first, couple afterwards: the couple must pick up the child
    edit::link_parent(doc, &kid, &mum).unwrap();
    edit::link_parent(doc, &kid, &dad).unwrap();
    let couple = edit::link_spouse(doc, &mum, &dad).unwrap();
    assert_eq!(edit::check(doc), Vec::<String>::new());

    let xml = String::from_utf8(doc.to_bytes()).unwrap();
    assert!(xml.contains(&format!("ChildId=\"{kid}\" FamilyId=\"{couple}\"")));

    let tree = Tree::from_doc(doc);
    assert_eq!(tree.parents(&kid).len(), 2);
    assert_eq!(tree.spouses(&mum), vec![dad.as_str()]);

    // linking twice is a no-op; cycles and self-links are rejected
    edit::link_parent(doc, &kid, &mum).unwrap();
    assert_eq!(Tree::from_doc(doc).parents(&kid).len(), 2);
    assert!(edit::link_parent(doc, &mum, &kid).is_err());
    assert!(edit::link_spouse(doc, &kid, &kid).is_err());

    edit::unlink_parent(doc, &kid, &dad).unwrap();
    assert!(!String::from_utf8(doc.to_bytes()).unwrap().contains(&format!("ChildId=\"{kid}\" FamilyId=\"{couple}\"")));
    assert_eq!(edit::check(doc), Vec::<String>::new());

    edit::delete_person(doc, &mum).unwrap();
    let tree = Tree::from_doc(doc);
    assert!(tree.person(&mum).is_none());
    assert!(tree.parents(&kid).is_empty());
    assert!(tree.spouses(&dad).is_empty());
    assert_eq!(edit::check(doc), Vec::<String>::new());
}

#[test]
fn deleting_real_person_keeps_file_consistent() {
    let mut pkg = open();
    let home = Tree::from_doc(&pkg.doc).home_person.unwrap();
    edit::delete_person(&mut pkg.doc, &home).unwrap();
    assert_eq!(edit::check(&pkg.doc), Vec::<String>::new());
    assert_eq!(Tree::from_doc(&pkg.doc).home_person, None);
}

#[test]
fn new_ids_look_like_the_apps() {
    let id = edit::new_id();
    assert_eq!(id.len(), 22);
    assert!(id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

#[test]
fn marriage_fact_on_new_couple() {
    let mut pkg = open();
    let a = edit::add_person(&mut pkg.doc, "A", "X", Gender::Male);
    let b = edit::add_person(&mut pkg.doc, "B", "Y", Gender::Female);
    let couple = edit::link_spouse(&mut pkg.doc, &a, &b).unwrap();
    let input = FactInput {
        date: Date { year: Some(2001), ..Default::default() },
        place: "Pärnu".into(),
        ..Default::default()
    };
    let fact = edit::add_couple_fact(&mut pkg.doc, &couple, "MARR", &input).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let link = tree.spouse_links.iter().find(|l| l.id == couple).unwrap();
    assert_eq!(link.facts.len(), 1);
    assert_eq!(
        (link.facts[0].id.as_str(), link.facts[0].date.year, link.facts[0].place.as_str()),
        (fact.as_str(), Some(2001), "Pärnu")
    );
    edit::delete_fact(&mut pkg.doc, &fact).unwrap();
    assert!(Tree::from_doc(&pkg.doc).spouse_links.iter().find(|l| l.id == couple).unwrap().facts.is_empty());
}

fn find(tree: &Tree, name: &str) -> String {
    tree.people.iter().find(|p| p.display_name() == name).unwrap().id.clone()
}

#[test]
fn notes_on_people_and_facts() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let erik = find(&tree, "Erik Lindqvist");
    let birth = tree.person(&erik).unwrap().fact("BIRT").unwrap().id.clone();
    edit::set_person_note(&mut pkg.doc, &erik, "Loves sailing & <boats>").unwrap();
    edit::set_fact_note(&mut pkg.doc, &birth, "Born at home").unwrap();
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    let tree = Tree::from_doc(&reopened.doc);
    let p = tree.person(&erik).unwrap();
    assert_eq!(p.notes[0].plain_text(), "Loves sailing & <boats>");
    assert_eq!(p.fact("BIRT").unwrap().notes[0].text, "Born at home");
    let mut doc = reopened.doc;
    edit::set_person_note(&mut doc, &erik, "").unwrap();
    assert!(Tree::from_doc(&doc).person(&erik).unwrap().notes.is_empty());
}

#[test]
fn alternate_names_custom_events_and_couple_status() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let liis = find(&tree, "Liis Mets");
    let alt = tree.person(&liis).unwrap().names.iter().find(|n| !n.is_primary).unwrap().id.clone();
    edit::update_name(&mut pkg.doc, &liis, &alt, "Liisa", "Lindqvist", "Married").unwrap();
    let primary = tree.person(&liis).unwrap().primary_name().unwrap().id.clone();
    assert!(edit::remove_name(&mut pkg.doc, &liis, &primary).is_err());

    let fid = edit::add_custom_fact(&mut pkg.doc, &liis, "Emigrated", &FactInput::default()).unwrap();
    let karl = find(&tree, "Karl Lindqvist");
    let couple = tree.spouse_links.iter().find(|l| l.other(&karl) == liis).unwrap().id.clone();
    edit::set_couple_status(&mut pkg.doc, &couple, "Deceased").unwrap();

    let tree = Tree::from_doc(&pkg.doc);
    let p = tree.person(&liis).unwrap();
    assert!(p.names.iter().any(|n| n.given == "Liisa" && n.name_type == "Married"));
    assert_eq!(p.facts.iter().find(|f| f.id == fid).unwrap().label(), "Emigrated");
    assert_eq!(tree.spouse_links.iter().find(|l| l.id == couple).unwrap().status, "Deceased");
    edit::remove_name(&mut pkg.doc, &liis, &alt).unwrap();
    assert_eq!(Tree::from_doc(&pkg.doc).person(&liis).unwrap().names.len(), 1);
}

#[test]
fn reorder_children() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let erik = find(&tree, "Erik Lindqvist");
    let kids: Vec<String> = tree.children(&erik).iter().map(|s| s.to_string()).collect();
    assert_eq!(kids.len(), 2);
    assert!(edit::move_child(&mut pkg.doc, &erik, &kids[1], -1).unwrap());
    assert!(!edit::move_child(&mut pkg.doc, &erik, &kids[1], -1).unwrap());
    let tree = Tree::from_doc(&pkg.doc);
    assert_eq!(tree.children(&erik), vec![kids[1].as_str(), kids[0].as_str()]);
    assert_eq!(edit::check(&pkg.doc), Vec::<String>::new());
}

#[test]
fn photos_can_be_shared_switched_and_removed() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let erik = find(&tree, "Erik Lindqvist");
    let leo = find(&tree, "Leo Lindqvist");
    let first = tree.person(&erik).unwrap().primary_image.clone().unwrap();
    let second = pkg.add_photo(&erik, vec![1, 2, 3], "png", "second.png").unwrap();
    edit::set_primary_photo(&mut pkg.doc, &erik, &first).unwrap();
    pkg.share_photo(&leo, &second).unwrap();

    // still used by Leo: file stays
    pkg.remove_photo(&erik, &second).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let part = tree.media[&second].part.clone();
    assert!(pkg.part(&part).is_some());
    assert_eq!(tree.person(&erik).unwrap().primary_image.as_deref(), Some(first.as_str()));

    // last user gone: file, PackagedFile and relationship are removed
    pkg.remove_photo(&leo, &second).unwrap();
    assert!(pkg.part(&part).is_none());
    assert!(!Tree::from_doc(&pkg.doc).media.contains_key(&second));
    let rels = String::from_utf8(pkg.part("_rels/.rels").unwrap().to_vec()).unwrap();
    assert!(!rels.contains(&part));
    assert!(Tree::from_doc(&pkg.doc).person(&leo).unwrap().primary_image.is_none());
}

fn serialized(pkg: &Package) -> String {
    String::from_utf8_lossy(&pkg.doc.to_bytes()).into_owned()
}

fn names_of(el: &familyx::doc::Element) -> Vec<&str> {
    el.elements().map(|e| e.name.as_str()).collect()
}

#[test]
fn sources_and_citations_roundtrip_with_schema_order() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let (sources_before, repos_before) = (tree.sources.len(), tree.repositories.len());
    let erik = find(&tree, "Erik Lindqvist");
    let birth = tree.person(&erik).unwrap().fact("BIRT").unwrap().id.clone();

    let repo = edit::find_or_add_repository(&mut pkg.doc, "Test Archive").unwrap();
    assert_eq!(edit::find_or_add_repository(&mut pkg.doc, "Test Archive").unwrap(), repo);
    let source = edit::add_source(
        &mut pkg.doc,
        &edit::SourceInput {
            title: "Parish register <1890>".into(),
            author: "Parish clerk".into(),
            publisher: "Printed 1891".into(),
            note: "Water damaged".into(),
            repository: Some(repo.clone()),
            call_number: "KA 12/3".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let cit = edit::add_citation(&mut pkg.doc, &birth, &source, " p. 42 ").unwrap();
    let person_cit = edit::add_citation(&mut pkg.doc, &erik, &source, "").unwrap();

    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    let doc = &reopened.doc;

    // source: element order and record attributes
    let s = doc
        .root
        .child("SourceCollection")
        .unwrap()
        .children_named("Source")
        .find(|s| s.attr("Id").as_deref() == Some(source.as_str()))
        .unwrap();
    assert!(s.attr("LastSaved").is_some() && s.attr("Creation").is_some());
    assert_eq!(names_of(s), ["Title", "Author", "Publisher", "Repositories", "Notes"]);
    let entry = s.path("Repositories/SourceRepositoryCitation").unwrap();
    assert!(entry.attr("Id").is_some());
    assert_eq!(names_of(entry), ["CallNumber", "Repository"]);

    // fact: citations sit between the age data and the multimedia list
    let fact = doc
        .root
        .child("PeopleCollection")
        .unwrap()
        .elements()
        .flat_map(|p| p.path("Facts").into_iter().flat_map(|f| f.children_named("Fact")))
        .find(|f| f.attr("Id").as_deref() == Some(birth.as_str()))
        .unwrap();
    let names = names_of(fact);
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(at("AgeAtEvent") < at("FactCitationCollection"));
    assert!(names.iter().position(|x| *x == "Multimedia").is_none_or(|m| at("FactCitationCollection") < m));
    let coll = fact.child("FactCitationCollection").unwrap();
    assert_eq!(names_of(coll), ["Citations", "Index"]);
    assert_eq!(coll.child_text("Index").as_deref(), Some("0"));
    let citation = coll.path("Citations/Citation").unwrap();
    assert!(citation.attr("Id").is_some() && citation.attr("Creation").is_some());
    assert_eq!(names_of(citation), ["Source", "Page"]);

    // person: citations between the family links and the portrait
    let person = doc
        .root
        .child("PeopleCollection")
        .unwrap()
        .children_named("Person")
        .find(|p| p.attr("Id").as_deref() == Some(erik.as_str()))
        .unwrap();
    let names = names_of(person);
    let at = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(at("FamilySpouseLinks") < at("PersonCitationCollection"));
    assert!(at("PersonCitationCollection") < at("PrimaryImageId"));
    assert_eq!(names_of(person.path("PersonCitationCollection/Citations/Citation").unwrap()), ["Source"]);

    // the typed view
    let tree = Tree::from_doc(doc);
    assert_eq!(tree.sources.len(), sources_before + 1);
    assert_eq!(tree.repositories.len(), repos_before + 1);
    let s = tree.source(&source).unwrap();
    assert_eq!(s.title, "Parish register <1890>");
    assert_eq!((s.author.as_str(), s.publisher.as_str()), ("Parish clerk", "Printed 1891"));
    assert_eq!(s.repository.as_deref(), Some(repo.as_str()));
    assert_eq!(s.call_number, "KA 12/3");
    assert_eq!(s.notes[0].text, "Water damaged");
    let p = tree.person(&erik).unwrap();
    let fact_cit = &p.fact("BIRT").unwrap().citations[0];
    assert_eq!(
        (fact_cit.id.as_str(), fact_cit.source.as_str(), fact_cit.page.as_str()),
        (cit.as_str(), source.as_str(), "p. 42")
    );
    assert_eq!(p.citations[0].id, person_cit);
    assert_eq!(edit::check(doc), Vec::<String>::new());
}

#[test]
fn updating_and_deleting_sources_and_citations() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let erik = find(&tree, "Erik Lindqvist");
    let birth = tree.person(&erik).unwrap().fact("BIRT").unwrap().id.clone();
    let input = |title: &str| edit::SourceInput { title: title.into(), ..Default::default() };
    let source = edit::add_source(&mut pkg.doc, &input("A")).unwrap();
    let other = edit::add_source(&mut pkg.doc, &input("B")).unwrap();
    let c1 = edit::add_citation(&mut pkg.doc, &birth, &source, "1").unwrap();
    edit::add_citation(&mut pkg.doc, &erik, &source, "2").unwrap();
    let c3 = edit::add_citation(&mut pkg.doc, &birth, &other, "3").unwrap();

    edit::update_source(&mut pkg.doc, &source, &edit::SourceInput { author: "Z".into(), ..input("A2") }).unwrap();
    edit::set_citation_page(&mut pkg.doc, &c1, "11").unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    assert_eq!(
        (tree.source(&source).unwrap().title.as_str(), tree.source(&source).unwrap().author.as_str()),
        ("A2", "Z")
    );
    assert_eq!(tree.person(&erik).unwrap().fact("BIRT").unwrap().citations[0].page, "11");

    // deleting a source drops its citations everywhere, and empty collections
    edit::delete_source(&mut pkg.doc, &source).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    assert!(tree.source(&source).is_none() && tree.source(&other).is_some());
    let p = tree.person(&erik).unwrap();
    assert!(p.citations.is_empty());
    assert_eq!(p.fact("BIRT").unwrap().citations.len(), 1);
    assert!(!serialized(&pkg).contains("PersonCitationCollection"));
    assert_eq!(edit::check(&pkg.doc), Vec::<String>::new());

    edit::remove_citation(&mut pkg.doc, &c3).unwrap();
    assert!(Tree::from_doc(&pkg.doc).person(&erik).unwrap().fact("BIRT").unwrap().citations.is_empty());
    assert!(!serialized(&pkg).contains("FactCitationCollection"));
    assert!(edit::remove_citation(&mut pkg.doc, &c3).is_err());
    assert!(edit::add_citation(&mut pkg.doc, &birth, "nope", "").is_err());
    assert!(edit::delete_source(&mut pkg.doc, "nope").is_err());
}

#[test]
fn check_reports_citations_to_missing_sources() {
    let mut pkg = open();
    let tree = Tree::from_doc(&pkg.doc);
    let erik = find(&tree, "Erik Lindqvist");
    let before = edit::check(&pkg.doc);
    let source =
        edit::add_source(&mut pkg.doc, &edit::SourceInput { title: "A".into(), ..Default::default() }).unwrap();
    edit::add_citation(&mut pkg.doc, &erik, &source, "").unwrap();
    // remove the source record only, leaving the citation dangling
    pkg.doc
        .root
        .child_mut("SourceCollection")
        .unwrap()
        .remove_children_where(|s| s.attr("Id").as_deref() == Some(source.as_str()));
    let problems = edit::check(&pkg.doc);
    assert_eq!(problems.len(), before.len() + 1, "{problems:?}");
    assert!(problems.iter().any(|p| p.contains("missing source")));
}

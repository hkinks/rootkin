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
    let birth = FactInput {
        date: Date { year: Some(1990), month: Some(4), day: Some(2), descriptor: "Span".into() },
        place: "Tartu <& co>".into(),
        details: String::new(),
    };
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
    let cleared = FactInput { date: Date { year: Some(1991), ..Default::default() }, ..Default::default() };
    edit::update_fact(&mut doc, &fact_id, &cleared).unwrap();
    let tree = Tree::from_doc(&doc);
    let f = tree.person(&id).unwrap().fact("BIRT").unwrap();
    assert_eq!((f.date.year, f.date.month, f.place.as_str()), (Some(1991), None, ""));
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
    let input = FactInput { date: Date { year: Some(2001), ..Default::default() }, place: "Pärnu".into(), ..Default::default() };
    let fact = edit::add_couple_fact(&mut pkg.doc, &couple, "MARR", &input).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    let link = tree.spouse_links.iter().find(|l| l.id == couple).unwrap();
    assert_eq!(link.facts.len(), 1);
    assert_eq!((link.facts[0].id.as_str(), link.facts[0].date.year, link.facts[0].place.as_str()), (fact.as_str(), Some(2001), "Pärnu"));
    edit::delete_fact(&mut pkg.doc, &fact).unwrap();
    assert!(Tree::from_doc(&pkg.doc).spouse_links.iter().find(|l| l.id == couple).unwrap().facts.is_empty());
}

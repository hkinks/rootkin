//! Godparents, witnesses and sponsors (`Association`) and the person's lists survive merges and deletes.

use familyx::doc::{Document, Element};
use familyx::edit::{self, FactInput};
use familyx::merge::{merge_people, MergeOptions};
use familyx::model::{Gender, Tree};

fn person<'a>(doc: &'a mut Document, id: &str) -> &'a mut Element {
    doc.root
        .child_mut("PeopleCollection")
        .unwrap()
        .elements_mut()
        .find(|p| p.attr("Id").as_deref() == Some(id))
        .unwrap()
}

fn text(name: &str, value: &str) -> Element {
    let mut e = Element::new(name);
    e.set_text(value);
    e
}

fn association(who: &str, role: &str) -> Element {
    let mut a = Element::new("Association").with_attr("Id", &edit::new_id());
    a.push(text("PersonId", who));
    a.push(text("Role", role));
    a
}

/// Add a godparent `who` to `on`'s birth.
fn godparent(doc: &mut Document, on: &str, who: &str) {
    let birth = edit::add_fact(doc, on, "BIRT", &FactInput::default()).unwrap();
    fn find<'a>(el: &'a mut Element, id: &str) -> Option<&'a mut Element> {
        if el.name == "Fact" && el.attr("Id").as_deref() == Some(id) {
            return Some(el);
        }
        el.elements_mut().find_map(|c| find(c, id))
    }
    find(&mut doc.root, &birth).unwrap().ensure_child("Associations", &[]).push(association(who, "Godparent"));
}

fn xml(doc: &Document) -> String {
    String::from_utf8(doc.to_bytes()).unwrap()
}

#[test]
fn merge_repoints_associations_and_check_sees_them() {
    let mut doc = edit::new_family_document();
    let child = edit::add_person(&mut doc, "Kid", "Test", Gender::Male);
    let god1 = edit::add_person(&mut doc, "God", "Parent", Gender::Male);
    let god2 = edit::add_person(&mut doc, "God", "Parent", Gender::Male);
    godparent(&mut doc, &child, &god2);
    merge_people(&mut doc, &god1, &god2, MergeOptions::default()).unwrap();
    assert!(xml(&doc).contains(&format!("<PersonId>{god1}</PersonId>")));
    assert!(!xml(&doc).contains(&god2));
    assert!(edit::check(&doc).is_empty(), "{:?}", edit::check(&doc));

    // a dangling association is reported
    godparent(&mut doc, &child, "no-such-person");
    assert!(edit::check(&doc).iter().any(|p| p.contains("association") && p.contains("no-such-person")));
}

#[test]
fn delete_keeps_the_name_of_a_godparent_but_not_the_link() {
    let mut doc = edit::new_family_document();
    let child = edit::add_person(&mut doc, "Kid", "Test", Gender::Male);
    let god = edit::add_person(&mut doc, "Mari", "Kask", Gender::Female);
    godparent(&mut doc, &child, &god);
    edit::delete_person(&mut doc, &god).unwrap();
    let out = xml(&doc);
    assert!(!out.contains(&god), "no link to the deleted person");
    assert!(out.contains("<PersonName>Mari Kask</PersonName>"), "{out}");
    assert!(out.contains("<Role>Godparent</Role>"));
    assert!(edit::check(&doc).is_empty());
}

#[test]
fn merge_combines_lists_entry_by_entry() {
    let mut doc = edit::new_family_document();
    let keep = edit::add_person(&mut doc, "Ann", "Test", Gender::Female);
    let gone = edit::add_person(&mut doc, "Ann", "Test", Gender::Female);
    let a = edit::add_person(&mut doc, "Witness", "A", Gender::Male);
    let b = edit::add_person(&mut doc, "Witness", "B", Gender::Male);
    for (id, tag, haplo) in [(&keep, "t1", "H1"), (&gone, "t2", "U5")] {
        let p = person(&mut doc, id);
        let mut tags = Element::new("Tags");
        tags.push(text("string", tag));
        tags.push(text("string", "shared"));
        p.push(tags);
        let mut dna = Element::new("MtDNA");
        let mut entry = Element::new("MtDNA").with_attr("Id", &edit::new_id());
        entry.push(text("Haplogroup", haplo));
        dna.push(entry);
        p.child_mut("Biology").unwrap().push(dna);
    }
    let keep_assoc = association(&a, "Witness");
    let mut same_again = keep_assoc.clone();
    same_again.set_attr("Id", &edit::new_id());
    let mut assocs = Element::new("Associations");
    assocs.push(keep_assoc);
    person(&mut doc, &keep).push(assocs);
    let mut assocs = Element::new("Associations");
    assocs.push(same_again);
    assocs.push(association(&b, "Witness"));
    assocs.push(association(&keep, "Friend")); // would be an association with oneself
    person(&mut doc, &gone).push(assocs);

    merge_people(&mut doc, &keep, &gone, MergeOptions::default()).unwrap();
    let p = person(&mut doc, &keep).clone();
    let strings = |path: &str| -> Vec<String> {
        p.path(path).into_iter().flat_map(|l| l.elements()).map(|e| e.text()).collect::<Vec<_>>()
    };
    assert_eq!(strings("Tags"), ["t1", "shared", "t2"]);
    let haplos: Vec<String> =
        p.path("Biology/MtDNA").unwrap().elements().map(|e| e.child_text("Haplogroup").unwrap()).collect();
    assert_eq!(haplos, ["H1", "U5"], "DNA of both is kept");
    let who: Vec<String> =
        p.child("Associations").unwrap().elements().map(|e| e.child_text("PersonId").unwrap()).collect();
    assert_eq!(who, [a.clone(), b.clone()]);
    assert_eq!(Tree::from_doc(&doc).person(&keep).unwrap().gender, Gender::Female);
    assert!(edit::check(&doc).is_empty());
}

#[test]
fn merge_of_couples_keeps_notes_and_photos_of_both() {
    let mut doc = edit::new_family_document();
    let partner = edit::add_person(&mut doc, "Partner", "P", Gender::Male);
    let keep = edit::add_person(&mut doc, "Ann", "Test", Gender::Female);
    let gone = edit::add_person(&mut doc, "Ann", "Test", Gender::Female);
    let c1 = edit::link_spouse(&mut doc, &partner, &keep).unwrap();
    let c2 = edit::link_spouse(&mut doc, &partner, &gone).unwrap();
    for (couple, note, photo) in [(&c1, "Met in Tartu", "m1"), (&c2, "Married in Riga", "m2")] {
        let link = doc
            .root
            .child_mut("RelationshipCollection")
            .unwrap()
            .elements_mut()
            .find(|l| l.attr("Id").as_deref() == Some(couple.as_str()))
            .unwrap();
        let mut notes = Element::new("Notes");
        let mut list = Element::new("Notes");
        let mut n = Element::new("Note").with_attr("Id", &edit::new_id());
        n.push(text("Text", note));
        list.push(n);
        notes.push(list);
        notes.push(text("Index", "0"));
        link.push(notes);
        let mut media = Element::new("Multimedia");
        media.push(text("string", photo));
        link.push(media);
    }
    merge_people(&mut doc, &keep, &gone, MergeOptions::default()).unwrap();
    let out = xml(&doc);
    for kept in ["Met in Tartu", "Married in Riga", ">m1<", ">m2<"] {
        assert!(out.contains(kept), "{kept} lost");
    }
    assert_eq!(Tree::from_doc(&doc).spouse_links.len(), 1);
}

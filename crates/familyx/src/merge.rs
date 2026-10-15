//! Merging two records of the same person into one.
//!
//! One person is kept, the other removed. Everything the removed person has
//! is added to the kept one (events, alternate names, notes, citations,
//! photos), every family link moves to the kept person without creating
//! doubles, and the removed person is deleted. See "Merging people" in
//! docs/design.md for how each element is handled.

use std::collections::{HashMap, HashSet};

use crate::doc::{Document, Element, Node};
use crate::duplicates::{fold, name_key};
use crate::edit::{self, EditError, EditResult, CITATION_COLLECTION_ORDER, FACT_ORDER, NAME_ORDER, PERSON_ORDER};
use crate::model::{Gender, Id, Tree};

/// Which of the two people a choice comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Side {
    /// The person who stays.
    #[default]
    Kept,
    /// The person who is merged in and removed.
    Removed,
}

/// Choices for [`merge_people`]. The defaults keep the kept person's name and gender.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MergeOptions {
    /// Whose primary name becomes the primary name; the other's becomes an alternate name.
    pub name_from: Side,
    /// Whose gender is used. An unknown gender never overrules a known one.
    pub gender_from: Side,
}

/// Merge `remove` into `keep` and delete `remove`. One call is one logical edit:
/// on an error the document may be half changed, so callers work on a snapshot
/// (the app's undo does).
pub fn merge_people(doc: &mut Document, keep: &str, remove: &str, opts: MergeOptions) -> EditResult<()> {
    edit::validate_pair(doc, keep, remove)?;
    if would_be_own_ancestor(&Tree::from_doc(doc), keep, remove) {
        return Err(EditError::Invalid(
            "one of the two descends from the other, so merging them would make the person their own ancestor",
        ));
    }
    merge_links(doc, keep, remove)?;
    // godparents, witnesses, sponsors: on any person, fact or couple
    edit::repoint_associations(doc, remove, keep);
    merge_data(doc, keep, remove, opts)?;
    if let Some(props) = doc.root.child_mut("FamilyProperties") {
        for field in ["HomePersonId", "LastSavedPersonId"] {
            if let Some(e) = props.child_mut(field) {
                if e.text() == remove {
                    e.set_text(keep);
                }
            }
        }
    }
    edit::delete_person(doc, remove)?;
    attach_children_to_couples(doc, keep)?;
    doc.forget_index();
    Ok(())
}

/// Whether the merged person would descend from themselves. A direct parent link between the two
/// is dropped by the merge, so only longer paths count (e.g. merging a grandparent with a grandchild).
fn would_be_own_ancestor(tree: &Tree, keep: &str, remove: &str) -> bool {
    let merged = |id: &str| if id == remove { keep.to_string() } else { id.to_string() };
    let between = |a: &str, b: &str| (a == keep && b == remove) || (a == remove && b == keep);
    let children = |p: &str| -> Vec<String> {
        let mut out = Vec::new();
        for from in if p == keep { vec![keep, remove] } else { vec![p] } {
            out.extend(tree.child_links_of(from).filter(|l| !between(from, &l.child)).map(|l| merged(&l.child)));
        }
        out
    };
    let mut seen = HashSet::new();
    let mut stack = children(keep);
    while let Some(c) = stack.pop() {
        if c == keep {
            return true;
        }
        if seen.insert(c.clone()) {
            stack.extend(children(&c));
        }
    }
    false
}

// ---------------------------------------------------------------- links

fn involves(l: &crate::model::SpouseLink, id: &str) -> bool {
    l.person1 == id || l.person2 == id
}

fn merge_links(doc: &mut Document, keep: &str, remove: &str) -> EditResult<()> {
    // links between the two people themselves would become links to oneself
    let tree = Tree::from_doc(doc);
    for l in tree.spouse_links.iter().filter(|l| involves(l, keep) && involves(l, remove)) {
        edit::remove_spouse_link(doc, &l.id);
    }
    for l in tree
        .parent_links
        .iter()
        .filter(|l| (l.parent == keep && l.child == remove) || (l.parent == remove && l.child == keep))
    {
        edit::remove_relationship(doc, &l.id);
    }

    let tree = Tree::from_doc(doc);
    let mut delete: HashSet<Id> = HashSet::new();
    let mut repoint: HashMap<Id, &'static str> = HashMap::new();
    let mut modifiers: HashMap<Id, String> = HashMap::new();
    // removed couple -> the kept person's couple with the same partner
    let mut redirect: HashMap<Id, Id> = HashMap::new();

    for l in tree.parent_links.iter().filter(|l| l.parent == remove || l.child == remove) {
        let (attr, existing) = if l.parent == remove {
            ("Person1Id", tree.parent_links.iter().find(|e| e.parent == keep && e.child == l.child))
        } else {
            ("Person2Id", tree.parent_links.iter().find(|e| e.parent == l.parent && e.child == keep))
        };
        match existing {
            Some(e) => {
                delete.insert(l.id.clone());
                if e.modifier.is_empty() && !l.modifier.is_empty() {
                    modifiers.insert(e.id.clone(), l.modifier.clone());
                }
            }
            None => {
                repoint.insert(l.id.clone(), attr);
            }
        }
    }
    for l in tree.spouse_links.iter().filter(|l| involves(l, remove)) {
        let partner = l.other(remove);
        let existing = tree.spouse_links.iter().find(|e| e.id != l.id && involves(e, keep) && e.other(keep) == partner);
        match existing {
            Some(e) => {
                redirect.insert(l.id.clone(), e.id.clone());
            }
            None => {
                repoint.insert(l.id.clone(), if l.person1 == remove { "Person1Id" } else { "Person2Id" });
            }
        }
    }

    let rel = edit::collection(doc, "RelationshipCollection");
    let removed_couples: HashMap<Id, Element> = rel
        .elements()
        .filter_map(|e| {
            let id = e.attr("Id")?.into_owned();
            redirect.contains_key(&id).then(|| (id, e.clone()))
        })
        .collect();
    for el in rel.elements_mut() {
        let id = el.attr("Id").map(|i| i.into_owned()).unwrap_or_default();
        if let Some(attr) = repoint.get(&id) {
            el.set_attr(attr, keep);
        }
        if let Some(m) = modifiers.get(&id) {
            el.ensure_child("ParentChildModifier", &["CustomParentChildModifier", "ParentChildModifier"]).set_text(m);
        }
        if let Some(from) = redirect.iter().find(|(_, to)| **to == id).and_then(|(from, _)| removed_couples.get(from)) {
            merge_couple(el, from);
        }
    }
    rel.remove_children_where(|e| {
        let id = e.attr("Id").map(|i| i.into_owned()).unwrap_or_default();
        delete.contains(&id) || redirect.contains_key(&id)
    });

    // the back-references on people
    let people = edit::collection(doc, "PeopleCollection");
    let mut moved: Vec<(&str, Element)> = Vec::new();
    for p in people.elements_mut() {
        let is_removed = p.attr("Id").as_deref() == Some(remove);
        for list in ["FamilySpouseLinks", "FamilyChildLinks"] {
            let Some(l) = p.child_mut(list) else { continue };
            for e in l.elements_mut() {
                if let Some(to) = e.attr("FamilyId").and_then(|f| redirect.get(f.as_ref()).cloned()) {
                    e.set_attr("FamilyId", &to);
                }
            }
            dedupe_families(l);
            if is_removed {
                moved.extend(l.elements().map(|e| (list, e.clone())));
                l.children.clear();
            }
        }
    }
    if let Some(k) = people.elements_mut().find(|p| p.attr("Id").as_deref() == Some(keep)) {
        for (list, mut entry) in moved {
            entry.set_attr(if list == "FamilySpouseLinks" { "SpouseId" } else { "ChildId" }, keep);
            let l = k.ensure_child(list, PERSON_ORDER);
            let family = entry.attr("FamilyId").map(|f| f.into_owned());
            if !l.elements().any(|e| e.attr("FamilyId").map(|f| f.into_owned()) == family) {
                l.push(entry);
            }
        }
    }
    doc.forget_index();
    Ok(())
}

fn dedupe_families(list: &mut Element) {
    let mut seen = HashSet::new();
    list.children.retain(|n| match n {
        Node::Element(e) => e.attr("FamilyId").is_none_or(|f| seen.insert(f.into_owned())),
        _ => true,
    });
}

/// Add what the removed couple link knows to the kept person's link with the same partner.
fn merge_couple(dst: &mut Element, from: &Element) {
    const ORDER: &[&str] = &["Facts", "SpouseRelationshipStatus", "NumberOfChildren"];
    if let Some(gf) = from.child("Facts") {
        let facts = dst.ensure_child("Facts", ORDER);
        merge_facts(facts, gf);
    }
    if dst.child_text("SpouseRelationshipStatus").is_none_or(|s| s.is_empty() || s == "Blank") {
        if let Some(s) = from.child_text("SpouseRelationshipStatus").filter(|s| !s.is_empty() && s != "Blank") {
            dst.ensure_child("SpouseRelationshipStatus", ORDER).set_text(&s);
        }
    }
    merge_notes(dst, &[], from);
    for c in from.elements().filter(|c| !ORDER.contains(&c.name.as_str()) && c.name != "Notes") {
        match dst.child_mut(&c.name) {
            None => {
                dst.push(c.clone());
            }
            Some(d) if LISTS.contains(&c.name.as_str()) => merge_entries(d, c),
            Some(_) => {}
        }
    }
}

/// Lists merged entry by entry (on people and couples); other elements are taken only when missing.
const LISTS: &[&str] = &["Associations", "Tags", "CustomIds", "Multimedia", "LDSFacts", "YDNA", "MtDNA"];

/// Append the entries of `from` that `dst` doesn't have yet. Entries that differ only in their
/// record id and timestamps count as the same.
fn merge_entries(dst: &mut Element, from: &Element) {
    for e in from.elements() {
        if !dst.elements().any(|d| same_entry(d, e)) {
            dst.push(e.clone());
        }
    }
}

fn same_entry(a: &Element, b: &Element) -> bool {
    let attrs = |e: &Element| -> Vec<(String, String)> {
        e.attrs.iter().filter(|(k, _)| !matches!(k.as_str(), "Id" | "LastSaved" | "Creation")).cloned().collect()
    };
    a.name == b.name && attrs(a) == attrs(b) && a.children == b.children
}

/// After the merge, children of the kept person who have both members of a couple as parents
/// belong to that couple, as `edit::link_parent` arranges for new links.
fn attach_children_to_couples(doc: &mut Document, keep: &str) -> EditResult<()> {
    let tree = Tree::from_doc(doc);
    let mut people = vec![keep];
    people.extend(tree.children(keep));
    for child in people {
        let parents = tree.parents(child);
        for (i, a) in parents.iter().enumerate() {
            for b in &parents[i + 1..] {
                let couple = tree.spouse_links.iter().find(|l| involves(l, a) && involves(l, b));
                if let Some(couple) = couple {
                    edit::add_family_child_link(doc, child, &couple.id)?;
                }
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- the person's own data

/// Same event: tag, kind, date, place and details all agree.
fn fact_signature(f: &Element) -> String {
    let date = |name: &str| f.child_text(&format!("FactDate/{name}")).unwrap_or_default();
    let descriptor = date("DateDescriptor");
    let descriptor = if descriptor == "Exact" { String::new() } else { descriptor };
    [
        f.child_text("Tag").unwrap_or_default(),
        f.child_text("CustomFactTag").unwrap_or_default(),
        date("StartYear"),
        date("StartMonth"),
        date("StartDay"),
        date("EndYear"),
        date("EndMonth"),
        date("EndDay"),
        descriptor,
        date("Text").trim().to_string(),
        fold(&f.child_text("PlaceDetails/Description").unwrap_or_default())
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" "),
        f.child_text("Details").unwrap_or_default().trim().to_string(),
    ]
    .join("\u{1f}")
}

/// Add the facts of `from` (a `Facts` element) to `facts`. An event that is already there
/// only gains the other's citations and notes. A second birth, death, marriage or
/// occupation is added without the "primary" flag, so the first one stays the main one.
fn merge_facts(facts: &mut Element, from: &Element) {
    let mut by_signature: HashMap<String, usize> = HashMap::new();
    for (i, n) in facts.children.iter().enumerate() {
        if let Node::Element(f) = n {
            by_signature.entry(fact_signature(f)).or_insert(i);
        }
    }
    for g in from.children_named("Fact") {
        let signature = fact_signature(g);
        if let Some(&i) = by_signature.get(&signature) {
            if let Node::Element(dst) = &mut facts.children[i] {
                merge_citations(dst, "FactCitationCollection", FACT_ORDER, g);
                merge_notes(dst, FACT_ORDER, g);
            }
            continue;
        }
        let mut fact = g.clone();
        let tag = g.child_text("Tag").unwrap_or_default();
        if edit::takes_primary(&tag)
            && facts.children_named("Fact").any(|f| f.child_text("Tag").as_deref() == Some(tag.as_str()))
        {
            fact.remove_children_where(|e| e.name == "IsPrimary");
        }
        facts.push(fact);
        by_signature.insert(signature, facts.children.len() - 1);
    }
}

fn merge_citations(dst: &mut Element, collection: &str, order: &[&str], from: &Element) {
    let key = |c: &Element| (c.child_text("Source").unwrap_or_default(), c.child_text("Page").unwrap_or_default());
    let mut have: HashSet<(String, String)> = dst
        .path(&format!("{collection}/Citations"))
        .into_iter()
        .flat_map(|l| l.children_named("Citation"))
        .map(key)
        .collect();
    let new: Vec<Element> = from
        .path(&format!("{collection}/Citations"))
        .into_iter()
        .flat_map(|l| l.children_named("Citation"))
        .filter(|c| have.insert(key(c)))
        .cloned()
        .collect();
    if new.is_empty() {
        return;
    }
    let wrapper = dst.ensure_child(collection, order);
    if wrapper.child("Index").is_none() {
        wrapper.ensure_child("Index", CITATION_COLLECTION_ORDER).set_text("0");
    }
    let list = wrapper.ensure_child("Citations", CITATION_COLLECTION_ORDER);
    for c in new {
        list.push(c);
    }
}

fn merge_notes(dst: &mut Element, order: &[&str], from: &Element) {
    let text = |n: &Element| n.child_text("Text").unwrap_or_default().trim().to_string();
    let mut have: HashSet<String> =
        dst.path("Notes/Notes").into_iter().flat_map(|l| l.children_named("Note")).map(text).collect();
    let new: Vec<Element> = from
        .path("Notes/Notes")
        .into_iter()
        .flat_map(|l| l.children_named("Note"))
        .filter(|n| !text(n).is_empty() && have.insert(text(n)))
        .cloned()
        .collect();
    if new.is_empty() {
        return;
    }
    let notes = dst.ensure_child("Notes", order);
    let list = notes.ensure_child("Notes", &["Notes", "Index"]);
    for n in new {
        list.push(n);
    }
    if notes.child("Index").is_none() {
        notes.ensure_child("Index", &["Notes", "Index"]).set_text("0");
    }
}

fn is_real_name(n: &Element) -> bool {
    ["Names", "Surname", "Nickname"].iter().any(|f| n.child_text(f).is_some_and(|t| !t.is_empty()))
}

/// A name's identity: the words of the name, suffix and nickname, folded.
fn full_name_key(n: &Element) -> (String, String, String) {
    let text = |f: &str| n.child_text(f).unwrap_or_default();
    (
        name_key(&text("Names"), &text("SurnamePrefix"), &text("Surname")),
        fold(&text("Suffix")).trim().to_string(),
        fold(&text("Nickname")).trim().to_string(),
    )
}

fn primary_index(names: &[&Element]) -> Option<usize> {
    names.iter().position(|n| n.child_text("IsPrimary").as_deref() == Some("true")).or(if names.is_empty() {
        None
    } else {
        Some(0)
    })
}

/// Children of a person this module handles itself.
const HANDLED: &[&str] = &[
    "PersonNameCollection",
    "Biology",
    "FamilyChildLinks",
    "FamilySpouseLinks",
    "PersonCitationCollection",
    "PrimaryImageId",
    "Face",
    "IsLiving",
    "Facts",
    "Multimedia",
    "Notes",
];

fn merge_data(doc: &mut Document, keep: &str, remove: &str, opts: MergeOptions) -> EditResult<()> {
    let tree = Tree::from_doc(doc);
    let keep_p = tree.person(keep).ok_or_else(|| EditError::NoPerson(keep.to_string()))?;
    let gone_p = tree.person(remove).ok_or_else(|| EditError::NoPerson(remove.to_string()))?;
    let gender = match (opts.gender_from, keep_p.gender, gone_p.gender) {
        (Side::Kept, Gender::Unknown, other) | (Side::Removed, other, Gender::Unknown) => other,
        (Side::Kept, own, _) => own,
        (Side::Removed, _, other) => other,
    };
    let gone_died = gone_p.fact("DEAT").is_some();
    let gone: Element = doc
        .root
        .child("PeopleCollection")
        .and_then(|c| c.elements().find(|p| p.attr("Id").as_deref() == Some(remove)))
        .cloned()
        .ok_or_else(|| EditError::NoPerson(remove.to_string()))?;
    let el = edit::person_mut(doc, keep)?;

    // names
    let gone_names: Vec<&Element> = gone
        .path("PersonNameCollection/Names")
        .into_iter()
        .flat_map(Element::elements)
        .filter(|n| is_real_name(n))
        .collect();
    if !gone_names.is_empty() {
        let names = el.ensure_child("PersonNameCollection", PERSON_ORDER).ensure_child("Names", &[]);
        let gone_primary = primary_index(&gone_names);
        let promote = opts.name_from == Side::Removed && gone_primary.is_some();
        if promote {
            for n in names.elements_mut() {
                n.remove_children_where(|c| c.name == "IsPrimary");
            }
        }
        for (i, g) in gone_names.iter().enumerate() {
            let primary = promote && Some(i) == gone_primary;
            let key = full_name_key(g);
            if let Some(same) = names.elements_mut().find(|n| is_real_name(n) && full_name_key(n) == key) {
                if primary {
                    same.ensure_child("IsPrimary", NAME_ORDER).set_text("true");
                }
                continue;
            }
            let mut name = (*g).clone();
            name.remove_children_where(|e| e.name == "IsPrimary");
            if primary {
                name.ensure_child("IsPrimary", NAME_ORDER).set_text("true");
            }
            // keep the app's trailing empty <Name/> placeholder last
            let at = names
                .children
                .len()
                .saturating_sub(usize::from(names.elements().last().is_some_and(|n| !is_real_name(n))));
            names.children.insert(at, Node::Element(name));
        }
    }

    if gone_died {
        el.remove_children_where(|e| e.name == "IsLiving");
    }
    // DNA and the like: everyone has a Biology (with the gender), so merge it member by member
    if let Some(b) = gone.child("Biology") {
        let mine = el.ensure_child("Biology", PERSON_ORDER);
        for c in b.elements().filter(|c| c.name != "Gender") {
            match mine.child_mut(&c.name) {
                None => {
                    mine.push(c.clone());
                }
                Some(d) if LISTS.contains(&c.name.as_str()) => merge_entries(d, c),
                Some(_) => {}
            }
        }
    }

    if let Some(from) = gone.child("Facts") {
        merge_facts(el.ensure_child("Facts", PERSON_ORDER), from);
    }
    merge_citations(el, "PersonCitationCollection", PERSON_ORDER, &gone);
    merge_notes(el, PERSON_ORDER, &gone);

    // photos
    let photo_ids: Vec<String> =
        gone.child("Multimedia").into_iter().flat_map(|m| m.children_named("string")).map(Element::text).collect();
    if !photo_ids.is_empty() {
        let list = el.ensure_child("Multimedia", PERSON_ORDER);
        for id in photo_ids {
            if !list.children_named("string").any(|s| s.text() == id) {
                list.push(edit::text_el("string", &id));
            }
        }
    }
    if el.child_text("PrimaryImageId").is_none_or(|p| p.is_empty()) {
        if let Some(primary) = gone.child_text("PrimaryImageId").filter(|p| !p.is_empty()) {
            el.ensure_child("PrimaryImageId", PERSON_ORDER).set_text(&primary);
            if let (Some(face), None) = (gone.child("Face"), el.child("Face")) {
                *el.ensure_child("Face", PERSON_ORDER) = face.clone();
            }
        }
    }

    // lists entry by entry; anything else the removed person has, if the kept one has none of it
    for c in gone.elements().filter(|c| !HANDLED.contains(&c.name.as_str())) {
        match el.child_mut(&c.name) {
            None => *el.ensure_child(&c.name, PERSON_ORDER) = c.clone(),
            Some(dst) if LISTS.contains(&c.name.as_str()) => {
                merge_entries(dst, c);
                if c.name == "Associations" {
                    // an association with the kept person themselves makes no sense
                    dst.remove_children_where(|a| a.child_text("PersonId").as_deref() == Some(keep));
                }
            }
            Some(_) => {}
        }
    }
    edit::touch(el);
    edit::set_gender(doc, keep, gender)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_event_has_the_same_signature() {
        let mut doc = edit::new_family_document();
        let a = edit::add_person(&mut doc, "A", "B", Gender::Unknown);
        let input = edit::FactInput {
            date: crate::model::Date { year: Some(1900), ..Default::default() },
            place: "Tartu".into(),
            ..Default::default()
        };
        let f1 = edit::add_fact(&mut doc, &a, "BIRT", &input).unwrap();
        let f2 = edit::add_fact(&mut doc, &a, "BIRT", &input).unwrap();
        let facts: Vec<&Element> = doc
            .root
            .child("PeopleCollection")
            .unwrap()
            .elements()
            .next()
            .unwrap()
            .child("Facts")
            .unwrap()
            .elements()
            .collect();
        assert_ne!(f1, f2);
        assert_eq!(fact_signature(facts[0]), fact_signature(facts[1]));
    }
}

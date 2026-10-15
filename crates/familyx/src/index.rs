//! Lookup tables that keep `edit` fast on large documents.
//!
//! Finding a person, source or relationship link by id used to scan the whole
//! collection, so adding N people and links took time proportional to N
//! squared (an import of 20,000 people took minutes). The tables here are a
//! cache owned by the [`Document`]; the XML tree stays the only source of
//! truth and the answers are always the same as a scan would give:
//!
//! * a hit is checked against the element it points to,
//! * a miss is trusted only while the collection still has the length the
//!   table was built for, otherwise the table is rebuilt first,
//! * `edit` clears the tables before any change it does not track itself, and
//!   adds to them when it appends a person, source, link or fact.
//!
//! A document clone starts without tables, and comparing documents ignores them.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};

use crate::doc::{Document, Element, Node};

/// Ids of the elements named `elem` in a collection, to their position in `children`.
#[derive(Default)]
struct IdIndex {
    map: HashMap<String, usize>,
    /// `children.len()` of the collection when the table was last in step with it.
    len: usize,
}

impl IdIndex {
    fn build(coll: &Element, elem: &str) -> Self {
        let mut map = HashMap::new();
        for (pos, node) in coll.children.iter().enumerate() {
            if let Node::Element(e) = node {
                if e.name == elem {
                    if let Some(id) = e.attr("Id") {
                        map.entry(id.into_owned()).or_insert(pos);
                    }
                }
            }
        }
        IdIndex { map, len: coll.children.len() }
    }

    fn find(slot: &mut Option<IdIndex>, coll: Option<&Element>, elem: &str, id: &str) -> Option<usize> {
        let coll = coll?;
        if let Some(index) = slot.as_ref() {
            match index.map.get(id) {
                Some(&pos)
                    if element_at(coll, pos).is_some_and(|e| e.name == elem && e.attr("Id").as_deref() == Some(id)) =>
                {
                    return Some(pos)
                }
                None if index.len == coll.children.len() => return None,
                _ => {}
            }
        }
        let index = IdIndex::build(coll, elem);
        let found = index.map.get(id).copied();
        *slot = Some(index);
        found
    }

    /// The collection just got a new last child.
    fn appended(slot: &mut Option<IdIndex>, coll: Option<&Element>, elem: &str) {
        let (Some(index), Some(coll)) = (slot.as_mut(), coll) else { return };
        let pos = coll.children.len().wrapping_sub(1);
        let new = element_at(coll, pos).filter(|e| e.name == elem);
        match new {
            Some(e) if index.len == pos => {
                if let Some(id) = e.attr("Id") {
                    index.map.entry(id.into_owned()).or_insert(pos);
                }
                index.len = coll.children.len();
            }
            _ => *slot = None,
        }
    }
}

fn element_at(coll: &Element, pos: usize) -> Option<&Element> {
    match coll.children.get(pos)? {
        Node::Element(e) => Some(e),
        _ => None,
    }
}

/// The two kinds of relationship link the editor looks up by their people.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum LinkKind {
    ParentChild,
    Spouse,
}

impl LinkKind {
    pub(crate) fn from_type(t: &str) -> Option<Self> {
        match t {
            "ParentChildLink" => Some(LinkKind::ParentChild),
            "SpouseLink" => Some(LinkKind::Spouse),
            _ => None,
        }
    }
}

#[derive(Default)]
struct Links {
    by_id: HashMap<String, usize>,
    /// (kind, Person1Id, Person2Id) to the first such link.
    pairs: HashMap<(LinkKind, String, String), usize>,
    /// child -> parents and parent -> children, in document order.
    parents: HashMap<String, Vec<String>>,
    children: HashMap<String, Vec<String>>,
    len: usize,
}

impl Links {
    fn build(rel: &Element) -> Self {
        let mut links = Links::default();
        for (pos, node) in rel.children.iter().enumerate() {
            if let Node::Element(e) = node {
                links.add(pos, e);
            }
        }
        links.len = rel.children.len();
        links
    }

    fn add(&mut self, pos: usize, link: &Element) {
        if link.name != "RelationshipLink" {
            return;
        }
        if let Some(id) = link.attr("Id") {
            self.by_id.entry(id.into_owned()).or_insert(pos);
        }
        let kind = link.attr("xsi:type").and_then(|t| LinkKind::from_type(&t));
        let (Some(kind), Some(p1), Some(p2)) = (kind, link.attr("Person1Id"), link.attr("Person2Id")) else { return };
        let (p1, p2) = (p1.into_owned(), p2.into_owned());
        if kind == LinkKind::ParentChild {
            self.parents.entry(p2.clone()).or_default().push(p1.clone());
            self.children.entry(p1.clone()).or_default().push(p2.clone());
        }
        self.pairs.entry((kind, p1, p2)).or_insert(pos);
    }
}

#[derive(Default)]
struct Inner {
    people: Option<IdIndex>,
    sources: Option<IdIndex>,
    links: Option<Links>,
    /// fact id -> (on a relationship link rather than a person, position in that collection)
    facts: Option<HashMap<String, (bool, usize)>>,
}

#[derive(Default)]
pub(crate) struct IndexCell(Box<Mutex<Inner>>);

impl Clone for IndexCell {
    fn clone(&self) -> Self {
        IndexCell::default()
    }
}

impl std::fmt::Debug for IndexCell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("IndexCell")
    }
}

impl PartialEq for IndexCell {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// What `edit` just appended to a collection.
#[derive(Clone, Copy)]
pub(crate) enum Appended {
    Person,
    Source,
    Link,
}

fn collect_facts(el: &Element, home: (bool, usize), out: &mut HashMap<String, (bool, usize)>) {
    if el.name == "Fact" {
        if let Some(id) = el.attr("Id") {
            out.entry(id.into_owned()).or_insert(home);
        }
    }
    for child in el.elements() {
        collect_facts(child, home, out);
    }
}

impl Document {
    fn tables(&self) -> MutexGuard<'_, Inner> {
        self.index.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forget all lookup tables; call before changing the document in a way the tables do not track.
    pub(crate) fn forget_index(&self) {
        *self.tables() = Inner::default();
    }

    /// Position of the `Person` with this id in `PeopleCollection`'s children.
    pub(crate) fn person_pos(&self, id: &str) -> Option<usize> {
        IdIndex::find(&mut self.tables().people, self.root.child("PeopleCollection"), "Person", id)
    }

    pub(crate) fn source_pos(&self, id: &str) -> Option<usize> {
        IdIndex::find(&mut self.tables().sources, self.root.child("SourceCollection"), "Source", id)
    }

    /// The collection just got a new last child of this kind.
    pub(crate) fn note_appended(&self, what: Appended) {
        let mut t = self.tables();
        match what {
            Appended::Person => IdIndex::appended(&mut t.people, self.root.child("PeopleCollection"), "Person"),
            Appended::Source => IdIndex::appended(&mut t.sources, self.root.child("SourceCollection"), "Source"),
            Appended::Link => {
                let (Some(links), Some(rel)) = (t.links.as_mut(), self.root.child("RelationshipCollection")) else {
                    return;
                };
                let pos = rel.children.len().wrapping_sub(1);
                match element_at(rel, pos) {
                    Some(link) if links.len == pos => {
                        links.add(pos, link);
                        links.len = rel.children.len();
                    }
                    _ => t.links = None,
                }
            }
        }
    }

    fn links_with<T>(&self, f: impl FnOnce(&Links) -> T) -> Option<T> {
        let rel = self.root.child("RelationshipCollection")?;
        let mut t = self.tables();
        if t.links.as_ref().is_none_or(|l| l.len != rel.children.len()) {
            t.links = Some(Links::build(rel));
        }
        t.links.as_ref().map(f)
    }

    /// A position from the link table, checked against the element there; on a stale hit the
    /// table is rebuilt once and asked again.
    fn verified_link(&self, find: impl Fn(&Links) -> Option<usize>, ok: impl Fn(&Element) -> bool) -> Option<usize> {
        let rel = self.root.child("RelationshipCollection")?;
        for _ in 0..2 {
            let pos = self.links_with(&find).flatten()?;
            if element_at(rel, pos).is_some_and(&ok) {
                return Some(pos);
            }
            self.tables().links = None;
        }
        None
    }

    /// Position in `RelationshipCollection` of the link with this id.
    pub(crate) fn link_pos(&self, id: &str) -> Option<usize> {
        self.verified_link(|l| l.by_id.get(id).copied(), |e| e.attr("Id").as_deref() == Some(id))
    }

    /// Position of the first link of `kind` from `a` to `b` (or, unless `ordered`, from `b` to `a`).
    pub(crate) fn link_between(&self, kind: LinkKind, a: &str, b: &str, ordered: bool) -> Option<usize> {
        let is = |e: &Element, x: &str, y: &str| {
            e.attr("xsi:type").and_then(|t| LinkKind::from_type(&t)) == Some(kind)
                && e.attr("Person1Id").as_deref() == Some(x)
                && e.attr("Person2Id").as_deref() == Some(y)
        };
        self.verified_link(
            |l| {
                let find = |x: &str, y: &str| l.pairs.get(&(kind, x.to_string(), y.to_string())).copied();
                match (find(a, b), if ordered { None } else { find(b, a) }) {
                    (Some(p), Some(q)) => Some(p.min(q)),
                    (p, q) => p.or(q),
                }
            },
            |e| is(e, a, b) || (!ordered && is(e, b, a)),
        )
    }

    pub(crate) fn parents_of(&self, child: &str) -> Vec<String> {
        self.links_with(|l| l.parents.get(child).cloned().unwrap_or_default()).unwrap_or_default()
    }

    pub(crate) fn children_of(&self, parent: &str) -> Vec<String> {
        self.links_with(|l| l.children.get(parent).cloned().unwrap_or_default()).unwrap_or_default()
    }

    /// Where a fact probably lives; the caller checks, and falls back to a scan.
    pub(crate) fn fact_home(&self, id: &str) -> Option<(bool, usize)> {
        let mut t = self.tables();
        let facts = t.facts.get_or_insert_with(|| {
            let mut out = HashMap::new();
            for (rel, name) in [(false, "PeopleCollection"), (true, "RelationshipCollection")] {
                for (pos, node) in self.root.child(name).into_iter().flat_map(|c| c.children.iter().enumerate()) {
                    if let Node::Element(e) = node {
                        collect_facts(e, (rel, pos), &mut out);
                    }
                }
            }
            out
        });
        facts.get(id).copied()
    }

    /// `edit` added a fact to the person or link at `home`.
    pub(crate) fn note_fact(&self, id: &str, home: (bool, usize)) {
        if let Some(facts) = self.tables().facts.as_mut() {
            facts.insert(id.to_string(), home);
        }
    }

    /// A fact lookup that failed to confirm: rebuild next time.
    pub(crate) fn forget_facts(&self) {
        self.tables().facts = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, FactInput};
    use crate::model::Gender;

    /// Every lookup the tables answer, compared with a scan of the XML.
    fn assert_tables_match_scan(doc: &Document, people: &[String]) {
        let scan_person = |id: &str| {
            let coll = doc.root.child("PeopleCollection").unwrap();
            coll.children.iter().position(
                |n| matches!(n, Node::Element(e) if e.name == "Person" && e.attr("Id").as_deref() == Some(id)),
            )
        };
        let scan_link = |kind: &str, a: &str, b: &str, ordered: bool| {
            doc.root.child("RelationshipCollection").unwrap().children.iter().position(|n| match n {
                Node::Element(l) if l.name == "RelationshipLink" => {
                    let (p1, p2) = (l.attr("Person1Id"), l.attr("Person2Id"));
                    l.attr("xsi:type").as_deref() == Some(kind)
                        && ((p1.as_deref() == Some(a) && p2.as_deref() == Some(b))
                            || (!ordered && p1.as_deref() == Some(b) && p2.as_deref() == Some(a)))
                }
                _ => false,
            })
        };
        let links: Vec<&Element> =
            doc.root.child("RelationshipCollection").unwrap().children_named("RelationshipLink").collect();
        for id in people.iter().map(String::as_str).chain(["missing"]) {
            assert_eq!(doc.person_pos(id), scan_person(id), "person {id}");
            let parents: Vec<String> = links
                .iter()
                .filter(|l| {
                    l.attr("xsi:type").as_deref() == Some("ParentChildLink")
                        && l.attr("Person2Id").as_deref() == Some(id)
                })
                .filter_map(|l| l.attr("Person1Id").map(|s| s.into_owned()))
                .collect();
            assert_eq!(doc.parents_of(id), parents, "parents of {id}");
            for other in people {
                assert_eq!(
                    doc.link_between(LinkKind::ParentChild, id, other, true),
                    scan_link("ParentChildLink", id, other, true)
                );
                assert_eq!(
                    doc.link_between(LinkKind::Spouse, id, other, false),
                    scan_link("SpouseLink", id, other, false)
                );
            }
        }
    }

    #[test]
    fn lookups_stay_correct_through_edits() {
        let mut doc = edit::new_family_document();
        let mut people: Vec<String> =
            (0..8).map(|i| edit::add_person(&mut doc, &format!("P{i}"), "Test", Gender::Unknown)).collect();
        assert_tables_match_scan(&doc, &people);

        edit::link_spouse(&mut doc, &people[0], &people[1]).unwrap();
        edit::link_parent(&mut doc, &people[2], &people[0]).unwrap();
        edit::link_parent(&mut doc, &people[2], &people[1]).unwrap();
        edit::link_parent(&mut doc, &people[3], &people[0]).unwrap();
        assert_tables_match_scan(&doc, &people);

        // facts are found through the table, and again after it was dropped
        let fact = edit::add_fact(&mut doc, &people[4], "BIRT", &FactInput::default()).unwrap();
        let couple = edit::link_spouse(&mut doc, &people[4], &people[5]).unwrap();
        let couple_fact = edit::add_couple_fact(&mut doc, &couple, "MARR", &FactInput::default()).unwrap();
        let source = edit::add_source(&mut doc, &Default::default()).unwrap();
        edit::add_citation(&mut doc, &fact, &source, "p. 1").unwrap();
        edit::add_citation(&mut doc, &couple_fact, &source, "p. 2").unwrap();
        doc.forget_index();
        edit::add_citation(&mut doc, &fact, &source, "p. 3").unwrap();
        assert!(edit::add_citation(&mut doc, "no such fact", &source, "").is_err());
        assert!(edit::check(&doc).is_empty());

        // removals and reorderings shift positions: tables must follow
        edit::delete_person(&mut doc, &people[1]).unwrap();
        people.remove(1);
        assert_tables_match_scan(&doc, &people);
        edit::unlink_parent(&mut doc, &people[2], &people[0]).unwrap();
        assert_tables_match_scan(&doc, &people);
        edit::move_child(&mut doc, &people[0], &people[2], 1).unwrap();
        assert_tables_match_scan(&doc, &people);
        let extra = edit::add_person(&mut doc, "Late", "Test", Gender::Female);
        people.push(extra);
        edit::link_parent(&mut doc, &people[4], &people[0]).unwrap();
        assert_tables_match_scan(&doc, &people);
        assert!(edit::link_parent(&mut doc, "missing", &people[0]).is_err());
        assert!(edit::check(&doc).is_empty());

        // a clone starts with no tables and equals the original
        assert_eq!(doc.clone(), doc);
        assert_tables_match_scan(&doc.clone(), &people);
    }
}

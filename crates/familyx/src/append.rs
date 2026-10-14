//! Adding the contents of one family document to another ("import into this tree").
//!
//! Every record that is copied gets a fresh id, and every reference to it
//! (links between people, back-references on people, citations, photos) is
//! rewritten to match. Sources and repositories that already exist in the
//! target (same title, author, ...) are reused instead of copied.
//! The target's own records and its home person are not touched.

use std::collections::{HashMap, HashSet};

use crate::doc::{Document, Element, Node};
use crate::edit::{self, new_id};
use crate::model::{Id, Tree};

/// What [`crate::Package::append`] added.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AppendReport {
    /// Ids of the added people (new ids, in the order of the other tree).
    pub people: Vec<Id>,
    /// Couples (spouse links) added.
    pub couples: usize,
    /// Parent-child links added.
    pub parent_links: usize,
    /// Sources added; sources the tree already had are not counted.
    pub sources: usize,
    /// Photos and other media files added.
    pub photos: usize,
}

/// A media file that was copied: its part name in the other package and the new one in this.
pub(crate) struct CopiedPart {
    pub old: String,
    pub new: String,
}

const COLLECTIONS: &[&str] = &[
    "PeopleCollection",
    "SourceCollection",
    "RepositoryCollection",
    "PackagedFileCollection",
    "RelationshipCollection",
    "TagCollection",
];

fn collect_ids(el: &Element, out: &mut Vec<String>) {
    if let Some(id) = el.attr("Id") {
        out.push(id.into_owned());
    }
    for c in el.elements() {
        collect_ids(c, out);
    }
}

/// Replace every attribute value and text that is exactly an old id.
fn remap(el: &mut Element, map: &HashMap<String, String>) {
    for (_, v) in &mut el.attrs {
        if let Some(new) = map.get(v.as_str()) {
            *v = new.clone();
        }
    }
    for node in &mut el.children {
        match node {
            Node::Element(c) => remap(c, map),
            Node::Text(t) => {
                if let Some(new) = map.get(t.trim()) {
                    *t = new.clone();
                }
            }
            Node::Raw(_) => {}
        }
    }
}

fn same_source(a: &crate::model::Source, b: &crate::model::Source) -> bool {
    let notes = |s: &crate::model::Source| s.notes.iter().map(|n| n.text.trim().to_string()).collect::<Vec<_>>();
    !a.title.trim().is_empty()
        && a.title.trim() == b.title.trim()
        && a.abbreviation.trim() == b.abbreviation.trim()
        && a.author.trim() == b.author.trim()
        && a.publisher.trim() == b.publisher.trim()
        && a.call_number.trim() == b.call_number.trim()
        && notes(a) == notes(b)
}

/// Copy people, couples, sources, repositories, media records and tags of `src` into `dst`.
/// The media files themselves are the package's business: see the returned parts.
pub(crate) fn append_document(dst: &mut Document, src: &Document) -> (AppendReport, Vec<CopiedPart>) {
    let src_tree = Tree::from_doc(src);
    let dst_tree = Tree::from_doc(dst);

    // records that already exist here are reused: old id -> existing id
    let mut map: HashMap<String, String> = HashMap::new();
    let mut skip: HashSet<String> = HashSet::new();
    for r in &src_tree.repositories {
        let title = r.title.trim();
        if let Some(existing) = dst_tree.repositories.iter().find(|e| e.title.trim() == title && !title.is_empty()) {
            map.insert(r.id.clone(), existing.id.clone());
            skip.insert(r.id.clone());
        }
    }
    for s in &src_tree.sources {
        let mut s = s.clone();
        // compare repositories after the mapping above
        s.repository = s.repository.map(|r| map.get(&r).cloned().unwrap_or(r));
        if let Some(existing) = dst_tree.sources.iter().find(|e| same_source(e, &s)) {
            map.insert(s.id.clone(), existing.id.clone());
            skip.insert(s.id.clone());
        }
    }
    // everything else gets a fresh id, nested records (names, facts, citations) included
    for name in COLLECTIONS {
        for el in src.root.child(name).into_iter().flat_map(Element::elements) {
            let mut ids = Vec::new();
            collect_ids(el, &mut ids);
            let top = el.attr("Id").map(|i| i.into_owned());
            if top.is_some_and(|t| skip.contains(&t)) {
                continue;
            }
            for id in ids {
                map.entry(id).or_insert_with(new_id);
            }
        }
    }

    let mut report = AppendReport::default();
    let mut parts = Vec::new();
    for name in COLLECTIONS {
        let items: Vec<Element> = src
            .root
            .child(name)
            .into_iter()
            .flat_map(Element::elements)
            .filter(|e| e.attr("Id").is_none_or(|i| !skip.contains(i.as_ref())))
            .cloned()
            .collect();
        if items.is_empty() {
            continue;
        }
        let target = edit::collection(dst, name);
        for mut el in items {
            remap(&mut el, &map);
            match *name {
                "PeopleCollection" if el.name == "Person" => {
                    report.people.push(el.attr("Id").unwrap_or_default().into_owned());
                }
                "SourceCollection" => report.sources += 1,
                "RelationshipCollection" => match el.attr("xsi:type").as_deref() {
                    Some("SpouseLink") => report.couples += 1,
                    Some("ParentChildLink") => report.parent_links += 1,
                    _ => {}
                },
                "PackagedFileCollection" => {
                    if let Some(old) = el.child_text("RelativePath").filter(|p| !p.is_empty()) {
                        let old = old.replace('\\', "/");
                        let ext = old.rsplit_once('.').map_or("", |(_, e)| e);
                        let new = format!("Multimedia/{}{}{ext}", new_id(), if ext.is_empty() { "" } else { "." });
                        let original_is_path =
                            el.child_text("OriginalFileName").map(|o| o.replace('\\', "/")) == Some(old.clone());
                        let windows = new.replace('/', "\\");
                        if let Some(p) = el.child_mut("RelativePath") {
                            p.set_text(&windows);
                        }
                        if original_is_path {
                            if let Some(p) = el.child_mut("OriginalFileName") {
                                p.set_text(&windows);
                            }
                        }
                        parts.push(CopiedPart { old, new });
                    }
                    report.photos += 1;
                }
                _ => {}
            }
            target.push(el);
        }
    }
    (report, parts)
}

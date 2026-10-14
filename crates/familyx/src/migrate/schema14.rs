//! The 14.x steps of the app's conversion chain that change the shape of elements, in both directions.
//!
//! My Family Tree 12.6 writes schema `Version` 14.4. [`upgrade`] does what the current app's chain does to
//! such a tree on open (gates 14.0 to 14.8); [`downgrade`] writes a current tree back in the 14.4 shape, so
//! that app reads it. The 14.7 step (the `FamilyId` notation) is in [`crate::protection`]; opening applies it
//! to every tree from an older container.

use crate::doc::{Document, Element, Node};
use crate::edit::{record, text_el, FAMILY_ORDER};
use crate::protection;

/// `FileProperties/Version` written by My Family Tree 12.6.
pub const APP12_VERSION: &str = "14.4";

/// `NameTranslation` string arrays (up to 14.7) and the `NamePart` lists that replaced them (14.8).
const NAME_ARRAYS: [(&str, &str); 6] = [
    ("Prefixes", "PrefixParts"),
    ("Names", "NameParts"),
    ("Nicknames", "NicknameParts"),
    ("SurnamePrefixes", "SurnamePrefixParts"),
    ("Surnames", "SurnameParts"),
    ("Suffixes", "SuffixParts"),
];
const PACKAGED_FILE_ORDER: &[&str] =
    &["OriginalFileName", "RelativePath", "FileName", "IsResolved", "IsTranslation", "Type", "Properties"];

/// What saving for My Family Tree 12.x could not carry over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Downgrade {
    /// The tree had a password; 12.x cannot read the current notation, so the copy has none.
    pub password_removed: bool,
    /// Task places reduced to their description (12.x has a plain `Location` text).
    pub task_places: usize,
}

fn walk(el: &mut Element, f: &mut impl FnMut(&mut Element)) {
    f(el);
    for child in el.elements_mut() {
        walk(child, f);
    }
}

/// Replace the child element `name` (if any) in place by `f(old)`.
fn replace_child(el: &mut Element, name: &str, f: impl FnOnce(&Element) -> Element) {
    for node in &mut el.children {
        if let Node::Element(old) = node {
            if old.name == name {
                *node = Node::Element(f(old));
                return;
            }
        }
    }
}

/// The app's 14.0, 14.5, 14.6 and 14.8 steps on a tree of an older `version` (`None`: unversioned).
pub(super) fn upgrade(doc: &mut Document, version: Option<f64>) {
    let before = |gate: f64| version.is_none_or(|v| v < gate);
    walk(&mut doc.root, &mut |el| match el.name.as_str() {
        // initiatory (LDS): INIT (14.0) and EVEN_WAC (14.5) became INIL
        "Fact" => {
            if let Some(tag) = el.child_mut("Tag") {
                let old = tag.text();
                if (old == "INIT" && before(14.0)) || (old == "EVEN_WAC" && before(14.5)) {
                    tag.set_text("INIL");
                }
            }
        }
        "NameTranslation" if before(14.8) => {
            for (array, parts) in NAME_ARRAYS {
                replace_child(el, array, |old| {
                    let mut list = Element::new(parts);
                    for s in old.children_named("string") {
                        list.push(record("NamePart")).push(text_el("Name", &s.text()));
                    }
                    list
                });
            }
        }
        // 12.x stores whether the file was found; the current app works it out on open
        "PackagedFile" => {
            el.remove_children_where(|c| c.name == "IsResolved");
        }
        _ => {}
    });
    if before(14.6) {
        for task in doc.root.child_mut("TaskCollection").into_iter().flat_map(Element::elements_mut) {
            replace_child(task, "Location", |old| {
                let mut place = record("PlaceDetails");
                let text = old.text();
                if !text.is_empty() {
                    place.push(text_el("Description", &text));
                }
                place
            });
        }
    }
}

/// Rewrite a current tree into the 14.4 shape My Family Tree 12.6 reads. `has_part` tells whether a
/// package part (`Multimedia/x.png`) exists, for the photos' `IsResolved` flag.
pub fn downgrade(doc: &mut Document, has_part: &dyn Fn(&str) -> bool) -> Downgrade {
    let mut result = Downgrade::default();
    walk(&mut doc.root, &mut |el| match el.name.as_str() {
        "NameTranslation" => {
            for (array, parts) in NAME_ARRAYS {
                replace_child(el, parts, |old| {
                    let mut list = Element::new(array);
                    for part in old.children_named("NamePart") {
                        list.push(text_el("string", &part.child_text("Name").unwrap_or_default()));
                    }
                    list
                });
            }
        }
        // without it 12.x shows a placeholder and drops the file on its next save
        "PackagedFile" => {
            let path = el.child_text("RelativePath").unwrap_or_default().replace('\\', "/");
            let found = !path.is_empty() && has_part(&path);
            el.ensure_child("IsResolved", PACKAGED_FILE_ORDER).set_text(if found { "true" } else { "false" });
        }
        _ => {}
    });
    for task in doc.root.child_mut("TaskCollection").into_iter().flat_map(Element::elements_mut) {
        replace_child(task, "PlaceDetails", |old| {
            if old.elements().any(|e| e.name != "Description" && !e.children.is_empty()) {
                result.task_places += 1;
            }
            text_el("Location", &old.child_text("Description").unwrap_or_default())
        });
    }
    result.password_removed = protection::document_is_protected(doc);
    doc.root.set_attr("FamilyId", &protection::legacy_default_family_id());
    let props = doc.root.ensure_child("FileProperties", FAMILY_ORDER);
    props.ensure_child("Version", &["Version", "LastSavedDate", "CreationDate"]).set_text(APP12_VERSION);
    result
}

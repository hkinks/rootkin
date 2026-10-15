//! Nobody can become their own ancestor.

use familyx::edit;
use familyx::merge::{merge_people, MergeOptions};
use familyx::model::{Gender, Tree};

/// A line of `n` people, each the parent of the next; returns their ids, eldest first.
fn line(doc: &mut familyx::doc::Document, n: usize) -> Vec<String> {
    let ids: Vec<String> = (0..n).map(|i| edit::add_person(doc, &format!("P{i}"), "Line", Gender::Male)).collect();
    for pair in ids.windows(2) {
        edit::link_parent(doc, &pair[1], &pair[0]).unwrap();
    }
    ids
}

#[test]
fn link_parent_refuses_cycles_of_any_length() {
    for n in [2, 3, 5, 40] {
        let mut doc = edit::new_family_document();
        let ids = line(&mut doc, n);
        let (eldest, youngest) = (&ids[0], &ids[n - 1]);
        // the youngest becomes the eldest's parent: a loop
        let err = edit::link_parent(&mut doc, eldest, youngest).unwrap_err().to_string();
        assert!(err.contains("descends from") || err.contains("already a child"), "{n}: {err}");
        assert!(edit::is_ancestor(&doc, eldest, youngest));
        assert!(!edit::is_ancestor(&doc, youngest, eldest));
        assert!(edit::check(&doc).is_empty());
    }
}

#[test]
fn merge_refuses_a_grandparent_with_a_grandchild() {
    let mut doc = edit::new_family_document();
    let ids = line(&mut doc, 3);
    let err = merge_people(&mut doc, &ids[0], &ids[2], MergeOptions::default()).unwrap_err().to_string();
    assert!(err.contains("their own ancestor"), "{err}");
    let err = merge_people(&mut doc, &ids[2], &ids[0], MergeOptions::default()).unwrap_err().to_string();
    assert!(err.contains("their own ancestor"), "{err}");
    assert_eq!(Tree::from_doc(&doc).people.len(), 3, "nothing changed");
}

#[test]
fn merge_through_a_shared_relative_is_refused_too() {
    // keep's child X is the parent of remove: merging makes keep their own grandparent
    let mut doc = edit::new_family_document();
    let keep = edit::add_person(&mut doc, "A", "T", Gender::Male);
    let x = edit::add_person(&mut doc, "X", "T", Gender::Male);
    let other = edit::add_person(&mut doc, "Y", "T", Gender::Male);
    let remove = edit::add_person(&mut doc, "A", "T", Gender::Male);
    edit::link_parent(&mut doc, &x, &keep).unwrap();
    edit::link_parent(&mut doc, &remove, &other).unwrap();
    edit::link_parent(&mut doc, &other, &x).unwrap();
    assert!(merge_people(&mut doc, &keep, &remove, MergeOptions::default()).is_err());
}

#[test]
fn merging_unrelated_or_direct_relatives_still_works() {
    let mut doc = edit::new_family_document();
    let ids = line(&mut doc, 2);
    // a parent entered twice as their own child: the direct link is dropped by the merge
    merge_people(&mut doc, &ids[0], &ids[1], MergeOptions::default()).unwrap();
    assert!(edit::check(&doc).is_empty());
    let mut doc = edit::new_family_document();
    let a = line(&mut doc, 3);
    let b = line(&mut doc, 3);
    merge_people(&mut doc, &a[1], &b[1], MergeOptions::default()).unwrap();
    let tree = Tree::from_doc(&doc);
    assert_eq!(tree.parents(&a[1]).len(), 2);
    assert_eq!(tree.children(&a[1]).len(), 2);
}

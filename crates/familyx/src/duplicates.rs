//! Finding people who appear twice in a tree.
//!
//! Two people are candidates when one of their names matches (ignoring case,
//! diacritics, punctuation and word order, alternate names included) and
//! their birth years are at most 2 apart or one of them is unknown. Pairs
//! that are already relatives, or that clearly differ (gender, death year),
//! are not reported.

use std::collections::{HashMap, HashSet};

use crate::model::{Gender, Id, Person, Tree};

/// Two people who may be the same person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateCandidate {
    pub a: Id,
    pub b: Id,
    /// Higher is more likely; 0 to about 130.
    pub score: u32,
    /// The match is on an alternate (e.g. married or birth) name, not on both primary names.
    pub via_alternate_name: bool,
}

/// Lower case, diacritics removed, punctuation turned into spaces.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => out.push('a'),
            'ç' | 'ć' | 'č' => out.push('c'),
            'ď' | 'đ' => out.push('d'),
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => out.push('e'),
            'ğ' => out.push('g'),
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' => out.push('i'),
            'ł' => out.push('l'),
            'ñ' | 'ń' | 'ň' => out.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => out.push('o'),
            'ř' => out.push('r'),
            'ś' | 'š' | 'ş' => out.push('s'),
            'ť' | 'ţ' => out.push('t'),
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => out.push('u'),
            'ý' | 'ÿ' => out.push('y'),
            'ź' | 'ž' | 'ż' => out.push('z'),
            'ß' => out.push_str("ss"),
            'æ' => out.push_str("ae"),
            'œ' => out.push_str("oe"),
            c if c.is_alphanumeric() => out.push(c),
            _ => out.push(' '),
        }
    }
    out
}

/// The comparable form of a name: folded words in alphabetical order.
pub(crate) fn name_key(given: &str, prefix: &str, surname: &str) -> String {
    let folded = fold(&format!("{given} {prefix} {surname}"));
    let mut words: Vec<&str> = folded.split_whitespace().collect();
    words.sort_unstable();
    words.join(" ")
}

/// (key, is the primary name) for each distinct name of a person.
fn keys(p: &Person) -> Vec<(String, bool)> {
    let primary = p.primary_name().map(|n| n.id.as_str());
    let mut out: Vec<(String, bool)> = Vec::new();
    for n in &p.names {
        let key = name_key(&n.given, &n.surname_prefix, &n.surname);
        if !key.is_empty() && !out.iter().any(|(k, _)| *k == key) {
            out.push((key, Some(n.id.as_str()) == primary));
        }
    }
    out
}

fn year(p: &Person, tag: &str) -> Option<i32> {
    p.fact(tag).and_then(|f| f.date.year)
}

/// Candidate pairs in the whole tree, most likely first.
pub fn find_duplicates(tree: &Tree) -> Vec<DuplicateCandidate> {
    find(tree, |_, _| true)
}

/// Candidate pairs where one person is in `added` and the other is not,
/// e.g. after an import: "who of the new people is already here?".
pub fn find_duplicates_of_added(tree: &Tree, added: &HashSet<Id>) -> Vec<DuplicateCandidate> {
    find(tree, |a, b| added.contains(a) != added.contains(b))
}

fn find(tree: &Tree, wanted: impl Fn(&str, &str) -> bool) -> Vec<DuplicateCandidate> {
    /// More people than this under one name are not compared (a placeholder name).
    const MAX_GROUP: usize = 400;

    let mut blocks: HashMap<String, Vec<(usize, bool)>> = HashMap::new();
    for (i, p) in tree.people.iter().enumerate() {
        for (key, primary) in keys(p) {
            blocks.entry(key).or_default().push((i, primary));
        }
    }
    let mut parents: HashMap<&str, HashSet<&str>> = HashMap::new();
    let mut related: HashSet<(&str, &str)> = HashSet::new();
    for l in &tree.parent_links {
        parents.entry(l.child.as_str()).or_default().insert(l.parent.as_str());
        related.insert((l.parent.as_str(), l.child.as_str()));
        related.insert((l.child.as_str(), l.parent.as_str()));
    }
    let mut spouses: HashMap<&str, HashSet<&str>> = HashMap::new();
    for l in &tree.spouse_links {
        spouses.entry(l.person1.as_str()).or_default().insert(l.person2.as_str());
        spouses.entry(l.person2.as_str()).or_default().insert(l.person1.as_str());
        related.insert((l.person1.as_str(), l.person2.as_str()));
        related.insert((l.person2.as_str(), l.person1.as_str()));
    }
    let shares = |map: &HashMap<&str, HashSet<&str>>, a: &str, b: &str| {
        u32::from(matches!((map.get(a), map.get(b)), (Some(x), Some(y)) if !x.is_disjoint(y)))
    };

    // a pair found through two names keeps its best score
    let mut best: HashMap<(usize, usize), DuplicateCandidate> = HashMap::new();
    for (key, members) in &blocks {
        if members.len() < 2 || members.len() > MAX_GROUP {
            continue;
        }
        let two_words = key.contains(' ');
        for (x, &(i, i_primary)) in members.iter().enumerate() {
            for &(j, j_primary) in &members[x + 1..] {
                let (lo, hi) = (i.min(j), i.max(j));
                let (a, b) = (&tree.people[lo], &tree.people[hi]);
                if !wanted(&a.id, &b.id) || related.contains(&(a.id.as_str(), b.id.as_str())) {
                    continue;
                }
                if matches!((a.gender, b.gender), (Gender::Male, Gender::Female) | (Gender::Female, Gender::Male)) {
                    continue;
                }
                let birth = match (year(a, "BIRT"), year(b, "BIRT")) {
                    (Some(x), Some(y)) if (x - y).abs() <= 2 => 40 - 10 * (x - y).unsigned_abs(),
                    (Some(_), Some(_)) | (None, None) => continue,
                    // with one year unknown, a one-word name is too little to go on
                    _ if !two_words => continue,
                    _ => 10,
                };
                let death = match (year(a, "DEAT"), year(b, "DEAT")) {
                    (Some(x), Some(y)) if (x - y).abs() > 2 => continue,
                    (Some(x), Some(y)) if x == y => 5,
                    _ => 0,
                };
                let both_primary = i_primary && j_primary;
                let name = if both_primary { 60 } else { 40 };
                let relatives = 10 * (shares(&parents, &a.id, &b.id) + shares(&spouses, &a.id, &b.id));
                let candidate = DuplicateCandidate {
                    a: a.id.clone(),
                    b: b.id.clone(),
                    score: name + birth + death + relatives,
                    via_alternate_name: !both_primary,
                };
                match best.get(&(lo, hi)) {
                    Some(old) if old.score >= candidate.score => {}
                    _ => {
                        best.insert((lo, hi), candidate);
                    }
                }
            }
        }
    }
    let mut out: Vec<DuplicateCandidate> = best.into_values().collect();
    out.sort_by(|x, y| y.score.cmp(&x.score).then_with(|| (&x.a, &x.b).cmp(&(&y.a, &y.b))));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edit::{self, FactInput};
    use crate::model::{Date, Gender};

    fn person(doc: &mut crate::doc::Document, given: &str, surname: &str, born: Option<i32>) -> Id {
        let id = edit::add_person(doc, given, surname, Gender::Unknown);
        if let Some(y) = born {
            let input = FactInput { date: Date { year: Some(y), ..Default::default() }, ..Default::default() };
            edit::add_fact(doc, &id, "BIRT", &input).unwrap();
        }
        id
    }

    fn pairs(doc: &crate::doc::Document) -> Vec<(Id, Id, u32)> {
        find_duplicates(&Tree::from_doc(doc)).into_iter().map(|d| (d.a, d.b, d.score)).collect()
    }

    #[test]
    fn folds_case_and_diacritics() {
        assert_eq!(fold("Jüri-Õie ŠÕÕ"), "juri oie soo");
        assert_eq!(name_key("Ott", "", "Ilves"), name_key("ILVES,", "", "ott"));
    }

    #[test]
    fn matches_names_with_close_or_missing_birth_years() {
        let mut doc = edit::new_family_document();
        let a = person(&mut doc, "Jüri", "Tamm", Some(1850));
        let b = person(&mut doc, "JURI", "tamm", Some(1852));
        let c = person(&mut doc, "Jüri", "Tamm", Some(1860));
        let d = person(&mut doc, "Jüri", "Tamm", None);
        let found = pairs(&doc);
        let find = |x: &Id, y: &Id| found.iter().find(|(p, q, _)| (p == x && q == y) || (p == y && q == x));
        assert!(find(&a, &b).is_some());
        assert!(find(&a, &c).is_none(), "ten years apart");
        assert!(find(&a, &d).is_some(), "one year missing");
        // a closer birth year ranks higher
        assert!(find(&a, &b).unwrap().2 > find(&a, &d).unwrap().2);
        assert_eq!(found[0].2, found.iter().map(|f| f.2).max().unwrap(), "best first");
    }

    #[test]
    fn both_years_missing_or_conflicts_are_not_reported() {
        let mut doc = edit::new_family_document();
        person(&mut doc, "Mari", "Kask", None);
        person(&mut doc, "Mari", "Kask", None);
        edit::add_person(&mut doc, "Ott", "Ilves", Gender::Male);
        edit::add_person(&mut doc, "Ott", "Ilves", Gender::Female);
        assert!(pairs(&doc).is_empty());
    }

    #[test]
    fn relatives_and_alternate_names() {
        let mut doc = edit::new_family_document();
        let father = person(&mut doc, "Karl", "Rand", Some(1900));
        let son = person(&mut doc, "Karl", "Rand", Some(1925));
        edit::link_parent(&mut doc, &son, &father).unwrap();
        assert!(pairs(&doc).is_empty());

        let anna = person(&mut doc, "Anna", "Saar", Some(1930));
        let anna2 = person(&mut doc, "Anna", "Koit", Some(1931));
        assert!(pairs(&doc).is_empty());
        edit::add_alternate_name(&mut doc, &anna2, "Anna", "Saar", "Birth").unwrap();
        let found = find_duplicates(&Tree::from_doc(&doc));
        assert_eq!(found.len(), 1);
        assert!(found[0].via_alternate_name);
        assert!([&anna, &anna2].contains(&&found[0].a));
    }

    #[test]
    fn only_pairs_with_one_added_person() {
        let mut doc = edit::new_family_document();
        let old = person(&mut doc, "Eva", "Paju", Some(1900));
        let new1 = person(&mut doc, "Eva", "Paju", Some(1900));
        let new2 = person(&mut doc, "Eva", "Paju", Some(1901));
        let added: HashSet<Id> = [new1, new2].into();
        let found = find_duplicates_of_added(&Tree::from_doc(&doc), &added);
        assert_eq!(found.len(), 2, "new1/old and new2/old, not new1/new2");
        assert!(found.iter().all(|d| d.a == old || d.b == old));
    }
}

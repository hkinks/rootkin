//! Group a tree's couples and parent links into GEDCOM-style families.

use familyx::model::{Gender, SpouseLink, Tree};

pub struct FamChild<'a> {
    pub id: &'a str,
    /// Parent-link modifier toward the husband / wife (`""` = natural).
    pub husb_mod: &'a str,
    pub wife_mod: &'a str,
}

impl FamChild<'_> {
    /// The first non-natural modifier of either parent.
    pub fn modifier(&self) -> &str {
        if self.husb_mod.is_empty() {
            self.wife_mod
        } else {
            self.husb_mod
        }
    }
}

pub struct Fam<'a> {
    /// The couple record (with its facts), when the parents are linked as spouses.
    pub link: Option<&'a SpouseLink>,
    pub husb: Option<&'a str>,
    pub wife: Option<&'a str>,
    pub children: Vec<FamChild<'a>>,
}

/// Husband/wife by gender; otherwise in the given order.
fn roles<'a>(tree: &Tree, a: &'a str, b: &'a str) -> (&'a str, &'a str) {
    let g = |id: &str| tree.person(id).map(|p| p.gender).unwrap_or_default();
    if (g(a) == Gender::Female && g(b) != Gender::Female) || (g(b) == Gender::Male && g(a) != Gender::Male) {
        (b, a)
    } else {
        (a, b)
    }
}

/// Modifier of the link `parent -> child`, borrowed from the tree.
fn link_modifier<'a>(tree: &'a Tree, child: &str, parent: Option<&str>) -> &'a str {
    parent
        .and_then(|p| tree.parent_links.iter().find(|l| l.child == child && l.parent == p))
        .map_or("", |l| l.modifier.as_str())
}

/// One family per couple, plus families for parents who are not linked as a
/// couple (a single parent, or two unmarried parents) so every child has one.
pub fn build(tree: &Tree) -> Vec<Fam<'_>> {
    let mut fams: Vec<Fam> = tree
        .spouse_links
        .iter()
        .map(|l| {
            let (h, w) = roles(tree, &l.person1, &l.person2);
            Fam { link: Some(l), husb: Some(h), wife: Some(w), children: Vec::new() }
        })
        .collect();

    for child in &tree.people {
        let mut remaining: Vec<&str> = Vec::new();
        for l in tree.parent_links.iter().filter(|l| l.child == child.id) {
            if !remaining.contains(&l.parent.as_str()) {
                remaining.push(&l.parent);
            }
        }
        while !remaining.is_empty() {
            // two remaining parents who are a couple: use that family
            let couple = remaining.iter().enumerate().find_map(|(i, a)| {
                remaining.iter().enumerate().skip(i + 1).find_map(|(j, b)| {
                    fams.iter().position(|f| f.link.is_some_and(|l| l.other(a) == *b)).map(|idx| (i, j, idx))
                })
            });
            let (idx, used) = match couple {
                Some((i, j, idx)) => (idx, vec![i, j]),
                None => {
                    let (husb, wife, used) = match remaining.as_slice() {
                        [a, b, ..] => {
                            let (h, w) = roles(tree, a, b);
                            (Some(h), Some(w), vec![0, 1])
                        }
                        [a] if tree.person(a).is_some_and(|p| p.gender == Gender::Female) => (None, Some(*a), vec![0]),
                        [a] => (Some(*a), None, vec![0]),
                        [] => unreachable!("loop condition"),
                    };
                    let idx = match fams.iter().position(|f| f.link.is_none() && f.husb == husb && f.wife == wife) {
                        Some(idx) => idx,
                        None => {
                            fams.push(Fam { link: None, husb, wife, children: Vec::new() });
                            fams.len() - 1
                        }
                    };
                    (idx, used)
                }
            };
            let (husb, wife) = (fams[idx].husb, fams[idx].wife);
            fams[idx].children.push(FamChild {
                id: &child.id,
                husb_mod: link_modifier(tree, &child.id, husb),
                wife_mod: link_modifier(tree, &child.id, wife),
            });
            // remove highest index first
            let mut used = used;
            used.sort_unstable_by(|a, b| b.cmp(a));
            for u in used {
                remaining.remove(u);
            }
        }
    }
    fams
}

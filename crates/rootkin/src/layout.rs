//! Tree layout, independent of drawing: ancestors above the focus person,
//! siblings to the left, spouses to the right, descendants below.
//! Coordinates are box centres in layout units; the focus is at the origin.

use std::collections::HashSet;

use eframe::egui::{Pos2, Rect, Vec2};
use familyx::model::{Gender, Id, Tree};

pub const BOX: Vec2 = Vec2::new(200.0, 60.0);
const H_GAP: f32 = 20.0;
const V_GAP: f32 = 50.0;
const STEP: f32 = BOX.x + H_GAP;
const ROW: f32 = BOX.y + V_GAP;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Settings {
    pub ancestors: u32,
    pub descendants: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { ancestors: 3, descendants: 2 }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    Focus,
    Ancestor,
    /// A parent beyond the two drawn as the main pair (e.g. adoptive).
    OtherParent,
    Spouse,
    Sibling {
        half: bool,
    },
    Descendant,
}

pub struct Node {
    pub id: Id,
    pub pos: Pos2,
    pub role: Role,
    /// Has parents that are not drawn.
    pub more_up: bool,
    /// Has children that are not drawn.
    pub more_down: bool,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EdgeKind {
    /// Parent(s) → child, drawn as an elbow from `from` down to `to`.
    Child { adopted: bool },
    /// A straight line between partners.
    Couple,
}

pub struct Edge {
    pub from: Pos2,
    pub to: Pos2,
    pub kind: EdgeKind,
}

#[derive(Default)]
pub struct Layout {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Layout {
    pub fn bounds(&self) -> Rect {
        self.nodes
            .iter()
            .map(|n| Rect::from_center_size(n.pos, BOX))
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::from_center_size(Pos2::ZERO, BOX))
    }

    fn add(&mut self, id: &str, pos: Pos2, role: Role) {
        self.nodes.push(Node { id: id.to_string(), pos, role, more_up: false, more_down: false });
    }
}

fn top(p: Pos2) -> Pos2 {
    p - Vec2::new(0.0, BOX.y / 2.0)
}

fn bottom(p: Pos2) -> Pos2 {
    p + Vec2::new(0.0, BOX.y / 2.0)
}

/// Whether the parent→child link is anything but biological.
fn is_adopted(tree: &Tree, parent: &str, child: &str) -> bool {
    tree.parent_links
        .iter()
        .any(|l| l.parent == parent && l.child == child && !l.modifier.is_empty() && l.modifier != "Natural")
}

/// The main pair of parents to draw: biological links first, preferring a
/// couple, father on the left. Returns the pair and the remaining parents.
pub fn parent_pair(tree: &Tree, child: &str) -> (Vec<String>, Vec<String>) {
    let mut all: Vec<&str> = Vec::new();
    for l in tree.parent_links.iter().filter(|l| l.child == child) {
        if !all.contains(&l.parent.as_str()) {
            all.push(&l.parent);
        }
    }
    all.sort_by_key(|p| is_adopted(tree, p, child));
    let mut pair: Vec<&str> = all.iter().take(1).copied().collect();
    if let Some(&first) = all.first() {
        let spouses = tree.spouses(first);
        let partner = all[1..].iter().copied().find(|p| spouses.contains(p)).or(all.get(1).copied());
        pair.extend(partner);
    }
    pair.sort_by_key(|p| match tree.person(p).map(|p| p.gender) {
        Some(Gender::Male) => 0,
        Some(Gender::Female) => 2,
        _ => 1,
    });
    let rest = all.iter().filter(|p| !pair.contains(p)).map(|s| s.to_string()).collect();
    (pair.into_iter().map(String::from).collect(), rest)
}

pub fn layout(tree: &Tree, focus: &str, settings: Settings) -> Layout {
    let mut l = Layout::default();
    l.add(focus, Pos2::ZERO, Role::Focus);

    // ancestors
    let (pair, others) = parent_pair(tree, focus);
    if settings.ancestors > 0 {
        place_ancestors(tree, focus, &pair, Pos2::ZERO, 1, settings.ancestors, &mut l);
        // extra parents to the right of the ancestor block, dashed to the focus
        let mut x = l.bounds().right() + H_GAP + BOX.x / 2.0;
        for p in &others {
            let pos = Pos2::new(x, -ROW);
            l.add(p, pos, Role::OtherParent);
            l.edges.push(Edge { from: bottom(pos), to: top(Pos2::ZERO), kind: EdgeKind::Child { adopted: true } });
            x += STEP;
        }
    }

    // siblings to the left, eldest leftmost
    let focus_parents: HashSet<&str> = tree.parents(focus).into_iter().collect();
    let mut siblings: Vec<&str> = Vec::new();
    for p in &pair {
        for c in tree.children(p) {
            if c != focus && !siblings.contains(&c) {
                siblings.push(c);
            }
        }
    }
    let connector = parent_connector(&l, &pair);
    for (i, sib) in siblings.iter().enumerate() {
        let pos = Pos2::new(-STEP * (siblings.len() - i) as f32, 0.0);
        let sib_parents: HashSet<&str> = tree.parents(sib).into_iter().collect();
        let half = sib_parents != focus_parents;
        l.add(sib, pos, Role::Sibling { half });
        if settings.ancestors > 0 {
            // a half-sibling hangs from the shared parent only
            let from = if half {
                pair.iter()
                    .find(|p| sib_parents.contains(p.as_str()))
                    .and_then(|p| l.nodes.iter().find(|n| n.id == *p))
                    .map(|n| bottom(n.pos))
            } else {
                connector
            };
            if let Some(from) = from {
                let adopted = pair.iter().any(|p| is_adopted(tree, p, sib));
                l.edges.push(Edge { from, to: top(pos), kind: EdgeKind::Child { adopted } });
            }
        }
    }

    // spouses to the right
    let spouses = tree.spouses(focus);
    for (i, s) in spouses.iter().enumerate() {
        let pos = Pos2::new(STEP * (i + 1) as f32, 0.0);
        l.add(s, pos, Role::Spouse);
        l.edges.push(Edge {
            from: pos - Vec2::new(STEP - BOX.x / 2.0, 0.0),
            to: pos - Vec2::new(BOX.x / 2.0, 0.0),
            kind: EdgeKind::Couple,
        });
    }

    // descendants, grouped by the other parent
    if settings.descendants > 0 {
        let kids = tree.children(focus);
        let mut groups: Vec<(Pos2, Vec<&str>)> = Vec::new();
        for (i, s) in spouses.iter().enumerate() {
            let theirs: Vec<&str> = kids.iter().copied().filter(|k| tree.parents(k).contains(s)).collect();
            let gap_mid = Pos2::new(STEP * (i + 1) as f32 - STEP / 2.0, 0.0);
            groups.push((gap_mid, theirs));
        }
        let grouped: HashSet<&str> = groups.iter().flat_map(|(_, k)| k.iter().copied()).collect();
        groups.push((bottom(Pos2::ZERO), kids.iter().copied().filter(|k| !grouped.contains(k)).collect()));

        let widths: Vec<f32> = groups
            .iter()
            .flat_map(|(_, ks)| ks.iter().map(|k| subtree_width(tree, k, 2, settings.descendants)))
            .collect();
        let total: f32 = widths.iter().sum();
        let centre = STEP * spouses.len() as f32 / 2.0;
        let mut left = centre - total / 2.0;
        let mut w = widths.iter();
        for (from, ks) in &groups {
            for k in ks {
                let width = *w.next().expect("one width per child");
                let pos = Pos2::new(left + width / 2.0, ROW);
                l.add(k, pos, Role::Descendant);
                let adopted = is_adopted(tree, focus, k);
                l.edges.push(Edge { from: *from, to: top(pos), kind: EdgeKind::Child { adopted } });
                place_descendants(tree, k, pos, 2, settings.descendants, &mut l);
                left += width;
            }
        }
    }

    // markers for relatives beyond the view
    let drawn: HashSet<String> = l.nodes.iter().map(|n| n.id.clone()).collect();
    for n in &mut l.nodes {
        n.more_up = tree.parents(&n.id).iter().any(|p| !drawn.contains(*p));
        n.more_down = tree.children(&n.id).iter().any(|c| !drawn.contains(*c));
    }
    l
}

/// Where the edge to a child of `pair` starts: the middle of the couple line,
/// or the single parent's bottom.
fn parent_connector(l: &Layout, pair: &[String]) -> Option<Pos2> {
    let pos = |id: &String| l.nodes.iter().find(|n| n.id == *id).map(|n| n.pos);
    match pair {
        [a, b] => Some(Pos2::new((pos(a)?.x + pos(b)?.x) / 2.0, pos(a)?.y)),
        [a] => pos(a).map(bottom),
        _ => None,
    }
}

fn ancestor_width(tree: &Tree, person: &str, generation: u32, max: u32) -> f32 {
    if generation > max {
        return STEP;
    }
    let (pair, _) = parent_pair(tree, person);
    let sum: f32 = pair.iter().map(|p| ancestor_width(tree, p, generation + 1, max)).sum();
    sum.max(STEP)
}

fn place_ancestors(
    tree: &Tree,
    child: &str,
    pair: &[String],
    child_pos: Pos2,
    generation: u32,
    max: u32,
    l: &mut Layout,
) {
    if pair.is_empty() || generation > max {
        return;
    }
    let widths: Vec<f32> = pair.iter().map(|p| ancestor_width(tree, p, generation + 1, max)).collect();
    let total: f32 = widths.iter().sum();
    let mut left = child_pos.x - total / 2.0;
    let y = -ROW * generation as f32;
    let mut positions = Vec::new();
    for (p, w) in pair.iter().zip(&widths) {
        let pos = Pos2::new(left + w / 2.0, y);
        l.add(p, pos, Role::Ancestor);
        positions.push(pos);
        left += w;
    }
    let from = match positions.as_slice() {
        [a, b] => {
            l.edges.push(Edge {
                from: *a + Vec2::new(BOX.x / 2.0, 0.0),
                to: *b - Vec2::new(BOX.x / 2.0, 0.0),
                kind: EdgeKind::Couple,
            });
            Pos2::new((a.x + b.x) / 2.0, y)
        }
        [a] => bottom(*a),
        _ => unreachable!("at most two parents"),
    };
    let adopted = pair.iter().any(|p| is_adopted(tree, p, child));
    l.edges.push(Edge { from, to: top(child_pos), kind: EdgeKind::Child { adopted } });
    for (p, pos) in pair.iter().zip(positions) {
        let (grand, _) = parent_pair(tree, p);
        place_ancestors(tree, p, &grand, pos, generation + 1, max, l);
    }
}

fn subtree_width(tree: &Tree, id: &str, depth: u32, max: u32) -> f32 {
    if depth > max {
        return STEP;
    }
    let sum: f32 = tree.children(id).iter().map(|k| subtree_width(tree, k, depth + 1, max)).sum();
    sum.max(STEP)
}

fn place_descendants(tree: &Tree, id: &str, pos: Pos2, depth: u32, max: u32, l: &mut Layout) {
    if depth > max {
        return;
    }
    let kids = tree.children(id);
    let total: f32 = kids.iter().map(|k| subtree_width(tree, k, depth + 1, max)).sum();
    let mut left = pos.x - total / 2.0;
    for kid in kids {
        let w = subtree_width(tree, kid, depth + 1, max);
        let child_pos = Pos2::new(left + w / 2.0, pos.y + ROW);
        l.add(kid, child_pos, Role::Descendant);
        let adopted = is_adopted(tree, id, kid);
        l.edges.push(Edge { from: bottom(pos), to: top(child_pos), kind: EdgeKind::Child { adopted } });
        place_descendants(tree, kid, child_pos, depth + 1, max, l);
        left += w;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use familyx::Package;

    fn example() -> Tree {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../familyx/tests/data/example.familyx");
        Tree::from_doc(&Package::open(path).unwrap().doc)
    }

    #[test]
    fn boxes_never_overlap() {
        let tree = example();
        for settings in
            [Settings::default(), Settings { ancestors: 5, descendants: 5 }, Settings { ancestors: 0, descendants: 0 }]
        {
            for p in &tree.people {
                let l = layout(&tree, &p.id, settings);
                for (i, a) in l.nodes.iter().enumerate() {
                    for b in &l.nodes[i + 1..] {
                        let (ra, rb) = (Rect::from_center_size(a.pos, BOX), Rect::from_center_size(b.pos, BOX));
                        assert!(
                            !ra.intersects(rb.shrink(1.0)),
                            "{} overlaps {} around {}",
                            a.id,
                            b.id,
                            p.display_name()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn shows_relatives_and_markers() {
        let tree = example();
        let id = |name: &str| tree.people.iter().find(|p| p.display_name() == name).unwrap().id.clone();
        let l = layout(&tree, &id("Erik Lindqvist"), Settings { ancestors: 1, descendants: 1 });
        let role = |name: &str| l.nodes.iter().find(|n| n.id == id(name)).map(|n| n.role);
        assert_eq!(role("Karl Lindqvist"), Some(Role::Ancestor));
        assert_eq!(role("Anna Lindqvist"), Some(Role::Sibling { half: false }));
        assert_eq!(role("Sofia Rahe"), Some(Role::Spouse));
        assert_eq!(role("Nora Lindqvist"), Some(Role::Descendant));
        assert_eq!(role("Arthur Lindqvist"), None);
        let karl = l.nodes.iter().find(|n| n.id == id("Karl Lindqvist")).unwrap();
        assert!(karl.more_up, "grandparents exist beyond one generation");
        let leo = l.nodes.iter().find(|n| n.id == id("Leo Lindqvist")).unwrap();
        assert!(leo.more_down, "Leo has a daughter beyond one generation");
        assert!(l.edges.iter().any(|e| e.kind == EdgeKind::Child { adopted: true }), "Nora is adopted");
    }
}

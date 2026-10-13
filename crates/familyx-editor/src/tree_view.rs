//! Zoomable pedigree canvas: ancestors above, focus person (+ spouses) in the
//! middle, descendants below.

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};
use familyx::model::{Gender, Id, Tree};

const BOX: Vec2 = Vec2::new(170.0, 54.0);
const H_GAP: f32 = 18.0;
const V_GAP: f32 = 46.0;
const ANCESTOR_GENERATIONS: u32 = 3;
const DESCENDANT_GENERATIONS: u32 = 2;

pub struct TreeView {
    pan: Vec2,
    zoom: f32,
    last_focus: Option<Id>,
    /// Fit every frame (follows window resizes) until the user pans or zooms.
    auto_fit: bool,
}

impl Default for TreeView {
    fn default() -> Self {
        TreeView { pan: Vec2::ZERO, zoom: 1.0, last_focus: None, auto_fit: true }
    }
}

pub enum TreeAction {
    Select(Id),
    Focus(Id),
}

struct Layout {
    boxes: Vec<(Id, Pos2)>,
    /// Parent box centre-bottom → child box centre-top, in layout coordinates.
    edges: Vec<(Pos2, Pos2)>,
    /// Spouse connectors.
    marriages: Vec<(Pos2, Pos2)>,
}

impl TreeView {
    pub fn ui(&mut self, ui: &mut egui::Ui, tree: &Tree, focus: &str, selected: Option<&str>) -> Option<TreeAction> {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = response.rect;
        let layout = layout(tree, focus);
        if self.last_focus.as_deref() != Some(focus) {
            self.auto_fit = true;
            self.last_focus = Some(focus.to_string());
        }
        if self.auto_fit {
            self.fit(&layout, canvas);
        }

        if response.dragged() && response.drag_delta() != Vec2::ZERO {
            self.pan += response.drag_delta();
            self.auto_fit = false;
        }
        if response.hovered() {
            let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            let factor = zoom_delta * (scroll.y / 400.0).exp();
            if factor != 1.0 {
                // zoom around the pointer
                if let Some(pointer) = response.hover_pos() {
                    let origin = canvas.center() + self.pan;
                    let new_zoom = (self.zoom * factor).clamp(0.25, 3.0);
                    let applied = new_zoom / self.zoom;
                    self.pan += (pointer - origin) * (1.0 - applied);
                    self.zoom = new_zoom;
                    self.auto_fit = false;
                }
            }
        }

        let origin = canvas.center() + self.pan;
        let to_screen = |p: Pos2| origin + p.to_vec2() * self.zoom;
        let line = Stroke::new(1.5 * self.zoom.max(0.5), ui.visuals().weak_text_color());

        for (from, to) in &layout.edges {
            let (a, b) = (to_screen(*from), to_screen(*to));
            let mid_y = (a.y + b.y) / 2.0;
            painter.line_segment([a, Pos2::new(a.x, mid_y)], line);
            painter.line_segment([Pos2::new(a.x, mid_y), Pos2::new(b.x, mid_y)], line);
            painter.line_segment([Pos2::new(b.x, mid_y), b], line);
        }
        for (a, b) in &layout.marriages {
            painter.line_segment([to_screen(*a), to_screen(*b)], Stroke::new(line.width * 1.5, line.color));
        }

        let mut action = None;
        let pointer = response.hover_pos();
        for (id, centre) in &layout.boxes {
            let Some(person) = tree.person(id) else { continue };
            let rect = Rect::from_center_size(to_screen(*centre), BOX * self.zoom);
            if !rect.intersects(canvas) {
                continue;
            }
            let hovered = pointer.is_some_and(|p| rect.contains(p));
            let fill = match person.gender {
                Gender::Male => Color32::from_rgb(0x2f, 0x5d, 0x8a),
                Gender::Female => Color32::from_rgb(0x8a, 0x3f, 0x5d),
                Gender::Unknown => Color32::from_rgb(0x55, 0x55, 0x55),
            };
            let fill = if hovered { fill.gamma_multiply(1.25) } else { fill };
            let stroke = if Some(id.as_str()) == selected {
                Stroke::new(3.0, Color32::from_rgb(0xf0, 0xc0, 0x40))
            } else if id == focus {
                Stroke::new(2.0, Color32::WHITE)
            } else {
                Stroke::new(1.0, Color32::from_gray(30))
            };
            let radius = CornerRadius::same((6.0 * self.zoom).round() as u8);
            painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);

            let font = |size: f32| FontId::proportional(size * self.zoom);
            let clip = painter.with_clip_rect(rect.shrink(4.0 * self.zoom));
            clip.text(
                rect.center() - Vec2::new(0.0, 8.0 * self.zoom),
                Align2::CENTER_CENTER,
                person.display_name(),
                font(14.0),
                Color32::WHITE,
            );
            clip.text(
                rect.center() + Vec2::new(0.0, 12.0 * self.zoom),
                Align2::CENTER_CENTER,
                person.lifespan(),
                font(12.0),
                Color32::from_gray(220),
            );

            if hovered && response.double_clicked() {
                action = Some(TreeAction::Focus(id.clone()));
            } else if hovered && response.clicked() {
                action = Some(TreeAction::Select(id.clone()));
            }
        }

        if action.is_none() && response.double_clicked() {
            self.auto_fit = true;
        }
        let fit_button = Rect::from_min_size(canvas.right_top() + Vec2::new(-52.0, 8.0), Vec2::new(44.0, 22.0));
        if ui.put(fit_button, egui::Button::new("Fit")).on_hover_text("Fit tree to view (or double-click background)").clicked() {
            self.auto_fit = true;
        }

        painter.text(
            canvas.left_bottom() + Vec2::new(8.0, -8.0),
            Align2::LEFT_BOTTOM,
            "drag to pan · scroll to zoom · click to select · double-click a person to centre, background to fit",
            FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
        action
    }
}

impl TreeView {
    /// Zoom and pan so the whole layout fits the canvas.
    fn fit(&mut self, layout: &Layout, canvas: Rect) {
        let bounds = layout
            .boxes
            .iter()
            .map(|(_, c)| Rect::from_center_size(*c, BOX))
            .reduce(|a, b| a.union(b))
            .unwrap_or(Rect::from_center_size(Pos2::ZERO, BOX))
            .expand(24.0);
        self.zoom = (canvas.width() / bounds.width()).min(canvas.height() / bounds.height()).clamp(0.35, 1.0);
        self.pan = -bounds.center().to_vec2() * self.zoom;
    }
}

fn layout(tree: &Tree, focus: &str) -> Layout {
    let mut l = Layout { boxes: Vec::new(), edges: Vec::new(), marriages: Vec::new() };
    let step = BOX.x + H_GAP;
    let row = BOX.y + V_GAP;

    // focus row: focus then spouses to the right
    l.boxes.push((focus.to_string(), Pos2::ZERO));
    for (i, spouse) in tree.spouses(focus).into_iter().enumerate() {
        let x = step * (i as f32 + 1.0);
        let prev_x = x - step;
        l.boxes.push((spouse.to_string(), Pos2::new(x, 0.0)));
        l.marriages.push((Pos2::new(prev_x + BOX.x / 2.0, 0.0), Pos2::new(x - BOX.x / 2.0, 0.0)));
    }

    // ancestors: binary slots per generation, father on the left
    // the top generation has 2^G slots of one box each
    let width = step * 2f32.powi(ANCESTOR_GENERATIONS as i32);
    let mut generation: Vec<(Option<String>, f32)> = vec![(Some(focus.to_string()), 0.0)];
    for g in 1..=ANCESTOR_GENERATIONS {
        let offset = width / 2f32.powi(g as i32 + 1);
        let y = -row * g as f32;
        let mut next = Vec::new();
        for (child, cx) in &generation {
            let parents = child.as_deref().map(|c| ordered_parents(tree, c)).unwrap_or([None, None]);
            for (k, parent) in parents.into_iter().enumerate() {
                let x = cx + if k == 0 { -offset } else { offset };
                if let Some(p) = &parent {
                    l.boxes.push((p.clone(), Pos2::new(x, y)));
                    l.edges.push((
                        Pos2::new(x, y + BOX.y / 2.0),
                        Pos2::new(*cx, y + row - BOX.y / 2.0),
                    ));
                }
                next.push((parent, x));
            }
        }
        generation = next;
    }

    // descendants: tidy layout by subtree width
    place_descendants(tree, focus, 0.0, 0.0, 1, &mut l);
    l
}

/// At most two parents to draw: non-adoptive links first, preferring a
/// couple, father on the left.
fn ordered_parents(tree: &Tree, child: &str) -> [Option<String>; 2] {
    let mut parents: Vec<&str> = tree
        .parent_links
        .iter()
        .filter(|l| l.child == child)
        .map(|l| (l.modifier == "Adopted", l.parent.as_str()))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|(_, p)| p)
        .collect();
    if parents.len() > 2 {
        let first = parents[0];
        let spouses = tree.spouses(first);
        let partner = parents[1..].iter().copied().find(|p| spouses.contains(p)).unwrap_or(parents[1]);
        parents = vec![first, partner];
    }
    parents.sort_by_key(|p| match tree.person(p).map(|p| p.gender) {
        Some(Gender::Male) => 0,
        Some(Gender::Female) => 2,
        _ => 1,
    });
    [parents.first().map(|s| s.to_string()), parents.get(1).map(|s| s.to_string())]
}

fn subtree_width(tree: &Tree, id: &str, depth: u32) -> f32 {
    let step = BOX.x + H_GAP;
    if depth > DESCENDANT_GENERATIONS {
        return step;
    }
    let kids = tree.children(id);
    let sum: f32 = kids.iter().map(|k| subtree_width(tree, k, depth + 1)).sum();
    sum.max(step)
}

fn place_descendants(tree: &Tree, id: &str, x: f32, y: f32, depth: u32, l: &mut Layout) {
    if depth > DESCENDANT_GENERATIONS {
        return;
    }
    let kids = tree.children(id);
    let total: f32 = kids.iter().map(|k| subtree_width(tree, k, depth + 1)).sum();
    let mut left = x - total / 2.0;
    let child_y = y + BOX.y + V_GAP;
    for kid in kids {
        let w = subtree_width(tree, kid, depth + 1);
        let cx = left + w / 2.0;
        l.boxes.push((kid.to_string(), Pos2::new(cx, child_y)));
        l.edges.push((Pos2::new(x, y + BOX.y / 2.0), Pos2::new(cx, child_y - BOX.y / 2.0)));
        place_descendants(tree, kid, cx, child_y, depth + 1, l);
        left += w;
    }
}

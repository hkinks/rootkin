//! Zoomable family tree canvas drawn from [`crate::layout`].

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, RichText, Sense, Shape, Stroke, StrokeKind, TextureId,
    Vec2,
};
use familyx::model::{Gender, Id, Person, Tree};

use crate::layout::{self, EdgeKind, Layout, Role, Settings, BOX};

pub struct TreeView {
    pan: Vec2,
    zoom: f32,
    /// Animation targets for `pan`/`zoom`.
    target: Option<(Vec2, f32)>,
    last_focus: Option<Id>,
    /// Keep fitting (e.g. on window resize) until the user pans or zooms.
    auto_fit: bool,
    pub settings: Settings,
    /// Screen rectangle of the canvas in the last frame (for image export).
    pub canvas: Rect,
}

impl Default for TreeView {
    fn default() -> Self {
        TreeView {
            pan: Vec2::ZERO,
            zoom: 1.0,
            target: None,
            last_focus: None,
            auto_fit: true,
            settings: Settings::default(),
            canvas: Rect::NOTHING,
        }
    }
}

pub enum TreeAction {
    /// Select the person and centre the tree on them.
    Focus(Id),
    Back,
    Forward,
}

/// A photo for a person: texture and the UV rectangle to show (face crop).
pub type PhotoFn<'a> = dyn FnMut(&Person) -> Option<(TextureId, Rect)> + 'a;

pub struct NavState {
    pub can_back: bool,
    pub can_forward: bool,
}

fn gender_fill(g: Gender) -> Color32 {
    match g {
        Gender::Male => Color32::from_rgb(0x2f, 0x5d, 0x8a),
        Gender::Female => Color32::from_rgb(0x8a, 0x3f, 0x5d),
        Gender::Other => Color32::from_rgb(0x5d, 0x6e, 0x3f),
        Gender::Unknown => Color32::from_rgb(0x55, 0x55, 0x55),
    }
}

pub fn gender_symbol(g: Gender) -> &'static str {
    match g {
        Gender::Male => "♂",
        Gender::Female => "♀",
        Gender::Other => "*",
        Gender::Unknown => "",
    }
}

/// The small line under a name in a tree box: gender symbol and years.
/// Nothing at all (not a lone "?") for someone without either.
fn box_subtitle(person: &Person, half: bool) -> String {
    let mut sub = format!("{} {}", gender_symbol(person.gender), person.lifespan());
    if half {
        sub.push_str(" · half");
    }
    sub.trim().to_string()
}

impl TreeView {
    pub fn fit_now(&mut self) {
        self.auto_fit = true;
    }

    fn zoom_by(&mut self, factor: f32, around: Pos2, canvas: Rect) {
        let origin = canvas.center() + self.pan;
        let new_zoom = (self.zoom * factor).clamp(0.2, 3.0);
        self.pan += (around - origin) * (1.0 - new_zoom / self.zoom);
        self.zoom = new_zoom;
        self.auto_fit = false;
        self.target = None;
    }

    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        tree: &Tree,
        focus: &str,
        nav: NavState,
        photo: &mut PhotoFn,
    ) -> Option<TreeAction> {
        let (response, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = response.rect;
        self.canvas = canvas;
        let layout = layout::layout(tree, focus, self.settings);
        if self.last_focus.as_deref() != Some(focus) {
            self.auto_fit = true;
            self.last_focus = Some(focus.to_string());
        }
        if self.auto_fit {
            self.target = Some(fit(&layout, canvas));
        }
        self.animate(ui.ctx());

        if response.dragged() && response.drag_delta() != Vec2::ZERO {
            self.pan += response.drag_delta();
            self.auto_fit = false;
            self.target = None;
        }
        if response.hovered() {
            let (zoom_delta, scroll) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta));
            let factor = zoom_delta * (scroll.y / 400.0).exp();
            if factor != 1.0 {
                if let Some(pointer) = response.hover_pos() {
                    self.zoom_by(factor, pointer, canvas);
                }
            }
        }
        if response.double_clicked() {
            self.auto_fit = true;
        }

        let mut action = self.draw(ui, &painter, &layout, tree, focus, photo);
        action = action.or(self.controls(ui, canvas, tree, focus, &nav));
        action
    }

    /// Ease `pan`/`zoom` towards the target.
    fn animate(&mut self, ctx: &egui::Context) {
        let Some((pan, zoom)) = self.target else { return };
        let t = 0.25;
        self.pan += (pan - self.pan) * t;
        self.zoom += (zoom - self.zoom) * t;
        if (pan - self.pan).length() < 0.5 && (zoom - self.zoom).abs() < 0.002 {
            self.pan = pan;
            self.zoom = zoom;
            if !self.auto_fit {
                self.target = None;
            }
        } else {
            ctx.request_repaint();
        }
    }

    fn draw(
        &self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        layout: &Layout,
        tree: &Tree,
        focus: &str,
        photo: &mut PhotoFn,
    ) -> Option<TreeAction> {
        let canvas = self.canvas;
        let origin = canvas.center() + self.pan;
        let to_screen = |p: Pos2| origin + p.to_vec2() * self.zoom;
        let line_color = ui.visuals().text_color().gamma_multiply(0.55);
        let line = Stroke::new((1.5 * self.zoom).max(1.0), line_color);

        for edge in &layout.edges {
            let (a, b) = (to_screen(edge.from), to_screen(edge.to));
            match edge.kind {
                EdgeKind::Couple => {
                    painter.line_segment([a, b], Stroke::new(line.width * 1.6, line_color));
                }
                EdgeKind::Child { adopted } => {
                    let mid_y = b.y - (b.y - a.y).abs().min(50.0 * self.zoom) / 2.0;
                    let points = [a, Pos2::new(a.x, mid_y), Pos2::new(b.x, mid_y), b];
                    if adopted {
                        for w in points.windows(2) {
                            painter.extend(Shape::dashed_line(&[w[0], w[1]], line, 6.0 * self.zoom, 4.0 * self.zoom));
                        }
                    } else {
                        painter.add(Shape::line(points.to_vec(), line));
                    }
                }
            }
        }

        let mut action = None;
        for node in &layout.nodes {
            let Some(person) = tree.person(&node.id) else { continue };
            let rect = Rect::from_center_size(to_screen(node.pos), BOX * self.zoom);
            if !rect.intersects(canvas) {
                continue;
            }
            let label = format!("{} {}", person.display_name(), person.lifespan());
            let response = ui
                .interact(
                    rect,
                    ui.id().with(("person", &node.id, node.pos.x as i32, node.pos.y as i32)),
                    Sense::click(),
                )
                .on_hover_text(match node.role {
                    Role::Sibling { half: true } => format!("{label}\nhalf-sibling"),
                    Role::OtherParent => format!("{label}\nadoptive or other parent"),
                    _ => label.clone(),
                })
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            if response.clicked() && node.id != focus {
                action = Some(TreeAction::Focus(node.id.clone()));
            }
            self.draw_box(painter, rect, person, node, response.hovered(), node.id == focus, photo);
        }
        action
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_box(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        person: &Person,
        node: &layout::Node,
        hovered: bool,
        is_focus: bool,
        photo: &mut PhotoFn,
    ) {
        let z = self.zoom;
        let fill = gender_fill(person.gender);
        let fill = if hovered { fill.gamma_multiply(1.3) } else { fill };
        let stroke = if is_focus {
            Stroke::new(3.0, Color32::from_rgb(0xf0, 0xc0, 0x40))
        } else {
            Stroke::new(1.0, Color32::from_black_alpha(90))
        };
        let radius = CornerRadius::same((7.0 * z).round().min(255.0) as u8);
        painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);

        let mut text_rect = rect.shrink(6.0 * z);
        if let Some((texture, uv)) = photo(person) {
            let side = rect.height() - 8.0 * z;
            let img = Rect::from_min_size(rect.left_top() + Vec2::splat(4.0 * z), Vec2::splat(side));
            painter.image(texture, img, uv, Color32::WHITE);
            text_rect.min.x = img.right() + 6.0 * z;
        }
        let font = |size: f32| FontId::proportional(size * z);
        let clip = painter.with_clip_rect(text_rect.intersect(self.canvas));
        let c = text_rect.center();
        clip.text(
            c - Vec2::new(0.0, 9.0 * z),
            Align2::CENTER_CENTER,
            person.display_name(),
            font(15.0),
            Color32::WHITE,
        );
        let half = matches!(node.role, Role::Sibling { half: true });
        clip.text(
            c + Vec2::new(0.0, 12.0 * z),
            Align2::CENTER_CENTER,
            box_subtitle(person, half),
            font(13.0),
            Color32::from_gray(225),
        );

        // more relatives beyond the view: small triangles above/below the box
        let tri = |tip: Pos2, up: bool| {
            let (w, h) = (6.0 * z.max(0.6), 6.0 * z.max(0.6));
            let base_y = if up { tip.y + h } else { tip.y - h };
            painter.add(Shape::convex_polygon(
                vec![tip, Pos2::new(tip.x - w, base_y), Pos2::new(tip.x + w, base_y)],
                Color32::from_gray(190),
                Stroke::NONE,
            ));
        };
        if node.more_up {
            tri(rect.center_top() - Vec2::new(0.0, 12.0 * z.max(0.6)), true);
        }
        if node.more_down {
            tri(rect.center_bottom() + Vec2::new(0.0, 12.0 * z.max(0.6)), false);
        }
    }

    /// Overlay: header with back/forward, zoom and generation controls, help.
    fn controls(
        &mut self,
        ui: &mut egui::Ui,
        canvas: Rect,
        tree: &Tree,
        focus: &str,
        nav: &NavState,
    ) -> Option<TreeAction> {
        let mut action = None;
        let name = tree.person(focus).map(|p| p.display_name()).unwrap_or_default();
        let header =
            Rect::from_min_size(canvas.left_top() + Vec2::new(8.0, 8.0), Vec2::new(canvas.width() - 16.0, 30.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(header), |ui| {
            ui.horizontal(|ui| {
                if ui.add_enabled(nav.can_back, egui::Button::new("◀ Back")).on_hover_text("Alt+Left").clicked() {
                    action = Some(TreeAction::Back);
                }
                if ui.add_enabled(nav.can_forward, egui::Button::new("Forward ▶")).on_hover_text("Alt+Right").clicked()
                {
                    action = Some(TreeAction::Forward);
                }
                ui.label(egui::RichText::new(format!("Family of {name}")).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Fit").on_hover_text("Show the whole tree (or double-click the background)").clicked()
                    {
                        self.auto_fit = true;
                    }
                    if ui.button(" + ").on_hover_text("Zoom in").clicked() {
                        self.zoom_by(1.25, canvas.center(), canvas);
                    }
                    if ui.button(" − ").on_hover_text("Zoom out").clicked() {
                        self.zoom_by(0.8, canvas.center(), canvas);
                    }
                    ui.label(RichText::new(" ? ").strong()).on_hover_text(
                        "Click a person to show their family\n\
                         Drag the background to move, scroll to zoom\n\
                         Arrow keys: up = parent, down = child, left/right = siblings and partners\n\
                         Alt+Left / Alt+Right: back / forward",
                    );
                });
            });
        });
        let footer = Rect::from_min_max(
            canvas.left_bottom() - Vec2::new(-8.0, 34.0),
            canvas.right_bottom() - Vec2::new(8.0, 4.0),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(footer), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut s = self.settings;
                let mut gens = |ui: &mut egui::Ui, label: &str, value: &mut u32| {
                    let before = *value;
                    ui.add(egui::DragValue::new(value).range(0..=8)).on_hover_text("Number of generations shown");
                    ui.label(label);
                    if *value != before {
                        self.auto_fit = true;
                    }
                };
                gens(ui, "Descendants", &mut s.descendants);
                gens(ui, "Ancestors", &mut s.ancestors);
                self.settings = s;
            });
        });
        action
    }
}
/// Pan and zoom that fit the layout into the canvas.
fn fit(layout: &Layout, canvas: Rect) -> (Vec2, f32) {
    let bounds = layout.bounds().expand(30.0);
    // leave room for the header and help line
    let avail = canvas.shrink2(Vec2::new(0.0, 40.0));
    let zoom = (avail.width() / bounds.width()).min(avail.height() / bounds.height()).clamp(0.3, 1.1);
    (-bounds.center().to_vec2() * zoom, zoom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_gender_without_dates_shows_nothing() {
        let mut p = Person::default();
        assert_eq!(box_subtitle(&p, false), "");
        assert_eq!(box_subtitle(&p, true), "· half");
        p.gender = Gender::Female;
        assert_eq!(box_subtitle(&p, false), "♀");
    }
}

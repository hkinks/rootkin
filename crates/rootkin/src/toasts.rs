//! Short-lived notifications at the bottom centre of the main area.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText};

const INFO_TIME: Duration = Duration::from_secs(5);
const ERROR_TIME: Duration = Duration::from_secs(12);

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Info,
    Error,
}

struct Toast {
    text: String,
    kind: Kind,
    created: Instant,
    /// Offer an "Undo" button with the toast.
    undo: bool,
}

#[derive(Default)]
pub struct Toasts {
    items: Vec<Toast>,
}

impl Toasts {
    pub fn info(&mut self, text: impl Into<String>) {
        self.push(text.into(), Kind::Info, false);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.push(text.into(), Kind::Error, false);
    }

    /// An info toast with an Undo button.
    pub fn undoable(&mut self, text: impl Into<String>) {
        self.push(text.into(), Kind::Info, true);
    }

    fn push(&mut self, text: String, kind: Kind, undo: bool) {
        self.items.retain(|t| t.text != text);
        self.items.push(Toast { text, kind, created: Instant::now(), undo });
        if self.items.len() > 4 {
            self.items.remove(0);
        }
    }

    /// Draw at the bottom centre of `area`; returns true if the user clicked "Undo".
    pub fn show(&mut self, ctx: &egui::Context, area: egui::Rect) -> bool {
        let now = Instant::now();
        self.items.retain(|t| now - t.created < if t.kind == Kind::Error { ERROR_TIME } else { INFO_TIME });
        if self.items.is_empty() {
            return false;
        }
        ctx.request_repaint_after(Duration::from_millis(250));
        let mut undo = false;
        let mut dismiss = None;
        egui::Area::new(egui::Id::new("toasts"))
            .pivot(egui::Align2::CENTER_BOTTOM)
            .fixed_pos(area.center_bottom() - egui::vec2(0.0, 16.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                for (i, t) in self.items.iter().enumerate() {
                    let fill = match t.kind {
                        Kind::Info => ui.visuals().extreme_bg_color,
                        Kind::Error => Color32::from_rgb(0x7a, 0x1f, 0x1f),
                    };
                    egui::Frame::popup(ui.style()).fill(fill).show(ui, |ui| {
                        ui.set_max_width(420.0f32.min(area.width() - 32.0));
                        ui.horizontal(|ui| {
                            let text = RichText::new(&t.text);
                            ui.label(if t.kind == Kind::Error { text.color(Color32::WHITE) } else { text });
                            if t.undo && ui.button("Undo").clicked() {
                                undo = true;
                                dismiss = Some(i);
                            }
                            if crate::small_button(ui, "✖").on_hover_text("Dismiss").clicked() {
                                dismiss = Some(i);
                            }
                        });
                    });
                }
            });
        if let Some(i) = dismiss {
            self.items.remove(i);
        }
        undo
    }
}

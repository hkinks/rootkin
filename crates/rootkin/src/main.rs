mod app;
mod export;
mod formats;
mod layout;
mod person_panel;
mod photos;
mod recent;
mod relation_dialog;
mod toasts;
mod tree_view;

use eframe::egui;

/// Larger text and click targets than egui's defaults.
fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|style| {
        use egui::{FontId, TextStyle};
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(12.5)),
            (TextStyle::Body, FontId::proportional(15.0)),
            (TextStyle::Button, FontId::proportional(15.0)),
            (TextStyle::Heading, FontId::proportional(21.0)),
            (TextStyle::Monospace, FontId::monospace(14.0)),
        ]
        .into();
        style.spacing.interact_size.y = 26.0;
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    });
}

fn icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../../../assets/rootkin.png")).expect("bundled icon is valid");
    let img = img.to_rgba8();
    egui::IconData { width: img.width(), height: img.height(), rgba: img.into_raw() }
}

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Rootkin")
            .with_inner_size([1400.0, 900.0])
            .with_app_id("rootkin")
            .with_icon(icon()),
        ..Default::default()
    };
    eframe::run_native(
        "Rootkin",
        options,
        Box::new(move |cc| {
            style(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, path)))
        }),
    )
}

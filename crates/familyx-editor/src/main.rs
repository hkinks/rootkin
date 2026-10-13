mod app;
mod person_panel;
mod photos;
mod relation_dialog;
mod tree_view;

fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Family Tree")
            .with_inner_size([1400.0, 900.0])
            .with_app_id("familyx-editor"),
        ..Default::default()
    };
    eframe::run_native("Family Tree", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, path)))))
}

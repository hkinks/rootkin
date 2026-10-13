//! Headless UI tests driving the real app through its accessibility tree.

use eframe::egui;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use familyx::Package;

use crate::app::App;

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../familyx/tests/data/example.familyx");

/// The app with the example tree loaded (without touching the recent-files list).
fn harness() -> Harness<'static, App> {
    let mut harness = Harness::builder().with_size([1400.0, 900.0]).build_eframe(|cc| {
        let mut app = App::new(cc, None);
        app.load(Package::open(EXAMPLE).unwrap(), None);
        app
    });
    harness.run_steps(4);
    harness
}

fn selected_name(h: &Harness<App>) -> String {
    let app = h.state();
    app.selected.as_deref().and_then(|id| app.tree.person(id)).map(|p| p.display_name()).unwrap_or_default()
}

#[test]
fn opens_at_the_home_person() {
    let h = harness();
    assert_eq!(selected_name(&h), "Erik Lindqvist");
    h.get_by_label("Family of Erik Lindqvist");
}

#[test]
fn clicking_the_people_list_selects_and_centres() {
    let mut h = harness();
    h.get_all_by_label("Leo Lindqvist  b. 1979").next().unwrap().click();
    h.run_steps(4);
    assert_eq!(selected_name(&h), "Leo Lindqvist");
    h.get_by_label("Family of Leo Lindqvist");
}

#[test]
fn clicking_a_tree_box_centres_and_back_returns() {
    let mut h = harness();
    h.get_by_label("Sofia Rahe b. 1955").click();
    h.run_steps(4);
    assert_eq!(selected_name(&h), "Sofia Rahe");
    h.get_by_label("◀ Back").click();
    h.run_steps(4);
    assert_eq!(selected_name(&h), "Erik Lindqvist");
}

#[test]
fn arrow_keys_move_between_relatives() {
    let mut h = harness();
    h.key_press(egui::Key::ArrowUp);
    h.run_steps(3);
    assert_eq!(selected_name(&h), "Karl Lindqvist");
    h.key_press(egui::Key::ArrowDown);
    h.run_steps(3);
    assert_eq!(selected_name(&h), "Erik Lindqvist");
}

#[test]
fn edits_are_undoable_and_marked_unsaved() {
    let mut h = harness();
    h.get_all_by_label("Leo Lindqvist  b. 1979").next().unwrap().click();
    h.run_steps(3);
    h.get_by_label("Make home person").click();
    h.run_steps(3);
    assert_eq!(h.state().tree.home_person.as_deref(), h.state().selected.as_deref());
    h.get_by_label_contains("Unsaved changes");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run_steps(3);
    assert_eq!(
        h.state().tree.person(h.state().tree.home_person.as_deref().unwrap()).unwrap().display_name(),
        "Erik Lindqvist"
    );
}

#[test]
fn welcome_screen_starts_a_new_tree() {
    let mut h = Harness::builder().with_size([1200.0, 800.0]).build_eframe(|cc| App::new(cc, None));
    h.run_steps(3);
    h.get_by_label("Start a new family tree").click();
    h.run_steps(3);
    assert!(h.state().tree.people.is_empty());
    h.get_by_label("New person");
}

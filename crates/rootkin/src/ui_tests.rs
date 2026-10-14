//! Headless UI tests driving the real app through its accessibility tree.

use eframe::egui;
use egui::accesskit::Role;
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

/// The text field currently holding `value`.
fn text_input<'a>(h: &'a Harness<App>, value: &'a str) -> egui_kittest::Node<'a> {
    h.get_all_by(move |n| n.role() == Role::TextInput && n.value().as_deref() == Some(value)).next().unwrap()
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

#[test]
fn sources_window_edits_and_deletes_sources() {
    let mut h = harness();
    h.state_mut().sources_ui.open = true;
    h.run_steps(3);
    h.get_by_label("Estonian census 1934  (1 citation)").click();
    h.run_steps(3);
    assert_eq!(h.state().sources_ui.editing_title(), Some("Estonian census 1934"));

    // rename through the form
    text_input(&h, "Estonian census 1934").focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    text_input(&h, "Estonian census 1934").type_text("Census of 1934");
    h.run_steps(3);
    h.get_by_label("Save").click();
    h.run_steps(3);
    let census = h.state().tree.sources.iter().find(|s| s.title == "Census of 1934").expect("renamed").id.clone();
    h.get_by_label_contains("Unsaved changes");

    // deleting asks first and takes the citation along
    h.get_by_label_contains("Delete…").click();
    h.run_steps(3);
    h.get_by_label_contains("removes its 1 citation");
    h.get_by_label("Delete").click();
    h.run_steps(3);
    assert!(h.state().tree.source(&census).is_none());
    assert!(familyx::edit::check(&h.state().pkg.as_ref().unwrap().doc).is_empty());
}

#[test]
fn citing_a_source_from_an_event() {
    let mut h = harness();
    let id = h.state().tree.sources[0].id.clone();
    let before = h.state().tree.citation_count(&id);
    h.get_all_by(|n| n.role() == Role::ComboBox && n.value().as_deref() == Some("+ Cite source"))
        .next()
        .unwrap()
        .click();
    h.run_steps(3);
    let first = h.state().tree.sources[0].display_title();
    h.get_by_label(&first).click();
    h.run_steps(3);
    assert_eq!(h.state().tree.citation_count(&id), before + 1);
    h.get_by_label_contains("Unsaved changes");
}

const SMALL_GED: &str = "0 HEAD\n1 CHAR UTF-8\n0 @I1@ INDI\n1 NAME Ada /Test/\n0 TRLR\n";

fn temp_ged(name: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("rootkin-test-{}-{name}.ged", std::process::id()));
    std::fs::write(&path, SMALL_GED).unwrap();
    path
}

#[test]
fn import_asks_before_replacing_unsaved_changes() {
    let path = temp_ged("guard");
    let mut h = harness();
    h.get_all_by_label("Leo Lindqvist  b. 1979").next().unwrap().click();
    h.run_steps(3);
    h.get_by_label("Make home person").click();
    h.run_steps(3);
    h.get_by_label_contains("Unsaved changes");

    h.state_mut().request_import(path.clone());
    h.run_steps(3);
    h.get_by_label("Save your changes?");
    assert!(h.state().tree.people.len() > 1, "tree must not be replaced yet");

    // Cancel keeps the current tree
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(h.state().tree.people.len() > 1);

    // Don't save goes on with the import
    h.state_mut().request_import(path.clone());
    h.run_steps(3);
    h.get_by_label("Don't save").click();
    h.run_steps(3);
    assert_eq!(h.state().tree.people.len(), 1);
    let _ = std::fs::remove_file(path);
}

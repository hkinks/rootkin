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

/// Step the app until background jobs (open, save, import) are done.
fn wait_for_jobs(h: &mut Harness<App>) {
    for _ in 0..500 {
        h.run_steps(2);
        if !h.state().is_busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("background job did not finish");
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
    wait_for_jobs(&mut h);
    assert_eq!(h.state().tree.people.len(), 1);
    let _ = std::fs::remove_file(path);
}

/// Open "Add child" for the selected person (surname prefilled).
fn open_add_child(h: &mut Harness<App>) {
    let app = h.state_mut();
    let anchor = app.selected.clone().unwrap();
    app.relation_dialog =
        Some(crate::relation_dialog::RelationDialog::new(crate::relation_dialog::Relation::Child, &anchor, &app.tree));
    h.run_steps(4);
}

#[test]
fn add_child_dialog_focuses_given_names_and_enter_adds() {
    let mut h = harness();
    let before = h.state().tree.people.len();
    open_add_child(&mut h);
    // the surname is prefilled, yet typing goes straight into "Given names"
    h.get_by_label("Add child of Erik Lindqvist");
    text_input(&h, "Lindqvist");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().tree.people.len(), before, "Enter with only a prefilled surname adds nobody");
    h.event(egui::Event::Text("Anna".into()));
    h.run_steps(3);
    text_input(&h, "Anna");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    assert_eq!(h.state().tree.people.len(), before + 1);
    assert!(h.state().tree.people.iter().any(|p| p.display_name() == "Anna Lindqvist"));
}

#[test]
fn a_second_parent_becomes_the_partner_of_the_first() {
    let mut h = harness();
    let app = h.state_mut();
    let (child, father) = app
        .apply("Setup", |doc| {
            let child = familyx::edit::add_person(doc, "Maria", "Kask", familyx::model::Gender::Female);
            let father = familyx::edit::add_person(doc, "Juri", "Kask", familyx::model::Gender::Male);
            familyx::edit::link_parent(doc, &child, &father)?;
            Ok((child, father))
        })
        .unwrap();
    app.select(&child);
    app.relation_dialog =
        Some(crate::relation_dialog::RelationDialog::new(crate::relation_dialog::Relation::Parent, &child, &app.tree));
    h.run_steps(4);
    h.get_by_label("The new parent and Juri Kask are a couple");
    h.event(egui::Event::Text("Anna".into()));
    h.run_steps(3);
    h.get_by_label("Anna and Juri Kask are a couple");
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let app = h.state();
    let mother = app.tree.parents(&child).into_iter().find(|p| *p != father).expect("second parent").to_string();
    assert_eq!(app.tree.person(&mother).unwrap().display_name(), "Anna");
    assert_eq!(app.tree.spouses(&father), vec![mother.as_str()]);
}

#[test]
fn toasts_stay_clear_of_the_person_panel() {
    let mut h = harness();
    open_add_child(&mut h);
    h.event(egui::Event::Text("Toast".into()));
    h.run_steps(3);
    h.key_press(egui::Key::Enter);
    h.run_steps(4);
    let undo = h.get_all_by_label("Undo").last().unwrap().rect();
    let tree_area = h.get_by_label("Family of Erik Lindqvist").rect();
    let panel = h.get_by_label("Delete this person…").rect();
    assert!(undo.max.x <= panel.min.x, "toast {undo:?} overlaps the person panel {panel:?}");
    assert!(undo.center().x < panel.min.x && tree_area.min.x < undo.min.x);
}

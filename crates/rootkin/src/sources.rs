//! The Sources window (View → Sources…): list, add, edit and delete sources.
//!
//! The form is saved with an explicit button, as one undoable step. Citing a
//! source on a person or event is done in the person panel.

use eframe::egui::{self, Color32, RichText};
use familyx::edit::{self, SourceInput};
use familyx::model::{Id, Source, Tree};

use crate::app::App;

#[derive(Default)]
pub struct SourcesUi {
    pub(crate) open: bool,
    form: Option<SourceForm>,
    confirm_delete: Option<Id>,
}

impl SourcesUi {
    /// Title in the form being edited (tests).
    #[cfg(test)]
    pub(crate) fn editing_title(&self) -> Option<&str> {
        self.form.as_ref().map(|f| f.title.as_str())
    }
}

#[derive(Clone, Default)]
struct SourceForm {
    /// `None` for a source that has not been saved yet.
    id: Option<Id>,
    title: String,
    abbreviation: String,
    author: String,
    publisher: String,
    repository: String,
    call_number: String,
    note: String,
    /// The note as loaded, to leave untouched notes (e.g. formatted ones) alone.
    original_note: String,
}

impl SourceForm {
    fn from_source(s: &Source, tree: &Tree) -> Self {
        let note = s.notes.first().map(|n| n.plain_text()).unwrap_or_default();
        SourceForm {
            id: Some(s.id.clone()),
            title: s.title.clone(),
            abbreviation: s.abbreviation.clone(),
            author: s.author.clone(),
            publisher: s.publisher.clone(),
            repository: s
                .repository
                .as_deref()
                .and_then(|r| tree.repository(r))
                .map(|r| r.title.clone())
                .unwrap_or_default(),
            call_number: s.call_number.clone(),
            original_note: note.clone(),
            note,
        }
    }

    fn input(&self, repository: Option<Id>) -> SourceInput {
        let trim = |s: &str| s.trim().to_string();
        SourceInput {
            title: trim(&self.title),
            abbreviation: trim(&self.abbreviation),
            author: trim(&self.author),
            publisher: trim(&self.publisher),
            note: (self.note.trim() != self.original_note.trim()).then(|| trim(&self.note)),
            repository,
            call_number: trim(&self.call_number),
        }
    }
}

enum Action {
    New,
    Cancel,
    Edit(Id),
    Save,
    Delete(Id),
}

fn field(ui: &mut egui::Ui, label: &str, text: &mut String, hint: &str) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY));
    ui.end_row();
}

impl App {
    pub(crate) fn sources_window(&mut self, ctx: &egui::Context) {
        if !self.sources_ui.open {
            return;
        }
        let mut open = true;
        let mut action = None;
        let tree = &self.tree;
        let state = &mut self.sources_ui;
        egui::Window::new("Sources").open(&mut open).default_width(460.0).default_height(520.0).show(ctx, |ui| {
            if self.pkg.is_none() {
                ui.label("Open or create a family tree first.");
                return;
            }
            if ui.button("+ New source").clicked() {
                action = Some(Action::New);
            }
            ui.add_space(4.0);
            egui::ScrollArea::vertical().id_salt("source_list").max_height(190.0).auto_shrink([false, true]).show(
                ui,
                |ui| {
                    if tree.sources.is_empty() {
                        ui.weak("No sources yet. Add one, then cite it from a person's events.");
                    }
                    for s in &tree.sources {
                        let n = tree.citation_count(&s.id);
                        let label = format!("{}  ({n} citation{})", s.display_title(), if n == 1 { "" } else { "s" });
                        let selected = state.form.as_ref().and_then(|f| f.id.as_deref()) == Some(s.id.as_str());
                        if ui.selectable_label(selected, label).clicked() && !selected {
                            action = Some(Action::Edit(s.id.clone()));
                        }
                    }
                },
            );
            ui.separator();
            let Some(form) = &mut state.form else {
                ui.weak("Select a source to edit it.");
                return;
            };
            ui.heading(if form.id.is_some() { "Edit source" } else { "New source" });
            egui::Grid::new("source_form").num_columns(2).show(ui, |ui| {
                field(ui, "Title", &mut form.title, "e.g. Parish register 1885–1895");
                field(ui, "Author", &mut form.author, "");
                field(ui, "Publication", &mut form.publisher, "publisher, place, year");
                field(ui, "Short title", &mut form.abbreviation, "");
                ui.label("Repository");
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut form.repository)
                            .hint_text("archive or library")
                            .desired_width(ui.available_width() - 40.0),
                    );
                    if !tree.repositories.is_empty() {
                        egui::ComboBox::from_id_salt("source_repository").width(30.0).selected_text("").show_ui(
                            ui,
                            |ui| {
                                for r in &tree.repositories {
                                    if ui.selectable_label(form.repository == r.title, &r.title).clicked() {
                                        form.repository = r.title.clone();
                                    }
                                }
                            },
                        );
                    }
                });
                ui.end_row();
                field(ui, "Call number", &mut form.call_number, "where to find it in the repository");
                ui.label("Notes");
                ui.add(egui::TextEdit::multiline(&mut form.note).desired_rows(3).desired_width(f32::INFINITY));
                ui.end_row();
            });
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let can_save = !form.title.trim().is_empty();
                if ui
                    .add_enabled(can_save, egui::Button::new(RichText::new("Save").strong()))
                    .on_disabled_hover_text("A source needs a title")
                    .clicked()
                {
                    action = Some(Action::Save);
                }
                if ui.button("Cancel").clicked() {
                    action = Some(Action::Cancel);
                }
                if let Some(id) = form.id.clone() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("Delete…").color(Color32::from_rgb(0xff, 0x80, 0x80))).clicked() {
                            action = Some(Action::Delete(id));
                        }
                    });
                }
            });
        });
        if !open {
            self.sources_ui.open = false;
        }
        if let Some(action) = action {
            self.run_source_action(action);
        }
        self.source_delete_dialog(ctx);
    }

    fn run_source_action(&mut self, action: Action) {
        self.flush_form();
        match action {
            Action::New => self.sources_ui.form = Some(SourceForm::default()),
            Action::Cancel => self.sources_ui.form = None,
            Action::Edit(id) => {
                self.sources_ui.form = self.tree.source(&id).map(|s| SourceForm::from_source(s, &self.tree));
            }
            Action::Delete(id) => self.sources_ui.confirm_delete = Some(id),
            Action::Save => {
                let Some(form) = self.sources_ui.form.clone() else { return };
                let label = if form.id.is_some() { "Edit source" } else { "Add source" };
                let saved = self.apply(label, |d| {
                    let repository = match form.repository.trim() {
                        "" => None,
                        name => Some(edit::find_or_add_repository(d, name)?),
                    };
                    let input = form.input(repository);
                    match &form.id {
                        Some(id) => edit::update_source(d, id, &input).map(|()| id.clone()),
                        None => edit::add_source(d, &input),
                    }
                });
                if let Some(id) = saved {
                    // the person panel shows source titles
                    self.form = None;
                    self.toasts.info(format!("Saved source “{}”", form.title.trim()));
                    self.sources_ui.form = self.tree.source(&id).map(|s| SourceForm::from_source(s, &self.tree));
                }
            }
        }
    }

    fn source_delete_dialog(&mut self, ctx: &egui::Context) {
        let Some(id) = self.sources_ui.confirm_delete.clone() else { return };
        let Some(source) = self.tree.source(&id).cloned() else {
            self.sources_ui.confirm_delete = None;
            return;
        };
        let n = self.tree.citation_count(&id);
        let (mut keep, mut delete) = (false, false);
        egui::Modal::new(egui::Id::new("confirm_delete_source")).show(ctx, |ui| {
            ui.heading(format!("Delete “{}”?", source.display_title()));
            if n > 0 {
                ui.label(format!(
                    "This also removes its {n} citation{} from people and events. You can undo it with Ctrl+Z.",
                    if n == 1 { "" } else { "s" }
                ));
            } else {
                ui.label("Nothing cites this source. You can undo it with Ctrl+Z.");
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                keep = ui.button(RichText::new("Keep").strong()).clicked();
                delete = ui.button(RichText::new("Delete").color(Color32::from_rgb(0xff, 0x80, 0x80))).clicked();
            });
        });
        if delete && self.apply("Delete source", |d| edit::delete_source(d, &id)).is_some() {
            self.form = None;
            self.sources_ui.form = None;
            self.toasts.undoable(format!("Deleted source “{}”", source.display_title()));
        }
        if delete || keep {
            self.sources_ui.confirm_delete = None;
        }
    }
}

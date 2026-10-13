use std::path::PathBuf;

use eframe::egui::{self, RichText};
use familyx::doc::Document;
use familyx::edit::EditResult;
use familyx::model::{Id, Person, Tree};
use familyx::Package;

use crate::person_panel::PersonForm;
use crate::photos::PhotoCache;
use crate::relation_dialog::RelationDialog;
use crate::tree_view::{TreeAction, TreeView};

const UNDO_LIMIT: usize = 100;

pub struct App {
    pub(crate) path: Option<PathBuf>,
    pub(crate) pkg: Option<Package>,
    pub(crate) tree: Tree,
    pub(crate) selected: Option<Id>,
    pub(crate) focus: Option<Id>,
    search: String,
    pub(crate) photos: PhotoCache,
    tree_view: TreeView,
    pub(crate) form: Option<PersonForm>,
    pub(crate) relation_dialog: Option<RelationDialog>,
    pub(crate) confirm_delete: Option<Id>,
    undo: Vec<Document>,
    redo: Vec<Document>,
    dirty: bool,
    confirm_close: bool,
    pub(crate) status: Option<String>,
}

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut app = App {
            path: None,
            pkg: None,
            tree: Tree::default(),
            selected: None,
            focus: None,
            search: String::new(),
            photos: PhotoCache::default(),
            tree_view: TreeView::default(),
            form: None,
            relation_dialog: None,
            confirm_delete: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
            confirm_close: false,
            status: None,
        };
        if let Some(path) = path {
            app.open(path);
        }
        app
    }

    fn open(&mut self, path: PathBuf) {
        match Package::open(&path) {
            Ok(pkg) => {
                self.tree = Tree::from_doc(&pkg.doc);
                let start = self.tree.home_person.clone().or_else(|| self.tree.people.first().map(|p| p.id.clone()));
                self.focus = start.clone();
                self.selected = start;
                self.pkg = Some(pkg);
                self.photos.clear();
                self.undo.clear();
                self.redo.clear();
                self.form = None;
                self.dirty = false;
                self.status = Some(format!("Opened {} ({} people)", path.display(), self.tree.people.len()));
                self.path = Some(path);
            }
            Err(e) => self.status = Some(format!("Could not open {}: {e}", path.display())),
        }
    }

    fn save(&mut self, path: PathBuf) {
        let Some(pkg) = &self.pkg else { return };
        match pkg.save(&path) {
            Ok(()) => {
                self.dirty = false;
                self.status = Some(format!("Saved {} (previous version kept as .bak)", path.display()));
                self.path = Some(path);
            }
            Err(e) => self.status = Some(format!("Save failed: {e}")),
        }
    }

    fn pick_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]).pick_file() {
            self.open(path);
        }
    }

    fn pick_save_as(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]);
        if let Some(name) = self.path.as_ref().and_then(|p| p.file_name()) {
            dialog = dialog.set_file_name(name.to_string_lossy());
        }
        if let Some(path) = dialog.save_file() {
            self.save(path);
        }
    }

    /// Run an edit against the document with an undo snapshot. On failure
    /// the document is restored and the error shown.
    pub(crate) fn apply<T>(&mut self, edit: impl FnOnce(&mut Document) -> EditResult<T>) -> Option<T> {
        let pkg = self.pkg.as_mut()?;
        let snapshot = pkg.doc.clone();
        match edit(&mut pkg.doc) {
            Ok(value) => {
                self.undo.push(snapshot);
                if self.undo.len() > UNDO_LIMIT {
                    self.undo.remove(0);
                }
                self.redo.clear();
                self.after_change();
                Some(value)
            }
            Err(e) => {
                pkg.doc = snapshot;
                self.status = Some(format!("Edit failed: {e}"));
                None
            }
        }
    }

    fn undo(&mut self, redo: bool) {
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let (Some(pkg), Some(doc)) = (self.pkg.as_mut(), from.pop()) else { return };
        to.push(std::mem::replace(&mut pkg.doc, doc));
        self.form = None;
        self.after_change();
    }

    fn after_change(&mut self) {
        let Some(pkg) = &self.pkg else { return };
        self.tree = Tree::from_doc(&pkg.doc);
        self.form = None;
        self.dirty = true;
        for slot in [&mut self.selected, &mut self.focus] {
            if slot.as_deref().is_some_and(|id| self.tree.person(id).is_none()) {
                *slot = None;
            }
        }
        if self.focus.is_none() {
            self.focus = self.selected.clone().or_else(|| self.tree.people.first().map(|p| p.id.clone()));
        }
    }

    pub(crate) fn select(&mut self, id: &str) {
        self.selected = Some(id.to_string());
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let undo = KeyboardShortcut::new(Modifiers::COMMAND, Key::Z);
        let redo = KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z);
        let redo_y = KeyboardShortcut::new(Modifiers::COMMAND, Key::Y);
        let open = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
        let save = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
        // redo before undo: Ctrl+Shift+Z would otherwise match Ctrl+Z
        let (redo, undo, open, save) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&redo) || i.consume_shortcut(&redo_y),
                i.consume_shortcut(&undo),
                i.consume_shortcut(&open),
                i.consume_shortcut(&save),
            )
        });
        let editing_text = ctx.memory(|m| m.focused().is_some());
        if redo && !editing_text {
            self.undo(true);
        }
        if undo && !editing_text {
            self.undo(false);
        }
        if open {
            self.pick_open();
        }
        if save {
            match self.path.clone() {
                Some(path) => self.save(path),
                None => self.pick_save_as(),
            }
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Open…").on_hover_text("Ctrl+O").clicked() {
                self.pick_open();
            }
            let has_file = self.pkg.is_some();
            if ui.add_enabled(has_file && self.path.is_some(), egui::Button::new("Save")).on_hover_text("Ctrl+S").clicked() {
                if let Some(path) = self.path.clone() {
                    self.save(path);
                }
            }
            if ui.add_enabled(has_file, egui::Button::new("Save as…")).clicked() {
                self.pick_save_as();
            }
            ui.separator();
            if ui.add_enabled(!self.undo.is_empty(), egui::Button::new("⟲ Undo")).on_hover_text("Ctrl+Z").clicked() {
                self.undo(false);
            }
            if ui.add_enabled(!self.redo.is_empty(), egui::Button::new("⟳ Redo")).on_hover_text("Ctrl+Shift+Z").clicked() {
                self.undo(true);
            }
            ui.separator();
            if ui.add_enabled(has_file, egui::Button::new("+ New person")).clicked() {
                self.relation_dialog = Some(RelationDialog::new_unrelated());
            }
            ui.separator();
            if let Some(path) = &self.path {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                ui.label(if self.dirty { format!("{name} •") } else { name.into_owned() });
            }
            if let Some(status) = &self.status {
                ui.separator();
                ui.weak(status);
            }
        });
    }

    fn people_list(&mut self, ui: &mut egui::Ui) {
        ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search people…").desired_width(f32::INFINITY));
        ui.separator();
        let needle = self.search.to_lowercase();
        let mut people: Vec<&Person> = self
            .tree
            .people
            .iter()
            .filter(|p| needle.is_empty() || p.names.iter().any(|n| n.display().to_lowercase().contains(&needle)))
            .collect();
        people.sort_by_cached_key(|p| sort_key(p));

        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for p in people {
                let is_selected = self.selected.as_deref() == Some(p.id.as_str());
                let label = format!("{}  {}", p.display_name(), p.lifespan());
                let r = ui.selectable_label(is_selected, label);
                if r.clicked() {
                    clicked = Some((p.id.clone(), false));
                }
                if r.double_clicked() {
                    clicked = Some((p.id.clone(), true));
                }
            }
        });
        if let Some((id, focus)) = clicked {
            if focus {
                self.focus = Some(id.clone());
            }
            self.select(&id);
        }
    }

    fn close_guard(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty && !self.confirm_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm_close = true;
        }
        if !self.confirm_close {
            return;
        }
        let mut choice = None;
        egui::Modal::new(egui::Id::new("confirm_close")).show(ctx, |ui| {
            ui.heading("Unsaved changes");
            ui.label("Save changes before closing?");
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(0);
                }
                if ui.button("Discard").clicked() {
                    choice = Some(1);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(2);
                }
            });
        });
        match choice {
            Some(0) => {
                match self.path.clone() {
                    Some(path) => self.save(path),
                    None => self.pick_save_as(),
                }
                if !self.dirty {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                self.confirm_close = false;
            }
            Some(1) => {
                self.dirty = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Some(_) => self.confirm_close = false,
            None => {}
        }
    }
}

pub(crate) fn sort_key(p: &Person) -> (String, String) {
    let n = p.primary_name();
    (n.map(|n| n.surname.to_lowercase()).unwrap_or_default(), n.map(|n| n.given.to_lowercase()).unwrap_or_default())
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        self.close_guard(&ctx);
        egui::Panel::top("top").show(ui, |ui| self.top_bar(ui));

        if self.pkg.is_none() {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.centered_and_justified(|ui| {
                    if ui.button(RichText::new("Open a .familyx file…").size(18.0)).clicked() {
                        self.pick_open();
                    }
                });
            });
            return;
        }

        egui::Panel::left("people").default_size(260.0).show(ui, |ui| self.people_list(ui));
        egui::Panel::right("person").default_size(380.0).show(ui, |ui| self.person_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            let Some(focus) = self.focus.clone() else { return };
            match self.tree_view.ui(ui, &self.tree, &focus, self.selected.as_deref()) {
                Some(TreeAction::Select(id)) => self.select(&id),
                Some(TreeAction::Focus(id)) => {
                    self.select(&id);
                    self.focus = Some(id);
                }
                None => {}
            }
        });
        self.relation_dialog_ui(&ctx);
        self.delete_dialog_ui(&ctx);
    }
}

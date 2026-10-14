use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, Color32, RichText};
use familyx::doc::Document;
use familyx::edit::{self, EditResult};
use familyx::model::{Id, Person, Tree};
use familyx::Package;

use crate::person_panel::PersonForm;
use crate::photos::PhotoCache;
use crate::relation_dialog::RelationDialog;
use crate::toasts::Toasts;
use crate::tree_view::{NavState, TreeAction, TreeView};

const UNDO_LIMIT: usize = 100;

/// A finished background job.
enum Job {
    Opened(PathBuf, Result<Package, String>),
    /// A converted genealogy file and a summary of what came in.
    Imported(PathBuf, Result<(Package, String), String>),
    Saved(PathBuf, u64, Result<(), String>),
}

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Surname,
    GivenName,
    Birth,
}

pub struct App {
    ctx: egui::Context,
    pub(crate) path: Option<PathBuf>,
    pub(crate) pkg: Option<Package>,
    pub(crate) tree: Tree,
    /// The selected person; the tree is always centred on them.
    pub(crate) selected: Option<Id>,
    history: Vec<Id>,
    future: Vec<Id>,
    search: String,
    sort_by: SortBy,
    living_only: bool,
    pub(crate) photos: PhotoCache,
    tree_view: TreeView,
    pub(crate) form: Option<PersonForm>,
    pub(crate) relation_dialog: Option<RelationDialog>,
    pub(crate) confirm_delete: Option<Id>,
    pub(crate) confirm_unlink: Option<crate::person_panel::Unlink>,
    undo: Vec<(String, Document)>,
    redo: Vec<(String, Document)>,
    /// Bumped on every change; a save records which version it wrote.
    version: u64,
    saved_version: u64,
    confirm_close: bool,
    /// Something to do once unsaved changes are dealt with.
    pending: Option<Pending>,
    busy: Option<String>,
    jobs: (mpsc::Sender<Job>, mpsc::Receiver<Job>),
    recent: Vec<PathBuf>,
    pub(crate) toasts: Toasts,
    error_dialog: Option<String>,
    /// An informational message shown in a dialog (e.g. after converting an old file).
    notice: Option<String>,
    show_about: bool,
    pub(crate) sources_ui: crate::sources::SourcesUi,
}

#[derive(Clone)]
enum Pending {
    Close,
    Open(PathBuf),
    Import(PathBuf),
    New,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        let mut app = App {
            ctx: cc.egui_ctx.clone(),
            path: None,
            pkg: None,
            tree: Tree::default(),
            selected: None,
            history: Vec::new(),
            future: Vec::new(),
            search: String::new(),
            sort_by: SortBy::Surname,
            living_only: false,
            photos: PhotoCache::default(),
            tree_view: TreeView::default(),
            form: None,
            relation_dialog: None,
            confirm_delete: None,
            confirm_unlink: None,
            undo: Vec::new(),
            redo: Vec::new(),
            version: 0,
            saved_version: 0,
            confirm_close: false,
            pending: None,
            busy: None,
            jobs: mpsc::channel(),
            recent: crate::recent::load(),
            toasts: Toasts::default(),
            error_dialog: None,
            notice: None,
            show_about: false,
            sources_ui: crate::sources::SourcesUi::default(),
        };
        if let Some(path) = path {
            app.start_open(path);
        }
        app
    }

    #[cfg(test)]
    pub(crate) fn is_busy(&self) -> bool {
        self.busy.is_some()
    }

    fn dirty(&self) -> bool {
        self.pkg.is_some() && self.version != self.saved_version
    }

    // ------------------------------------------------------------ files

    /// Opening and saving derive keys with 210k PBKDF2 rounds; run them off
    /// the UI thread.
    fn start_open(&mut self, path: PathBuf) {
        let tx = self.jobs.0.clone();
        self.busy = Some(format!("Opening {}…", file_name(&path)));
        std::thread::spawn(move || {
            let result = Package::open(&path).map_err(|e| e.to_string());
            let _ = tx.send(Job::Opened(path, result));
        });
    }

    fn start_save(&mut self, path: PathBuf) {
        self.flush_form();
        let Some(pkg) = &mut self.pkg else { return };
        edit::touch_file(&mut pkg.doc, self.selected.as_deref());
        let pkg = pkg.clone();
        let version = self.version;
        let tx = self.jobs.0.clone();
        self.busy = Some(format!("Saving {}…", file_name(&path)));
        std::thread::spawn(move || {
            let result = pkg.save(&path).map_err(|e| e.to_string());
            let _ = tx.send(Job::Saved(path, version, result));
        });
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        while let Ok(job) = self.jobs.1.try_recv() {
            self.busy = None;
            match job {
                Job::Opened(path, Ok(pkg)) => {
                    self.notice = migration_notice(&pkg);
                    self.load(pkg, Some(path));
                }
                Job::Opened(path, Err(e)) => {
                    self.error_dialog = Some(format!("Could not open {}:\n\n{e}", path.display()));
                }
                Job::Imported(_, Ok((pkg, report))) => {
                    self.load(pkg, None);
                    self.saved_version = u64::MAX;
                    self.toasts.info(format!("Imported {report}. Save it as a .familyx file to keep it."));
                }
                Job::Imported(path, Err(e)) => {
                    self.error_dialog = Some(format!("Could not import {}:\n\n{e}", path.display()));
                }
                Job::Saved(path, version, Ok(())) => {
                    self.saved_version = version;
                    crate::recent::remember(&mut self.recent, &path);
                    self.toasts.info(format!("Saved {}. The previous version is kept as .bak", file_name(&path)));
                    self.path = Some(path);
                    self.after_save();
                }
                Job::Saved(path, _, Err(e)) => {
                    self.pending = None;
                    self.error_dialog =
                        Some(format!("Could not save {}:\n\n{e}\n\nYour changes are still open here.", path.display()));
                }
            }
        }
    }

    /// Continue whatever was waiting for the save.
    fn after_save(&mut self) {
        match self.pending.take() {
            Some(Pending::Close) => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Pending::Open(path)) => self.start_open(path),
            Some(Pending::Import(path)) => self.start_import(path),
            Some(Pending::New) => self.new_tree(),
            None => {}
        }
    }

    pub(crate) fn load(&mut self, pkg: Package, path: Option<PathBuf>) {
        self.tree = Tree::from_doc(&pkg.doc);
        self.selected = self.tree.home_person.clone().or_else(|| self.tree.people.first().map(|p| p.id.clone()));
        self.history.clear();
        self.future.clear();
        self.pkg = Some(pkg);
        self.photos.clear();
        self.undo.clear();
        self.redo.clear();
        self.form = None;
        self.version = 0;
        self.saved_version = if path.is_some() { 0 } else { u64::MAX };
        self.tree_view.fit_now();
        if let Some(path) = &path {
            crate::recent::remember(&mut self.recent, path);
            self.toasts.info(format!("Opened {} ({} people)", file_name(path), self.tree.people.len()));
        }
        self.path = path;
    }

    fn new_tree(&mut self) {
        self.load(Package::new(), None);
        self.saved_version = 0;
        self.relation_dialog = Some(RelationDialog::new_unrelated());
    }

    /// Run `then` now, or after asking to save unsaved changes.
    fn guarded(&mut self, then: Pending) {
        self.flush_form();
        self.pending = Some(then);
        if self.dirty() {
            self.confirm_close = true;
        } else {
            self.after_save();
        }
    }

    fn pick_open(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]).pick_file() {
            self.guarded(Pending::Open(path));
        }
    }

    fn save(&mut self) {
        match self.path.clone() {
            Some(path) => self.start_save(path),
            None => self.pick_save_as(),
        }
    }

    fn pick_save_as(&mut self) {
        let mut dialog = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]);
        dialog = dialog.set_file_name(
            self.path.as_ref().and_then(|p| p.file_name()).map_or("family.familyx".into(), |n| n.to_string_lossy()),
        );
        match dialog.save_file() {
            Some(mut path) => {
                if path.extension().is_none() {
                    path.set_extension("familyx");
                }
                self.start_save(path);
            }
            None => self.pending = None,
        }
    }

    fn export_svg(&mut self) {
        let Some(focus) = self.selected.clone() else { return };
        let Some(path) =
            rfd::FileDialog::new().add_filter("SVG image", &["svg"]).set_file_name("family-tree.svg").save_file()
        else {
            return;
        };
        let svg = crate::export::tree_svg(&self.tree, &focus, self.tree_view.settings);
        match std::fs::write(&path, svg) {
            Ok(()) => self.toasts.info(format!("Exported {}", file_name(&path))),
            Err(e) => self.toasts.error(format!("Export failed: {e}")),
        }
    }

    fn export_png(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
    }

    /// Crop the canvas out of a requested screenshot and save it as PNG.
    fn handle_screenshot(&mut self, ctx: &egui::Context) {
        let shot = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(image) = shot else { return };
        let scale = ctx.pixels_per_point();
        let r = self.tree_view.canvas;
        let (x0, y0) = ((r.min.x * scale) as u32, (r.min.y * scale) as u32);
        let (w, h) = ((r.width() * scale) as u32, (r.height() * scale) as u32);
        let [iw, ih] = image.size;
        let mut out = image::RgbaImage::new(w.min(iw as u32 - x0), h.min(ih as u32 - y0));
        for (x, y, px) in out.enumerate_pixels_mut() {
            let c = image.pixels[(y0 + y) as usize * iw + (x0 + x) as usize];
            *px = image::Rgba([c.r(), c.g(), c.b(), 255]);
        }
        let Some(path) =
            rfd::FileDialog::new().add_filter("PNG image", &["png"]).set_file_name("family-tree.png").save_file()
        else {
            return;
        };
        match out.save(&path) {
            Ok(()) => self.toasts.info(format!("Exported {}", file_name(&path))),
            Err(e) => self.toasts.error(format!("Export failed: {e}")),
        }
    }

    fn save_report(&mut self, html: String, name: &str) {
        let Some(path) = rfd::FileDialog::new().add_filter("Web page", &["html"]).set_file_name(name).save_file()
        else {
            return;
        };
        match std::fs::write(&path, html) {
            Ok(()) => {
                self.toasts.info("Report saved and opened in your browser. Print it from there.");
                let _ = open::that_detached(&path);
            }
            Err(e) => self.toasts.error(format!("Could not save the report: {e}")),
        }
    }

    // ------------------------------------------------------------ editing

    /// Run an edit with an undo snapshot labelled `label`. On failure the
    /// document is restored and the error shown.
    pub(crate) fn apply<T>(&mut self, label: &str, edit: impl FnOnce(&mut Document) -> EditResult<T>) -> Option<T> {
        let pkg = self.pkg.as_mut()?;
        let snapshot = pkg.doc.clone();
        match edit(&mut pkg.doc) {
            Ok(value) => {
                self.push_undo(label, snapshot);
                self.after_change();
                Some(value)
            }
            Err(e) => {
                pkg.doc = snapshot;
                self.toasts.error(format!("{label} failed: {e}"));
                None
            }
        }
    }

    /// Like [`App::apply`] for edits that need the whole package (photos).
    pub(crate) fn apply_pkg<T>(
        &mut self,
        label: &str,
        edit: impl FnOnce(&mut Package) -> familyx::Result<T>,
    ) -> Option<T> {
        let pkg = self.pkg.as_mut()?;
        // photo parts aren't snapshotted; undo restores references only
        let snapshot = pkg.doc.clone();
        match edit(pkg) {
            Ok(value) => {
                self.push_undo(label, snapshot);
                self.photos.clear();
                self.after_change();
                Some(value)
            }
            Err(e) => {
                pkg.doc = snapshot;
                self.toasts.error(format!("{label} failed: {e}"));
                None
            }
        }
    }

    fn push_undo(&mut self, label: &str, snapshot: Document) {
        self.undo.push((label.to_string(), snapshot));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn undo(&mut self, redo: bool) {
        self.flush_form();
        let (from, to) = if redo { (&mut self.redo, &mut self.undo) } else { (&mut self.undo, &mut self.redo) };
        let (Some(pkg), Some((label, doc))) = (self.pkg.as_mut(), from.pop()) else { return };
        to.push((label.clone(), std::mem::replace(&mut pkg.doc, doc)));
        self.form = None;
        self.after_change();
        self.toasts.info(format!("{}: {label}", if redo { "Redone" } else { "Undone" }));
    }

    fn after_change(&mut self) {
        let Some(pkg) = &self.pkg else { return };
        self.tree = Tree::from_doc(&pkg.doc);
        self.version += 1;
        if self.selected.as_deref().is_some_and(|id| self.tree.person(id).is_none()) {
            self.selected = self.history.pop().filter(|id| self.tree.person(id).is_some());
        }
        if self.selected.is_none() {
            self.selected = self.tree.home_person.clone().or_else(|| self.tree.people.first().map(|p| p.id.clone()));
        }
        // a form for a person that still exists stays, so unsaved typing elsewhere survives
        if self.form.as_ref().is_some_and(|f| Some(f.person_id()) != self.selected.as_deref()) {
            self.form = None;
        }
    }

    /// Commit text the user is still typing (before saving, undo, navigation).
    pub(crate) fn flush_form(&mut self) {
        let Some(form) = self.form.take() else { return };
        let person = form.person_id().to_string();
        let edits = form.pending_edits(&self.tree);
        self.form = Some(form);
        for e in edits {
            self.apply_form_edit(&person, e);
        }
    }

    /// Select a person and centre the tree on them, keeping history.
    pub(crate) fn select(&mut self, id: &str) {
        if self.selected.as_deref() == Some(id) {
            return;
        }
        self.flush_form();
        if let Some(prev) = self.selected.replace(id.to_string()) {
            self.history.push(prev);
        }
        self.future.clear();
        self.form = None;
    }

    fn go_back(&mut self, forward: bool) {
        self.flush_form();
        let (from, to) =
            if forward { (&mut self.future, &mut self.history) } else { (&mut self.history, &mut self.future) };
        while let Some(id) = from.pop() {
            if self.tree.person(&id).is_some() {
                if let Some(cur) = self.selected.replace(id) {
                    to.push(cur);
                }
                self.form = None;
                return;
            }
        }
    }

    // ------------------------------------------------------------ input

    fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let sc = |m, k| KeyboardShortcut::new(m, k);
        let editing_text = ctx.egui_wants_keyboard_input();
        let (redo, undo, open, save, save_as, new, back, fwd) = ctx.input_mut(|i| {
            (
                // redo first: Ctrl+Shift+Z would otherwise match Ctrl+Z
                i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::Z))
                    || i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Y)),
                !editing_text && i.consume_shortcut(&sc(Modifiers::COMMAND, Key::Z)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::O)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::S)),
                i.consume_shortcut(&sc(Modifiers::COMMAND | Modifiers::SHIFT, Key::S)),
                i.consume_shortcut(&sc(Modifiers::COMMAND, Key::N)),
                i.consume_shortcut(&sc(Modifiers::ALT, Key::ArrowLeft)),
                i.consume_shortcut(&sc(Modifiers::ALT, Key::ArrowRight)),
            )
        });
        if redo && !editing_text {
            self.undo(true);
        }
        if undo {
            self.undo(false);
        }
        if open {
            self.pick_open();
        }
        if save_as && self.pkg.is_some() {
            self.flush_form();
            self.pick_save_as();
        } else if save && self.pkg.is_some() {
            self.save();
        }
        if new {
            self.guarded(Pending::New);
        }
        if back {
            self.go_back(false);
        }
        if fwd {
            self.go_back(true);
        }
        if !editing_text && self.pkg.is_some() && self.relation_dialog.is_none() {
            self.arrow_navigation(ctx);
        }
    }

    /// ↑ parent, ↓ child, ← → previous/next sibling (or spouse).
    fn arrow_navigation(&mut self, ctx: &egui::Context) {
        let Some(cur) = self.selected.clone() else { return };
        let key = ctx.input(|i| {
            [egui::Key::ArrowUp, egui::Key::ArrowDown, egui::Key::ArrowLeft, egui::Key::ArrowRight]
                .into_iter()
                .find(|k| i.key_pressed(*k) && i.modifiers.is_none())
        });
        let Some(key) = key else { return };
        let t = &self.tree;
        let target = match key {
            egui::Key::ArrowUp => crate::layout::parent_pair(t, &cur).0.into_iter().next(),
            egui::Key::ArrowDown => t.children(&cur).first().map(|s| s.to_string()),
            _ => {
                let (pair, _) = crate::layout::parent_pair(t, &cur);
                let mut row: Vec<String> =
                    pair.first().map(|p| t.children(p)).unwrap_or_default().iter().map(|s| s.to_string()).collect();
                if row.is_empty() {
                    row.push(cur.clone());
                }
                row.extend(t.spouses(&cur).iter().map(|s| s.to_string()));
                let i = row.iter().position(|s| *s == cur).unwrap_or(0) as i32;
                let j = if key == egui::Key::ArrowLeft { i - 1 } else { i + 1 };
                row.get(j.max(0) as usize).filter(|_| j >= 0).cloned()
            }
        };
        if let Some(id) = target {
            self.select(&id);
        }
    }

    // ------------------------------------------------------------ panels

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.add(egui::Button::new("New family tree").shortcut_text("Ctrl+N")).clicked() {
                    self.guarded(Pending::New);
                }
                if ui.add(egui::Button::new("Open…").shortcut_text("Ctrl+O")).clicked() {
                    self.pick_open();
                }
                ui.menu_button("Open recent", |ui| {
                    if self.recent.is_empty() {
                        ui.weak("No recent files");
                    }
                    for path in self.recent.clone() {
                        if ui.button(path.display().to_string()).clicked() {
                            self.guarded(Pending::Open(path));
                        }
                    }
                });
                ui.separator();
                let has = self.pkg.is_some();
                if ui.add_enabled(has, egui::Button::new("Save").shortcut_text("Ctrl+S")).clicked() {
                    self.save();
                }
                if ui.add_enabled(has, egui::Button::new("Save as…").shortcut_text("Ctrl+Shift+S")).clicked() {
                    self.flush_form();
                    self.pick_save_as();
                }
                if ui
                    .add_enabled(has, egui::Button::new("Save a copy for My Family Tree 12…"))
                    .on_hover_text("For older versions of the Windows app (12.x) that can't open current files")
                    .clicked()
                {
                    self.save_copy_for_app12();
                }
                ui.separator();
                if ui.button("Import GEDCOM / Gramps…").clicked() {
                    self.import(&ctx);
                }
                ui.add_enabled_ui(has, |ui| {
                    ui.menu_button("Export", |ui| {
                        use crate::formats::Format;
                        if ui
                            .button("GEDCOM 5.5.1 (.ged)…")
                            .on_hover_text("Readable by almost every genealogy program")
                            .clicked()
                        {
                            self.export_format("ged", Format::Gedcom);
                        }
                        if ui.button("GEDCOM 7 (.ged)…").clicked() {
                            self.export_format("ged", Format::Gedcom7);
                        }
                        if ui.button("Gramps (.gramps)…").clicked() {
                            self.export_format("gramps", Format::Gramps);
                        }
                        ui.separator();
                        if ui.button("Tree as image (SVG)…").clicked() {
                            self.export_svg();
                        }
                        if ui.button("Tree as picture (PNG)…").clicked() {
                            self.export_png(&ctx);
                        }
                    });
                    ui.menu_button("Print / report", |ui| {
                        if ui.button("Family of the selected person…").clicked() {
                            if let Some(id) = self.selected.clone() {
                                let name = self.tree.person(&id).map(|p| p.display_name()).unwrap_or_default();
                                self.save_report(
                                    crate::export::person_report(&self.tree, &id),
                                    &format!("{name}.html"),
                                );
                            }
                        }
                        if ui.button("Everyone in the tree…").clicked() {
                            self.save_report(crate::export::full_report(&self.tree), "family-tree.html");
                        }
                    });
                });
                ui.separator();
                if ui.button("Quit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let undo_label = self.undo.last().map_or("Undo".into(), |(l, _)| format!("Undo: {l}"));
                if ui
                    .add_enabled(!self.undo.is_empty(), egui::Button::new(undo_label).shortcut_text("Ctrl+Z"))
                    .clicked()
                {
                    self.undo(false);
                }
                let redo_label = self.redo.last().map_or("Redo".into(), |(l, _)| format!("Redo: {l}"));
                if ui
                    .add_enabled(!self.redo.is_empty(), egui::Button::new(redo_label).shortcut_text("Ctrl+Shift+Z"))
                    .clicked()
                {
                    self.undo(true);
                }
                ui.separator();
                if ui.add_enabled(self.pkg.is_some(), egui::Button::new("Add a person (not linked yet)…")).clicked() {
                    self.relation_dialog = Some(RelationDialog::new_unrelated());
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Fit tree to window").clicked() {
                    self.tree_view.fit_now();
                }
                if ui.add_enabled(self.pkg.is_some(), egui::Button::new("Sources…")).clicked() {
                    self.sources_ui.open = true;
                }
                ui.separator();
                egui::widgets::global_theme_preference_buttons(ui);
                ui.label("Text size: Ctrl + / Ctrl −");
            });
            ui.menu_button("Help", |ui| {
                if ui.button("About Rootkin").clicked() {
                    self.show_about = true;
                }
            });

            ui.separator();
            if let Some(path) = &self.path {
                ui.label(file_name(path));
            } else if self.pkg.is_some() {
                ui.label("New family tree");
            }
            if self.pkg.is_some() {
                if self.dirty() {
                    ui.label(RichText::new("● Unsaved changes").color(Color32::from_rgb(0xe0, 0xa0, 0x30)))
                        .on_hover_text("Press Ctrl+S to save");
                } else {
                    ui.label(RichText::new("✔ All changes saved").color(Color32::from_rgb(0x60, 0xb0, 0x60)));
                }
            }
            if let Some(busy) = &self.busy {
                ui.spinner();
                ui.label(busy);
            }
        });
    }

    fn welcome(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.15);
            ui.heading(RichText::new("Rootkin").size(34.0));
            ui.label("Your family tree on your own computer. No cloud, no account.");
            ui.add_space(24.0);
            let big = |t: &str| egui::Button::new(RichText::new(t).size(18.0)).min_size(egui::vec2(280.0, 44.0));
            if ui.add(big("Start a new family tree")).clicked() {
                self.new_tree();
            }
            if ui.add(big("Open a family tree (.familyx)…")).clicked() {
                self.pick_open();
            }
            if ui.add(big("Import GEDCOM or Gramps…")).clicked() {
                self.import(&ctx);
            }
            if !self.recent.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new("Recent").strong());
                for path in self.recent.clone() {
                    if ui.link(path.display().to_string()).clicked() {
                        self.start_open(path);
                    }
                }
            }
        });
    }

    fn empty_tree(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.heading("This family tree is empty");
            ui.add_space(12.0);
            if ui
                .add(
                    egui::Button::new(RichText::new("Add the first person").size(18.0))
                        .min_size(egui::vec2(240.0, 44.0)),
                )
                .clicked()
            {
                self.relation_dialog = Some(RelationDialog::new_unrelated());
            }
        });
    }

    fn people_list(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search names, places, years…")
                    .desired_width(ui.available_width() - 28.0),
            );
            if !self.search.is_empty() && ui.small_button("✖").on_hover_text("Clear search").clicked() {
                self.search.clear();
            }
        });
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("sort")
                .selected_text(match self.sort_by {
                    SortBy::Surname => "Sort: surname",
                    SortBy::GivenName => "Sort: first name",
                    SortBy::Birth => "Sort: birth year",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort_by, SortBy::Surname, "Surname");
                    ui.selectable_value(&mut self.sort_by, SortBy::GivenName, "First name");
                    ui.selectable_value(&mut self.sort_by, SortBy::Birth, "Birth year");
                });
            ui.checkbox(&mut self.living_only, "Living only");
        });

        let needle = self.search.to_lowercase();
        let matches = |p: &Person| {
            needle.is_empty()
                || p.names.iter().any(|n| n.display().to_lowercase().contains(&needle))
                || p.facts
                    .iter()
                    .any(|f| f.place.to_lowercase().contains(&needle) || f.date.display().contains(&needle))
        };
        let mut people: Vec<&Person> =
            self.tree.people.iter().filter(|p| (!self.living_only || p.is_living) && matches(p)).collect();
        match self.sort_by {
            SortBy::Surname => people.sort_by_cached_key(|p| sort_key(p)),
            SortBy::GivenName => people.sort_by_cached_key(|p| {
                let (unnamed, s, g) = sort_key(p);
                (unnamed, g, s)
            }),
            SortBy::Birth => people
                .sort_by_cached_key(|p| (p.fact("BIRT").and_then(|f| f.date.year).unwrap_or(i32::MAX), sort_key(p))),
        }
        ui.weak(format!("{} of {} people", people.len(), self.tree.people.len()));
        ui.separator();

        let mut clicked = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for p in people {
                let is_selected = self.selected.as_deref() == Some(p.id.as_str());
                let label = format!("{}  {}", p.display_name(), p.lifespan());
                if ui.selectable_label(is_selected, label).clicked() {
                    clicked = Some(p.id.clone());
                }
            }
        });
        if let Some(id) = clicked {
            self.select(&id);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        // unsaved changes before close/open/new
        if ctx.input(|i| i.viewport().close_requested()) {
            self.flush_form();
            if self.dirty() && !self.confirm_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.pending = Some(Pending::Close);
                self.confirm_close = true;
            }
        }
        if self.confirm_close {
            let mut choice = None;
            egui::Modal::new(egui::Id::new("confirm_close")).show(ctx, |ui| {
                ui.heading("Save your changes?");
                ui.label("Your changes will be lost if you don't save them.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(2);
                    }
                    if ui.button("Don't save").clicked() {
                        choice = Some(1);
                    }
                    if ui.button(RichText::new("Save").strong()).clicked() {
                        choice = Some(0);
                    }
                });
            });
            match choice {
                Some(0) => {
                    self.confirm_close = false;
                    self.save();
                }
                Some(1) => {
                    self.confirm_close = false;
                    self.saved_version = self.version;
                    self.after_save();
                }
                Some(_) => {
                    self.confirm_close = false;
                    self.pending = None;
                }
                None => {}
            }
        }

        if let Some(msg) = self.error_dialog.clone() {
            egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.heading(RichText::new("Something went wrong").color(Color32::LIGHT_RED));
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.error_dialog = None;
                }
            });
        }

        if let Some(msg) = self.notice.clone() {
            egui::Modal::new(egui::Id::new("notice")).show(ctx, |ui| {
                ui.set_max_width(480.0);
                ui.heading("Older file converted");
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.notice = None;
                }
            });
        }

        if self.show_about {
            egui::Modal::new(egui::Id::new("about")).show(ctx, |ui| {
                ui.heading("Rootkin");
                ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                ui.label("A free family tree editor. Your data stays on your computer: no cloud, no account, no tracking.");
                ui.label("Every save keeps the previous version next to your file as <name>.familyx.bak.");
                ui.label("Opens and saves .familyx files. Rootkin is an independent project and is not affiliated with Chronoplex Software.");
                ui.hyperlink_to("github.com/hkinks/rootkin", "https://github.com/hkinks/rootkin");
                ui.label("Released into the public domain (Unlicense) or under the MIT license.");
                if ui.button("Close").clicked() {
                    self.show_about = false;
                }
            });
        }

        self.sources_window(ctx);
    }

    // ------------------------------------------------------------ import/export

    /// Pick a file, then import it once unsaved changes are dealt with.
    fn import(&mut self, ctx: &egui::Context) {
        let _ = ctx;
        let Some(path) =
            rfd::FileDialog::new().add_filter("Genealogy files", &["ged", "gedcom", "gramps", "xml"]).pick_file()
        else {
            return;
        };
        self.request_import(path);
    }

    pub(crate) fn request_import(&mut self, path: PathBuf) {
        self.guarded(Pending::Import(path));
    }

    /// Converting a big file takes a while; do it off the UI thread.
    fn start_import(&mut self, path: PathBuf) {
        let tx = self.jobs.0.clone();
        self.busy = Some(format!("Importing {}…", file_name(&path)));
        std::thread::spawn(move || {
            let result = crate::formats::import(&path);
            let _ = tx.send(Job::Imported(path, result));
        });
    }

    fn save_copy_for_app12(&mut self) {
        self.flush_form();
        let Some(pkg) = &self.pkg else { return };
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map_or("family".into(), |s| s.to_string_lossy());
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Family tree", &["familyx"])
            .set_file_name(format!("{stem} (My Family Tree 12).familyx"))
            .save_file()
        else {
            return;
        };
        if self.path.as_ref() == Some(&path) {
            self.toasts.error("Choose another name: the copy must not replace the file you are working on");
            return;
        }
        match pkg.save_for_app12(&path) {
            Ok(downgrade) => {
                let mut msg = format!(
                    "Saved a copy for My Family Tree 12 as {}. Rootkin keeps working on your current file.",
                    file_name(&path)
                );
                if downgrade.password_removed {
                    msg.push_str(" The copy has no password: version 12 can't read this tree's password lock.");
                }
                if downgrade.task_places > 0 {
                    msg.push_str(&format!(
                        " {} research task place(s) keep only their name, as version 12 has no place details for tasks.",
                        downgrade.task_places
                    ));
                }
                self.toasts.info(msg);
            }
            Err(e) => self.toasts.error(format!("Could not save the copy: {e}")),
        }
    }

    fn export_format(&mut self, extension: &str, format: crate::formats::Format) {
        self.flush_form();
        let Some(pkg) = &self.pkg else { return };
        let stem = self.path.as_ref().and_then(|p| p.file_stem()).map_or("family".into(), |s| s.to_string_lossy());
        let Some(path) = rfd::FileDialog::new()
            .add_filter(extension, &[extension])
            .set_file_name(format!("{stem}.{extension}"))
            .save_file()
        else {
            return;
        };
        match crate::formats::export(pkg, &path, format) {
            Ok(()) => {
                self.toasts.info(format!("Exported {} (photos are in the _media folder next to it)", file_name(&path)))
            }
            Err(e) => self.toasts.error(format!("Export failed: {e}")),
        }
    }
}

/// What to tell the user about a file converted from an older app version.
fn migration_notice(pkg: &Package) -> Option<String> {
    let report = pkg.migration()?;
    let mut msg = format!(
        "This file was made by an older version of My Family Tree (format {}). Rootkin converted it \
         to the current format: {} relationships and {} photos were carried over.\n\n\
         Saving writes the current format; your original file is kept next to it as .bak.",
        if report.from_version.is_empty() { "unknown" } else { &report.from_version },
        report.spouse_links + report.parent_links,
        report.media,
    );
    if report.from_version == familyx::migrate::schema14::APP12_VERSION {
        msg.push_str("\n\nTo keep using the tree in My Family Tree 12, use File → Save a copy for My Family Tree 12…");
    }
    if !report.dropped.is_empty() {
        msg.push_str(&format!("\n\nAs the original app does, Rootkin left out: {}.", report.dropped.join("; ")));
    }
    if !report.warnings.is_empty() {
        msg.push_str(&format!(
            "\n\nSome old details could not be converted and are kept unchanged in the file, but \
             won't be shown: {}.\nConsider using \"Save as\" to keep the original untouched.",
            report.warnings.join("; ")
        ));
    }
    Some(msg)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// Sort key for people lists: surname, then given names; unnamed people last.
pub(crate) fn sort_key(p: &Person) -> (bool, String, String) {
    let n = p.primary_name();
    let surname = n.map(|n| n.surname.to_lowercase()).unwrap_or_default();
    let given = n.map(|n| n.given.to_lowercase()).unwrap_or_default();
    (surname.is_empty() && given.is_empty(), surname, given)
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        self.handle_screenshot(&ctx);
        self.shortcuts(&ctx);
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));

        // toasts sit at the bottom of the central area, clear of the side panels
        let central = if self.pkg.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.welcome(ui)).response.rect
        } else {
            egui::Panel::left("people").default_size(270.0).show(ui, |ui| self.people_list(ui));
            egui::Panel::right("person").default_size(400.0).show(ui, |ui| self.person_panel(ui));
            let panel = egui::CentralPanel::default().show(ui, |ui| {
                if self.tree.people.is_empty() {
                    self.empty_tree(ui);
                    return;
                }
                let Some(focus) = self.selected.clone() else { return };
                let nav = NavState { can_back: !self.history.is_empty(), can_forward: !self.future.is_empty() };
                let (pkg, photos, tree) = (self.pkg.as_ref().expect("checked"), &mut self.photos, &self.tree);
                let mut photo = |p: &Person| photos.face(&ctx, pkg, tree, p);
                match self.tree_view.ui(ui, &self.tree, &focus, nav, &mut photo) {
                    Some(TreeAction::Focus(id)) => self.select(&id),
                    Some(TreeAction::Back) => self.go_back(false),
                    Some(TreeAction::Forward) => self.go_back(true),
                    None => {}
                }
            });
            panel.response.rect
        };
        self.relation_dialog_ui(&ctx);
        self.delete_dialog_ui(&ctx);
        self.unlink_dialog_ui(&ctx);
        self.dialogs(&ctx);
        if self.toasts.show(&ctx, central) {
            self.undo(false);
        }
    }
}

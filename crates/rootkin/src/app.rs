use std::path::{Path, PathBuf};
use std::sync::mpsc;

use eframe::egui::{self, Color32, RichText};
use familyx::doc::Document;
use familyx::edit::{self, EditResult};
use familyx::model::{Id, Person, Tree};
use familyx::Package;

use crate::people_list::SortBy;
use crate::person_panel::PersonForm;
use crate::photos::PhotoCache;
use crate::relation_dialog::RelationDialog;
use crate::toasts::Toasts;
use crate::tree_view::{NavState, TreeAction, TreeView};

const UNDO_LIMIT: usize = 100;

/// How long a background job may run before the "Working" overlay shows. Input is blocked
/// from the start; the delay only keeps quick jobs on small trees from flashing a dialog.
const WORKING_OVERLAY_DELAY: std::time::Duration = std::time::Duration::from_millis(400);

/// The result of a background job.
pub(crate) enum Job {
    Opened(PathBuf, Result<Package, String>),
    /// A converted genealogy file and a summary of what came in.
    Imported(PathBuf, Result<(Package, crate::formats::ImportReport), String>),
    /// The same, to be added to the open tree.
    ImportedInto(PathBuf, Result<(Package, crate::formats::ImportReport), String>),
    Saved(PathBuf, Result<(), String>),
    /// The job panicked; what it was doing.
    Crashed(String),
}

/// A finished job with the state of the app when it started.
struct Finished {
    id: u64,
    /// [`App::generation`] when the job started: which tree it belongs to.
    generation: u64,
    /// [`App::version`] when the job started (for a save: the version it wrote).
    version: u64,
    job: Job,
}

/// The job that is running. Only one runs at a time, and editing and file actions wait for it.
struct Busy {
    id: u64,
    label: String,
    since: std::time::Instant,
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
    pub(crate) search: String,
    pub(crate) sort_by: SortBy,
    pub(crate) living_only: bool,
    pub(crate) list_cache: crate::people_list::ListCache,
    pub(crate) list_jump: Option<usize>,
    /// Bumped whenever `tree` is rebuilt; the people list caches against it.
    pub(crate) tree_epoch: u64,
    pub(crate) photos: PhotoCache,
    tree_view: TreeView,
    pub(crate) form: Option<PersonForm>,
    pub(crate) relation_dialog: Option<RelationDialog>,
    pub(crate) confirm_delete: Option<Id>,
    pub(crate) confirm_unlink: Option<crate::person_panel::Unlink>,
    undo: Vec<(String, Document)>,
    redo: Vec<(String, Document)>,
    /// Bumped on every change; a save records which version it wrote.
    pub(crate) version: u64,
    pub(crate) saved_version: u64,
    pub(crate) confirm_close: bool,
    /// Something to do once unsaved changes are dealt with.
    pub(crate) pending: Option<Pending>,
    /// The save started by answering "Save" to the question about unsaved changes; the
    /// waiting action runs when this save (and no other) has finished.
    pending_save: Option<u64>,
    busy: Option<Busy>,
    next_job: u64,
    /// Bumped whenever another tree is loaded; jobs started for an earlier tree are ignored.
    generation: u64,
    jobs: (mpsc::Sender<Finished>, mpsc::Receiver<Finished>),
    pub(crate) recent: Vec<PathBuf>,
    /// The person panel on the right; hide it to give the tree the room.
    pub(crate) show_person_panel: bool,
    pub(crate) toasts: Toasts,
    pub(crate) error_dialog: Option<crate::problems::Problem>,
    /// An informational message shown in a dialog (e.g. after converting an old file).
    notice: Option<String>,
    show_about: bool,
    /// File name and result of the last import, shown until dismissed.
    import_summary: Option<(String, crate::formats::ImportReport)>,
    /// After an import, until the first save: the file it came from and the
    /// file name "Save as" suggests.
    pub(crate) imported_from: Option<(String, String)>,
    /// When the last import was added to the open tree: what it added.
    import_into: Option<ImportedInto>,
    pub(crate) merge_ui: crate::merge_ui::MergeUi,
    pub(crate) sources_ui: crate::sources::SourcesUi,
    /// The open tree was converted from an older file and not saved since.
    pub(crate) converted: bool,
}

/// What "Import into this tree" added, for the summary.
struct ImportedInto {
    people: usize,
    /// New people who look like people that were already in the tree.
    likely_duplicates: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Pending {
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
            list_cache: Default::default(),
            list_jump: None,
            tree_epoch: 0,
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
            pending_save: None,
            busy: None,
            next_job: 0,
            generation: 0,
            jobs: mpsc::channel(),
            recent: crate::recent::load(),
            show_person_panel: true,
            toasts: Toasts::default(),
            error_dialog: None,
            notice: None,
            show_about: false,
            import_summary: None,
            imported_from: None,
            import_into: None,
            merge_ui: Default::default(),
            sources_ui: crate::sources::SourcesUi::default(),
            converted: false,
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

    pub(crate) fn dirty(&self) -> bool {
        self.pkg.is_some() && self.version != self.saved_version
    }

    // ------------------------------------------------------------ files

    /// Run `work` off the UI thread as the one background job. Returns its id, or `None` if
    /// another job is still running. A panic in `work` is reported instead of leaving the app busy.
    pub(crate) fn spawn_job(&mut self, label: String, work: impl FnOnce() -> Job + Send + 'static) -> Option<u64> {
        if self.busy.is_some() {
            return None;
        }
        self.next_job += 1;
        let id = self.next_job;
        let (generation, version) = (self.generation, self.version);
        let (tx, ctx, what) = (self.jobs.0.clone(), self.ctx.clone(), label.clone());
        self.busy = Some(Busy { id, label, since: std::time::Instant::now() });
        std::thread::spawn(move || {
            let job = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or(Job::Crashed(what));
            let _ = tx.send(Finished { id, generation, version, job });
            ctx.request_repaint();
        });
        Some(id)
    }

    /// Opening and saving derive keys with 210k PBKDF2 rounds; run them off
    /// the UI thread.
    pub(crate) fn start_open(&mut self, path: PathBuf) {
        let label = format!("Opening {}…", file_name(&path));
        self.spawn_job(label, move || {
            let result = Package::open(&path).map_err(|e| e.to_string());
            Job::Opened(path, result)
        });
    }

    /// Save in the background; returns the job id (`None` if another job is running).
    pub(crate) fn start_save(&mut self, path: PathBuf) -> Option<u64> {
        if self.busy.is_some() {
            return None;
        }
        self.flush_form();
        let pkg = self.pkg.as_mut()?;
        edit::touch_file(&mut pkg.doc, self.selected.as_deref());
        let pkg = pkg.clone();
        self.spawn_job(format!("Saving {}…", file_name(&path)), move || {
            let result = pkg.save(&path).map_err(|e| e.to_string());
            Job::Saved(path, result)
        })
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        if self.busy.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        while let Ok(done) = self.jobs.1.try_recv() {
            if self.busy.as_ref().map(|b| b.id) != Some(done.id) {
                continue;
            }
            self.busy = None;
            self.finish_job(done);
        }
    }

    fn finish_job(&mut self, done: Finished) {
        // the job belongs to the tree that is open, and that tree hasn't changed since it started
        let same_tree = done.generation == self.generation;
        let unchanged = same_tree && done.version == self.version;
        match done.job {
            // opening or importing would replace changes made in the meantime; adding to a
            // tree only needs it to still be open
            Job::Opened(path, _) | Job::Imported(path, _) if !unchanged => self.stale_result(&path),
            Job::ImportedInto(path, _) if !same_tree => self.stale_result(&path),
            Job::Opened(path, Ok(pkg)) => {
                self.notice = migration_notice(&pkg);
                let converted = self.notice.is_some();
                self.load(pkg, Some(path));
                if converted {
                    // the converted tree only exists in memory until the first save
                    self.converted = true;
                    self.saved_version = u64::MAX;
                }
            }
            Job::Opened(path, Err(e)) => {
                self.error_dialog = Some(crate::problems::cannot_open(&path, &e));
            }
            Job::Imported(path, Ok((pkg, summary))) => {
                self.load(pkg, None);
                self.saved_version = u64::MAX;
                let save_name = crate::formats::suggested_save_name(&summary, &path);
                self.imported_from = Some((file_name(&path), save_name));
                self.import_summary = Some((file_name(&path), summary));
            }
            Job::ImportedInto(path, Ok((other, summary))) => self.finish_import_into(&path, other, summary),
            Job::Imported(path, Err(e)) | Job::ImportedInto(path, Err(e)) => {
                self.error_dialog = Some(crate::problems::cannot_import(&path, &e));
            }
            Job::Saved(path, Ok(())) => {
                crate::recent::remember(&mut self.recent, &path);
                self.toasts.info(format!("Saved {}. The previous version is kept as .bak", file_name(&path)));
                if same_tree {
                    self.saved_version = done.version;
                    self.converted = false;
                    self.path = Some(path);
                }
                if self.pending_save.take() == Some(done.id) {
                    if same_tree && !self.dirty() {
                        self.run_pending();
                    } else if self.pending.is_some() {
                        // changed since the question was answered: ask again
                        self.confirm_close = true;
                    }
                }
            }
            Job::Saved(path, Err(e)) => {
                self.pending = None;
                self.pending_save = None;
                self.error_dialog = Some(crate::problems::cannot_save(&path, &e));
            }
            Job::Crashed(what) => {
                self.pending = None;
                self.pending_save = None;
                let what = what.trim_end_matches('…');
                self.toasts.error(format!("Something went wrong ({what}). Please try again."));
            }
        }
    }

    fn stale_result(&mut self, path: &Path) {
        self.toasts.error(format!("{} was not loaded because the tree changed in the meantime", file_name(path)));
    }

    /// Continue whatever was waiting for the unsaved changes to be dealt with.
    fn run_pending(&mut self) {
        self.pending_save = None;
        match self.pending.take() {
            Some(Pending::Close) => self.ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Some(Pending::Open(path)) => self.start_open(path),
            Some(Pending::Import(path)) => self.start_import(path),
            Some(Pending::New) => self.new_tree(),
            None => {}
        }
    }

    pub(crate) fn load(&mut self, pkg: Package, path: Option<PathBuf>) {
        self.generation += 1;
        self.pending_save = None;
        self.tree = Tree::from_doc(&pkg.doc);
        self.tree_epoch += 1;
        self.selected = self.tree.home_person.clone().or_else(|| self.tree.people.first().map(|p| p.id.clone()));
        self.history.clear();
        self.future.clear();
        self.pkg = Some(pkg);
        self.photos.clear();
        self.undo.clear();
        self.redo.clear();
        self.form = None;
        self.imported_from = None;
        self.import_into = None;
        self.merge_ui = Default::default();
        self.converted = false;
        self.version = 0;
        self.saved_version = if path.is_some() { 0 } else { u64::MAX };
        self.tree_view.fit_now();
        if let Some(path) = &path {
            crate::recent::remember(&mut self.recent, path);
            self.toasts.info(format!("Opened {} ({} people)", file_name(path), self.tree.people.len()));
        }
        self.path = path;
    }

    /// The made-up example family, as a new tree that is not saved yet.
    fn try_example(&mut self) {
        if self.busy.is_some() {
            return;
        }
        match rootkin_formats::example::build() {
            Ok(pkg) => {
                self.load(pkg, None);
                self.toasts.info("This is a made-up family. Use Save as if you want to keep it.");
            }
            Err(e) => self.toasts.error(format!("Could not make the example tree: {e}")),
        }
    }

    fn new_tree(&mut self) {
        if self.busy.is_some() {
            return;
        }
        self.load(Package::new(), None);
        self.saved_version = 0;
        self.relation_dialog = Some(RelationDialog::new_unrelated());
    }

    /// Run `then` now, or after asking to save unsaved changes.
    pub(crate) fn guarded(&mut self, then: Pending) {
        if self.busy.is_some() {
            return;
        }
        self.flush_form();
        self.pending = Some(then);
        self.pending_save = None;
        if self.dirty() {
            self.confirm_close = true;
        } else {
            self.run_pending();
        }
    }

    fn pick_open(&mut self) {
        if self.busy.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]).pick_file() {
            self.guarded(Pending::Open(path));
        }
    }

    /// Save to the current file, or ask for one. Returns the id of the save job.
    fn save(&mut self) -> Option<u64> {
        match self.path.clone() {
            Some(path) => self.start_save(path),
            None => self.pick_save_as(),
        }
    }

    fn pick_save_as(&mut self) -> Option<u64> {
        if self.busy.is_some() {
            return None;
        }
        self.flush_form();
        let mut dialog = rfd::FileDialog::new().add_filter("Family tree", &["familyx"]);
        let suggestion = match (&self.path, &self.imported_from) {
            (Some(path), _) => path.file_name().map(|n| n.to_string_lossy().into_owned()),
            (None, Some((_, save_name))) => Some(save_name.clone()),
            (None, None) => None,
        };
        dialog = dialog.set_file_name(suggestion.unwrap_or_else(|| "family.familyx".into()));
        match dialog.save_file() {
            Some(mut path) => {
                if path.extension().is_none() {
                    path.set_extension("familyx");
                }
                self.start_save(path)
            }
            None => {
                self.pending = None;
                None
            }
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
        if self.refuse_while_busy() {
            return None;
        }
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
        if self.refuse_while_busy() {
            return None;
        }
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

    /// True (with a message) while a background job runs: edits wait for it.
    fn refuse_while_busy(&mut self) -> bool {
        let Some(busy) = &self.busy else { return false };
        self.toasts.error(format!("Please wait: {}", busy.label));
        true
    }

    fn push_undo(&mut self, label: &str, snapshot: Document) {
        self.undo.push((label.to_string(), snapshot));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn undo(&mut self, redo: bool) {
        if self.refuse_while_busy() {
            return;
        }
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
        self.tree_epoch += 1;
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
        if self.busy.is_some() {
            return;
        }
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
                        if ui.button(file_name(&path)).on_hover_text(path.display().to_string()).clicked() {
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
                if ui
                    .add_enabled(has, egui::Button::new("Import into this tree…"))
                    .on_hover_text("Add the people of a GEDCOM, Gramps or zip file to the tree that is open")
                    .clicked()
                {
                    self.import_into();
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
                if ui
                    .add_enabled(self.pkg.is_some(), egui::Button::new("Find duplicate people…"))
                    .on_hover_text("People entered twice, for example after adding a file to this tree")
                    .clicked()
                {
                    self.merge_ui.dups_open = true;
                }
            });
            ui.menu_button("View", |ui| {
                if ui.button("Fit tree to window").clicked() {
                    self.tree_view.fit_now();
                }
                if ui.add_enabled(self.pkg.is_some(), egui::Button::new("Sources…")).clicked() {
                    self.sources_ui.open = true;
                }
                ui.checkbox(&mut self.show_person_panel, "Person panel")
                    .on_hover_text("Hide it on a small screen to give the tree more room");
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
            } else if let (Some((from, _)), true) = (&self.imported_from, self.pkg.is_some()) {
                ui.label(format!("Imported from {from}, not saved yet"));
            } else if self.pkg.is_some() {
                ui.label("New family tree");
            }
            if self.pkg.is_some() {
                if self.dirty() && self.converted {
                    ui.label(RichText::new("Converted, not saved yet").color(Color32::from_rgb(0xe0, 0xa0, 0x30)))
                        .on_hover_text("Press Ctrl+S to write the converted file");
                } else if self.dirty() {
                    ui.label(RichText::new("Unsaved changes").color(Color32::from_rgb(0xe0, 0xa0, 0x30)));
                } else {
                    ui.label(RichText::new("✔ All changes saved").color(Color32::from_rgb(0x60, 0xb0, 0x60)));
                }
                // quick save next to the status
                if self.dirty()
                    && self.busy.is_none()
                    && crate::small_button(ui, "Save now").on_hover_text("Ctrl+S").clicked()
                {
                    self.save();
                }
            }
            if self.pkg.is_some() {
                let label = if self.show_person_panel { "Hide person panel" } else { "Show person panel" };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if panel_toggle(ui, self.show_person_panel, label).on_hover_text(label).clicked() {
                        self.show_person_panel = !self.show_person_panel;
                    }
                });
            }
            if let Some(busy) = &self.busy {
                ui.spinner();
                ui.label(&busy.label);
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
            if ui.link("Try the example tree").on_hover_text("A made-up family to look around in").clicked() {
                self.try_example();
            }
            if !self.recent.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new("Recent").strong());
                for path in self.recent.clone() {
                    ui.add_space(4.0);
                    if ui
                        .link(RichText::new(file_name(&path)).strong())
                        .on_hover_text(path.display().to_string())
                        .clicked()
                    {
                        self.guarded(Pending::Open(path.clone()));
                    }
                    let folder = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                    ui.label(RichText::new(folder).weak().small());
                    if crate::small_button(ui, "Remove from list").clicked() {
                        crate::recent::forget(&mut self.recent, &path);
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

    fn dialogs(&mut self, ctx: &egui::Context) {
        // unsaved changes before close/open/new
        if ctx.input(|i| i.viewport().close_requested()) {
            self.flush_form();
            if self.dirty() {
                // always ask, also when the question is already showing for something else
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                let saving_to_close = self.pending == Some(Pending::Close) && self.pending_save.is_some();
                if !saving_to_close {
                    self.pending = Some(Pending::Close);
                    self.pending_save = None;
                    self.confirm_close = true;
                }
            }
        }
        // the question waits until a running job is done, so that its answer can act
        if self.confirm_close && self.busy.is_none() {
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
                    let waiting = self.pending.clone();
                    match self.save() {
                        Some(id) => {
                            self.pending = waiting;
                            self.pending_save = Some(id);
                        }
                        None => self.pending = None,
                    }
                }
                Some(1) => {
                    self.confirm_close = false;
                    self.saved_version = self.version;
                    self.run_pending();
                }
                Some(_) => {
                    self.confirm_close = false;
                    self.pending = None;
                }
                None => {}
            }
        }

        if let Some(problem) = self.error_dialog.clone() {
            let mut import = false;
            let mut close = false;
            egui::Modal::new(egui::Id::new("error")).show(ctx, |ui| {
                ui.set_max_width(480.0);
                ui.heading(RichText::new(&problem.heading).color(Color32::LIGHT_RED));
                ui.label(&problem.message);
                ui.add_space(4.0);
                ui.label(RichText::new(file_name(&problem.path)).strong())
                    .on_hover_text(problem.path.display().to_string());
                egui::CollapsingHeader::new("Details").show(ui, |ui| {
                    ui.label(RichText::new(problem.path.display().to_string()).monospace());
                    if !problem.details.is_empty() {
                        ui.label(RichText::new(&problem.details).monospace());
                    }
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if problem.offer_import && ui.button(RichText::new("Import").strong()).clicked() {
                        import = true;
                    }
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                });
            });
            if import {
                self.error_dialog = None;
                self.request_import(problem.path);
            } else if close {
                self.error_dialog = None;
            }
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

        if let Some((file, report)) = &self.import_summary {
            let mut choice = None;
            let mut save_report = false;
            let mut find_duplicates = false;
            let into = &self.import_into;
            egui::Modal::new(egui::Id::new("import_summary")).show(ctx, |ui| {
                ui.set_max_width(560.0);
                match into {
                    Some(added) => {
                        ui.heading(format!("Added {file} to this tree"));
                        ui.label(format!(
                            "The file holds {}. {} added to your tree. You can undo this with Edit > Undo.",
                            report.headline(),
                            if added.people == 1 { "1 person was".to_string() } else { format!("{} people were", added.people) },
                        ));
                        if added.likely_duplicates > 0 {
                            ui.label(format!(
                                "{} look{} like {} already in the tree. Check them and merge the ones that are the same person.",
                                if added.likely_duplicates == 1 {
                                    "1 pair of people".to_string()
                                } else {
                                    format!("{} pairs of people", added.likely_duplicates)
                                },
                                if added.likely_duplicates == 1 { "s" } else { "" },
                                if added.likely_duplicates == 1 { "the same person" } else { "the same people" },
                            ));
                        } else {
                            ui.label("None of the new people look like people who were already in the tree.");
                        }
                    }
                    None => {
                        ui.heading(format!("Imported {file}"));
                        ui.label(format!(
                            "{} came in. It is open as a new tree that is not saved yet.",
                            report.headline()
                        ));
                    }
                }
                egui::ScrollArea::vertical().max_height(340.0).auto_shrink([true, true]).show(ui, |ui| {
                    import_report_sections(ui, report);
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    match into {
                        Some(added) => {
                            if added.likely_duplicates > 0
                                && ui.button(RichText::new("Find duplicate people…").strong()).clicked()
                            {
                                find_duplicates = true;
                            }
                            let label = if added.likely_duplicates > 0 { "Later" } else { "OK" };
                            if ui.button(label).clicked() {
                                choice = Some(false);
                            }
                        }
                        None => {
                            if ui.button(RichText::new("Save as .familyx…").strong()).clicked() {
                                choice = Some(true);
                            }
                            if ui.button("Look around first").clicked() {
                                choice = Some(false);
                            }
                        }
                    }
                    if ui.button("Copy details").clicked() {
                        ui.ctx().copy_text(report.to_text(file));
                    }
                    if ui.button("Save report…").clicked() {
                        save_report = true;
                    }
                });
            });
            if save_report {
                self.save_import_report();
            }
            if find_duplicates {
                self.import_summary = None;
                self.merge_ui.dups_open = true;
            }
            if let Some(save) = choice {
                self.import_summary = None;
                if save {
                    self.pick_save_as();
                }
            }
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
        if self.busy.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Genealogy files", &["ged", "gedcom", "gramps", "xml", "gdz", "zip"])
            .pick_file()
        else {
            return;
        };
        self.request_import(path);
    }

    pub(crate) fn request_import(&mut self, path: PathBuf) {
        self.guarded(Pending::Import(path));
    }

    /// Write the import report of the summary dialog to a text file.
    fn save_import_report(&mut self) {
        let Some((file, report)) = &self.import_summary else { return };
        let stem = file.rsplit_once('.').map_or(file.as_str(), |(stem, _)| stem);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Text file", &["txt"])
            .set_file_name(format!("{stem}-import-report.txt"))
            .save_file()
        else {
            return;
        };
        match std::fs::write(&path, report.to_text(file)) {
            Ok(()) => self.toasts.info(format!("Saved the report as {}", file_name(&path))),
            Err(e) => self.toasts.error(format!("Could not save the report: {e}")),
        }
    }

    /// Converting a big file takes a while; do it off the UI thread.
    fn start_import(&mut self, path: PathBuf) {
        self.spawn_job(format!("Importing {}…", file_name(&path)), move || {
            let result = crate::formats::import(&path);
            Job::Imported(path, result)
        });
    }

    /// Pick a file and add its people to the open tree. Nothing is replaced, so no questions asked.
    fn import_into(&mut self) {
        if self.busy.is_some() {
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Genealogy files", &["ged", "gedcom", "gramps", "xml", "gdz", "zip"])
            .pick_file()
        else {
            return;
        };
        self.start_import_into(path);
    }

    pub(crate) fn start_import_into(&mut self, path: PathBuf) {
        self.flush_form();
        self.spawn_job(format!("Reading {}…", file_name(&path)), move || {
            let result = crate::formats::import(&path);
            Job::ImportedInto(path, result)
        });
    }

    /// Add the imported people to the open tree as one undoable step and show what happened.
    fn finish_import_into(&mut self, path: &Path, other: Package, summary: crate::formats::ImportReport) {
        let Some(added) = self.apply_pkg("Import into tree", |pkg| pkg.append(&other)) else { return };
        let new_people: std::collections::HashSet<Id> = added.people.iter().cloned().collect();
        let likely_duplicates = familyx::duplicates::find_duplicates_of_added(&self.tree, &new_people).len();
        self.import_into = Some(ImportedInto { people: added.people.len(), likely_duplicates });
        self.import_summary = Some((file_name(path), summary));
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
    Some(migration_text(pkg.migration()?))
}

fn migration_text(report: &familyx::migrate::Report) -> String {
    let app12 = report.from_version == familyx::migrate::schema14::APP12_VERSION;
    let origin =
        if app12 { "an older version of My Family Tree (version 12)" } else { "an older version of My Family Tree" };
    let mut msg = format!("This file was made by {origin}. Rootkin converted it to the current format.");
    let counts: Vec<String> = [(report.spouse_links + report.parent_links, "relationship"), (report.media, "photo")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| format!("{n} {what}{}", if n == 1 { "" } else { "s" }))
        .collect();
    if !counts.is_empty() {
        msg.push_str(&format!(" {} were carried over.", counts.join(" and ")));
    }
    msg.push_str(
        "\n\nNothing has been written yet. Saving writes the current format; your original file is kept \
         next to it as .bak.",
    );
    if app12 {
        msg.push_str("\n\nTo keep using the tree in My Family Tree 12, use File > Save a copy for My Family Tree 12…");
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
    msg
}

/// A small "side panel" icon: a window outline whose right part is filled
/// while the panel is shown.
fn panel_toggle(ui: &mut egui::Ui, shown: bool, label: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let visuals = ui.style().interact(&response);
    let frame = rect.shrink2(egui::vec2(2.0, 3.0));
    let color = visuals.fg_stroke.color;
    if response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    ui.painter().rect_stroke(frame, 2.0, egui::Stroke::new(1.5, color), egui::StrokeKind::Inside);
    let side = egui::Rect::from_min_max(egui::pos2(frame.right() - 7.0, frame.top()), frame.right_bottom());
    if shown {
        ui.painter().rect_filled(side, 2.0, color);
    } else {
        ui.painter().vline(side.left(), frame.y_range(), egui::Stroke::new(1.5, color));
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
        if self.busy.is_none() {
            self.shortcuts(&ctx);
        }
        egui::Panel::top("menu").show(ui, |ui| self.menu_bar(ui));

        // toasts sit at the bottom of the central area, clear of the side panels
        let central = if self.pkg.is_none() {
            egui::CentralPanel::default().show(ui, |ui| self.welcome(ui)).response.rect
        } else {
            egui::Panel::left("people").default_size(270.0).show(ui, |ui| self.people_list(ui));
            if self.show_person_panel {
                // on a narrow window the panel takes less so the tree isn't squeezed
                let width = (ui.ctx().content_rect().width() * 0.3).clamp(280.0, 400.0);
                egui::Panel::right("person").default_size(width).show(ui, |ui| self.person_panel(ui));
            }
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
        self.merge_dialogs(&ctx);
        self.dialogs(&ctx);
        self.working_overlay(&ctx);
        if self.toasts.show(&ctx, central) {
            self.undo(false);
        }
    }
}

impl App {
    /// While a job runs, a modal blocks the rest of the window. It stays invisible for a moment
    /// so that quick jobs don't flash a dialog.
    fn working_overlay(&mut self, ctx: &egui::Context) {
        let Some(busy) = &self.busy else { return };
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        let visible = busy.since.elapsed() >= WORKING_OVERLAY_DELAY;
        if !visible {
            ctx.request_repaint_after(WORKING_OVERLAY_DELAY);
        }
        let modal = egui::Modal::new(egui::Id::new("working"));
        let modal = if visible { modal } else { modal.backdrop_color(Color32::TRANSPARENT).frame(egui::Frame::NONE) };
        let label = busy.label.clone();
        modal.show(ctx, |ui| {
            if visible {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.heading(label);
                });
                ui.label("Please wait a moment.");
            }
        });
    }
}

/// The groups of an import report: what was left out, what to check and the
/// technical details.
fn import_report_sections(ui: &mut egui::Ui, report: &crate::formats::ImportReport) {
    if !report.left_out.is_empty() {
        ui.add_space(6.0);
        ui.strong("Left out or changed");
        for g in &report.left_out {
            ui.add_space(4.0);
            ui.label(RichText::new(format!("{} ({})", g.title, g.count)).strong());
            ui.label(RichText::new(&g.explanation).weak());
        }
    }
    if !report.duplicates.is_empty() || !report.broken_links.is_empty() {
        ui.add_space(8.0);
        ui.strong("Worth a look");
        let list = |ui: &mut egui::Ui, title: &str, items: &[String]| {
            if !items.is_empty() {
                egui::CollapsingHeader::new(format!("{title}: {}", items.len())).id_salt(title).show(ui, |ui| {
                    for item in items {
                        ui.label(format!("- {item}"));
                    }
                });
            }
        };
        list(ui, "Possible duplicate people", &report.duplicates);
        list(ui, "Links to records that are not in the file", &report.broken_links);
    }
    if !report.warnings.is_empty() {
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Technical details").show(ui, |ui| {
            for w in &report.warnings {
                ui.label(RichText::new(w).monospace());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_notice_mentions_only_meaningful_counts() {
        let mut report = familyx::migrate::Report { from_version: "14.4".into(), ..Default::default() };
        let msg = migration_text(&report);
        assert!(msg.contains("an older version of My Family Tree (version 12)"));
        assert!(!msg.contains("14.4") && !msg.contains("format 1"), "{msg}");
        assert!(!msg.contains("carried over") && !msg.contains(" 0 "), "{msg}");

        report.spouse_links = 2;
        report.parent_links = 3;
        let msg = migration_text(&report);
        assert!(msg.contains("5 relationships were carried over"), "{msg}");

        report.media = 1;
        report.from_version = "9.0".into();
        let msg = migration_text(&report);
        assert!(msg.contains("5 relationships and 1 photo were carried over"), "{msg}");
        assert!(msg.contains("an older version of My Family Tree."), "{msg}");
        assert!(!msg.contains("version 12"), "{msg}");
    }
}

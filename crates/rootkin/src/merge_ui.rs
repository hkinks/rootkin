//! Finding people who appear twice and merging two people into one.

use std::collections::HashSet;

use eframe::egui::{self, RichText};
use familyx::duplicates::{self, DuplicateCandidate};
use familyx::merge::{self, MergeOptions, Side};
use familyx::model::{Gender, Id, Person, Tree};

use crate::app::{sort_key, App};

#[derive(Default)]
pub struct MergeUi {
    pub(crate) dups_open: bool,
    /// Pairs the user said are different people, until the app closes.
    dismissed: HashSet<(Id, Id)>,
    /// The candidate pairs for the tree version they were computed for.
    cache: Option<(u64, Vec<DuplicateCandidate>)>,
    pub(crate) dialog: Option<MergeDialog>,
    pub(crate) picker: Option<Picker>,
}

/// The side-by-side comparison before merging two people.
pub struct MergeDialog {
    a: Id,
    b: Id,
    /// Person `a` is the one that stays.
    keep_a: bool,
    name_from: Side,
    gender_from: Side,
}

impl MergeDialog {
    /// `keep_a`: the first person stays. Otherwise the one with more recorded is kept.
    pub fn new(tree: &Tree, a: &str, b: &str, keep_a: Option<bool>) -> Self {
        let keep_a = keep_a.unwrap_or_else(|| amount_known(tree, a) >= amount_known(tree, b));
        MergeDialog { a: a.to_string(), b: b.to_string(), keep_a, name_from: Side::Kept, gender_from: Side::Kept }
    }

    fn keep_remove(&self) -> (&str, &str) {
        if self.keep_a {
            (&self.a, &self.b)
        } else {
            (&self.b, &self.a)
        }
    }
}

/// "Merge with…": choosing the other person.
pub struct Picker {
    anchor: Id,
    search: String,
    chosen: Option<Id>,
    /// People who look like the anchor: listed first.
    similar: HashSet<Id>,
}

impl Picker {
    pub fn new(anchor: &str, tree: &Tree) -> Self {
        let similar = duplicates::find_duplicates(tree)
            .into_iter()
            .filter_map(|d| {
                if d.a == anchor {
                    Some(d.b)
                } else if d.b == anchor {
                    Some(d.a)
                } else {
                    None
                }
            })
            .collect();
        Picker { anchor: anchor.to_string(), search: String::new(), chosen: None, similar }
    }
}

/// How much is recorded about a person; the fuller record is the better one to keep.
fn amount_known(tree: &Tree, id: &str) -> usize {
    let Some(p) = tree.person(id) else { return 0 };
    p.names.len()
        + p.facts.len()
        + p.notes.len()
        + p.media.len()
        + p.citations.len()
        + tree.parents(id).len()
        + tree.children(id).len()
        + tree.spouses(id).len()
}

fn one_line(p: &Person) -> String {
    format!("{}  {}", p.display_name(), p.lifespan()).trim_end().to_string()
}

fn confidence(score: u32) -> &'static str {
    match score {
        100.. => "Very likely the same person",
        80..=99 => "Likely the same person",
        _ => "Possibly the same person",
    }
}

fn gender_text(g: Gender) -> &'static str {
    match g {
        Gender::Male => "Male",
        Gender::Female => "Female",
        Gender::Other => "Other",
        Gender::Unknown => "Unknown",
    }
}

fn snippet(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 90 {
        format!("{}…", flat.chars().take(90).collect::<String>())
    } else {
        flat
    }
}

/// The rows of the comparison for one person.
fn describe(tree: &Tree, p: &Person) -> Vec<(&'static str, Vec<String>)> {
    let names_of =
        |ids: Vec<&str>| -> Vec<String> { ids.into_iter().filter_map(|id| tree.person(id)).map(one_line).collect() };
    let mut sources: Vec<(String, usize)> = Vec::new();
    let citations = p.citations.iter().chain(p.facts.iter().flat_map(|f| &f.citations));
    for c in citations {
        let title = tree.source(&c.source).map_or_else(|| "(missing source)".to_string(), |s| s.display_title());
        match sources.iter_mut().find(|(t, _)| *t == title) {
            Some((_, n)) => *n += 1,
            None => sources.push((title, 1)),
        }
    }
    vec![
        (
            "Names",
            p.names
                .iter()
                .map(|n| {
                    let mut s = n.display();
                    if p.names.len() > 1 && n.is_primary {
                        s.push_str(" (main name)");
                    } else if !n.name_type.is_empty() {
                        s.push_str(&format!(" ({})", n.name_type.to_lowercase()));
                    }
                    s
                })
                .collect(),
        ),
        ("Gender", vec![gender_text(p.gender).to_string()]),
        (
            "Events",
            p.facts
                .iter()
                .map(|f| {
                    let detail = [f.date.display(), f.place.clone(), f.details.clone()]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(", ");
                    if detail.is_empty() {
                        f.label().to_string()
                    } else {
                        format!("{}: {detail}", f.label())
                    }
                })
                .collect(),
        ),
        ("Parents", names_of(tree.parents(&p.id))),
        ("Partners", names_of(tree.spouses(&p.id))),
        ("Children", names_of(tree.children(&p.id))),
        (
            "Photos",
            p.media
                .iter()
                .filter_map(|m| tree.media.get(m))
                .map(|m| if m.title.is_empty() { m.file_name.clone() } else { m.title.clone() })
                .collect(),
        ),
        ("Notes", p.notes.iter().map(|n| snippet(&n.plain_text())).filter(|s| !s.is_empty()).collect()),
        ("Sources", sources.into_iter().map(|(t, n)| if n > 1 { format!("{t} ({n} citations)") } else { t }).collect()),
    ]
}

impl App {
    /// The duplicates window, the "Merge with…" picker and the comparison.
    pub(crate) fn merge_dialogs(&mut self, ctx: &egui::Context) {
        self.duplicates_window(ctx);
        self.merge_picker_ui(ctx);
        self.merge_dialog_ui(ctx);
    }

    pub(crate) fn open_merge_dialog(&mut self, a: &str, b: &str, keep_a: Option<bool>) {
        self.flush_form();
        self.merge_ui.dialog = Some(MergeDialog::new(&self.tree, a, b, keep_a));
    }

    fn duplicates_window(&mut self, ctx: &egui::Context) {
        if !self.merge_ui.dups_open {
            return;
        }
        if self.merge_ui.cache.as_ref().is_none_or(|(v, _)| *v != self.version) {
            let found = duplicates::find_duplicates(&self.tree);
            self.merge_ui.cache = Some((self.version, found));
        }
        let mut open = true;
        let mut compare: Option<(Id, Id)> = None;
        let mut dismiss: Option<(Id, Id)> = None;
        let (tree, ui_state) = (&self.tree, &self.merge_ui);
        egui::Window::new("Duplicate people").open(&mut open).default_width(560.0).default_height(460.0).show(
            ctx,
            |ui| {
                let rows: Vec<&DuplicateCandidate> = ui_state
                    .cache
                    .iter()
                    .flat_map(|(_, c)| c)
                    .filter(|d| !ui_state.dismissed.contains(&(d.a.clone(), d.b.clone())))
                    .collect();
                if rows.is_empty() {
                    ui.label("No people look like duplicates. Rootkin looks for the same name with birth years at most 2 years apart, or one year missing.");
                    return;
                }
                let n = rows.len();
                ui.label(format!(
                    "{n} {} of people may be the same person. The likeliest come first.",
                    if n == 1 { "pair" } else { "pairs" }
                ));
                ui.add_space(4.0);
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    for d in rows {
                        let (Some(a), Some(b)) = (tree.person(&d.a), tree.person(&d.b)) else { continue };
                        ui.push_id((&d.a, &d.b), |ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(one_line(a));
                                    ui.label(one_line(b));
                                    let how = if d.via_alternate_name { ", through another name" } else { "" };
                                    ui.label(RichText::new(format!("{}{how}", confidence(d.score))).weak());
                                });
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Compare and merge…").clicked() {
                                        compare = Some((d.a.clone(), d.b.clone()));
                                    }
                                    if ui
                                        .button("Not the same")
                                        .on_hover_text("Stop listing this pair")
                                        .clicked()
                                    {
                                        dismiss = Some((d.a.clone(), d.b.clone()));
                                    }
                                });
                            });
                            ui.separator();
                        });
                    }
                });
            },
        );
        if !open {
            self.merge_ui.dups_open = false;
        }
        if let Some(pair) = dismiss {
            self.merge_ui.dismissed.insert(pair);
        }
        if let Some((a, b)) = compare {
            self.open_merge_dialog(&a, &b, None);
        }
    }

    fn merge_picker_ui(&mut self, ctx: &egui::Context) {
        let Some(mut picker) = self.merge_ui.picker.take() else { return };
        let Some(anchor) = self.tree.person(&picker.anchor) else { return };
        let mut done = false;
        let mut go = false;
        let modal = egui::Modal::new(egui::Id::new("merge_picker")).show(ctx, |ui| {
            ui.set_width(400.0);
            ui.heading(format!("Merge {} with…", anchor.display_name()));
            ui.label("Choose the other record of the same person. You compare them before anything changes.");
            ui.add(egui::TextEdit::singleline(&mut picker.search).hint_text("Search…").desired_width(f32::INFINITY));
            let needle = picker.search.to_lowercase();
            let mut people: Vec<_> = self
                .tree
                .people
                .iter()
                .filter(|p| p.id != picker.anchor)
                .filter(|p| needle.is_empty() || p.display_name().to_lowercase().contains(&needle))
                .collect();
            people.sort_by_cached_key(|p| (!picker.similar.contains(&p.id), sort_key(p)));
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for p in people {
                    let selected = picker.chosen.as_deref() == Some(p.id.as_str());
                    let note = if picker.similar.contains(&p.id) { "  (looks similar)" } else { "" };
                    let r = ui.selectable_label(selected, format!("{}{note}", one_line(p)));
                    if r.clicked() {
                        picker.chosen = Some(p.id.clone());
                    }
                    if r.double_clicked() {
                        go = true;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                go |= ui.add_enabled(picker.chosen.is_some(), egui::Button::new("Compare…")).clicked();
                done |= ui.button("Cancel").clicked();
            });
        });
        done |= modal.should_close();
        if go {
            if let Some(chosen) = picker.chosen.clone() {
                self.open_merge_dialog(&picker.anchor, &chosen, Some(true));
                return;
            }
        }
        if !done {
            self.merge_ui.picker = Some(picker);
        }
    }

    fn merge_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.merge_ui.dialog.take() else { return };
        let (Some(a), Some(b)) = (self.tree.person(&dlg.a), self.tree.person(&dlg.b)) else { return };
        let mut done = false;
        let mut merge_now = false;
        let modal = egui::Modal::new(egui::Id::new("merge_dialog")).show(ctx, |ui| {
            ui.set_width(760.0);
            ui.heading("Merge two people");
            ui.label(
                "Pick the record to keep. Everything the other one has is added to it, \
                 and the other one is deleted. You can undo this afterwards.",
            );
            ui.add_space(6.0);
            let (rows_a, rows_b) = (describe(&self.tree, a), describe(&self.tree, b));
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("merge_compare")
                    .num_columns(3)
                    .min_col_width(80.0)
                    .spacing([16.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label("");
                        ui.radio_value(
                            &mut dlg.keep_a,
                            true,
                            RichText::new(format!("Keep {}", a.display_name())).strong(),
                        );
                        ui.radio_value(
                            &mut dlg.keep_a,
                            false,
                            RichText::new(format!("Keep {}", b.display_name())).strong(),
                        );
                        ui.end_row();
                        for ((title, lines_a), (_, lines_b)) in rows_a.iter().zip(&rows_b) {
                            ui.label(RichText::new(*title).strong());
                            for lines in [lines_a, lines_b] {
                                ui.vertical(|ui| {
                                    ui.set_width(300.0);
                                    if lines.is_empty() {
                                        ui.weak("none");
                                    }
                                    for line in lines {
                                        ui.label(line);
                                    }
                                });
                            }
                            ui.end_row();
                        }
                    });
            });
            ui.add_space(6.0);
            let (keep, gone) = if dlg.keep_a { (a, b) } else { (b, a) };
            ui.label(format!(
                "{} stays. {} is merged into them and deleted.",
                keep.display_name(),
                gone.display_name()
            ));
            if let (Some(kn), Some(gn)) = (keep.primary_name(), gone.primary_name()) {
                let same = |x: &familyx::model::Name, y: &familyx::model::Name| {
                    duplicates::fold(&x.display()).split_whitespace().collect::<Vec<_>>()
                        == duplicates::fold(&y.display()).split_whitespace().collect::<Vec<_>>()
                };
                if !same(kn, gn) {
                    ui.horizontal(|ui| {
                        ui.label("Main name:");
                        ui.radio_value(&mut dlg.name_from, Side::Kept, kn.display());
                        ui.radio_value(&mut dlg.name_from, Side::Removed, gn.display());
                    });
                    ui.label(RichText::new("The other name is kept as an alternate name.").weak());
                } else {
                    dlg.name_from = Side::Kept;
                }
            }
            if keep.gender != gone.gender && keep.gender != Gender::Unknown && gone.gender != Gender::Unknown {
                ui.horizontal(|ui| {
                    ui.label("Gender:");
                    ui.radio_value(&mut dlg.gender_from, Side::Kept, gender_text(keep.gender));
                    ui.radio_value(&mut dlg.gender_from, Side::Removed, gender_text(gone.gender));
                });
            } else {
                dlg.gender_from = Side::Kept;
            }
            ui.separator();
            ui.horizontal(|ui| {
                merge_now = ui.button(RichText::new("Merge").strong()).clicked();
                done |= ui.button("Cancel").clicked();
            });
        });
        done |= modal.should_close();

        if merge_now {
            let (keep, remove) = dlg.keep_remove();
            let (keep, remove) = (keep.to_string(), remove.to_string());
            let names = |id: &str| self.tree.person(id).map(|p| p.display_name()).unwrap_or_default();
            let message = format!("Merged {} into {}", names(&remove), names(&keep));
            let opts = MergeOptions { name_from: dlg.name_from, gender_from: dlg.gender_from };
            self.flush_form();
            if self.apply("Merge people", |doc| merge::merge_people(doc, &keep, &remove, opts)).is_some() {
                self.select(&keep);
                self.form = None;
                self.toasts.undoable(message);
            }
            return;
        }
        if !done {
            self.merge_ui.dialog = Some(dlg);
        }
    }
}

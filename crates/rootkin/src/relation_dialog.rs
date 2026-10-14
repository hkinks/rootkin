//! Modal for adding a person, or relating the selected person to an
//! existing or new one.

use eframe::egui;
use familyx::doc::Document;
use familyx::edit::{self, EditResult};
use familyx::model::{Gender, Id, Tree};

use crate::app::{sort_key, App};

#[derive(Clone, Copy, PartialEq)]
pub enum Relation {
    Parent,
    Spouse,
    Child,
}

pub struct RelationDialog {
    /// `None`: just add a new, unrelated person.
    relation: Option<(Relation, Id)>,
    use_existing: bool,
    search: String,
    chosen: Option<Id>,
    given: String,
    surname: String,
    gender: Gender,
    /// For a new child: the anchor's spouse who is the other parent.
    co_parent: Option<Id>,
    /// Put the cursor in "Given names" on the first frame.
    focus_given: bool,
    /// For a new parent of someone with one parent: make the two a couple.
    couple: bool,
    /// The existing parent to couple with, if the checkbox applies and is on.
    couple_with: Option<Id>,
}

impl RelationDialog {
    pub fn new_unrelated() -> Self {
        RelationDialog {
            relation: None,
            use_existing: false,
            search: String::new(),
            chosen: None,
            given: String::new(),
            surname: String::new(),
            gender: Gender::Unknown,
            co_parent: None,
            focus_given: true,
            couple: true,
            couple_with: None,
        }
    }

    pub fn new(relation: Relation, anchor: &str, tree: &Tree) -> Self {
        let anchor_surname = tree.person(anchor).and_then(|p| p.primary_name()).map(|n| n.surname.clone());
        let spouses = tree.spouses(anchor);
        RelationDialog {
            relation: Some((relation, anchor.to_string())),
            // children usually share the surname; parents/spouses often don't
            surname: if relation == Relation::Child { anchor_surname.unwrap_or_default() } else { String::new() },
            co_parent: (relation == Relation::Child && spouses.len() == 1).then(|| spouses[0].to_string()),
            ..Self::new_unrelated()
        }
    }
}

impl RelationDialog {
    /// Preselect the other parent of a new child.
    pub fn set_co_parent(&mut self, co_parent: Option<Id>) {
        self.co_parent = co_parent;
    }
}

impl App {
    pub(crate) fn relation_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(mut dlg) = self.relation_dialog.take() else { return };
        let mut done = false;
        let mut submit = false;

        let anchor_name =
            dlg.relation.as_ref().and_then(|(_, a)| self.tree.person(a)).map(|p| p.display_name()).unwrap_or_default();
        let title = match &dlg.relation {
            None => "New person".to_string(),
            Some((Relation::Parent, _)) => format!("Add parent of {anchor_name}"),
            Some((Relation::Spouse, _)) => format!("Add spouse of {anchor_name}"),
            Some((Relation::Child, _)) => format!("Add child of {anchor_name}"),
        };

        let modal = egui::Modal::new(egui::Id::new("relation_dialog")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.heading(title);
            if dlg.relation.is_some() {
                ui.horizontal(|ui| {
                    ui.radio_value(&mut dlg.use_existing, false, "New person");
                    ui.radio_value(&mut dlg.use_existing, true, "Existing person");
                });
            }
            ui.separator();

            if dlg.use_existing {
                ui.add(egui::TextEdit::singleline(&mut dlg.search).hint_text("Search…").desired_width(f32::INFINITY));
                let needle = dlg.search.to_lowercase();
                let anchor = dlg.relation.as_ref().map(|(_, a)| a.as_str());
                let mut people: Vec<_> = self
                    .tree
                    .people
                    .iter()
                    .filter(|p| Some(p.id.as_str()) != anchor)
                    .filter(|p| needle.is_empty() || p.display_name().to_lowercase().contains(&needle))
                    .collect();
                people.sort_by_cached_key(|p| sort_key(p));
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    for p in people {
                        let selected = dlg.chosen.as_deref() == Some(p.id.as_str());
                        let r = ui.selectable_label(selected, format!("{}  {}", p.display_name(), p.lifespan()));
                        if r.clicked() {
                            dlg.chosen = Some(p.id.clone());
                        }
                        if r.double_clicked() {
                            submit = true;
                        }
                    }
                });
            } else {
                egui::Grid::new("new_person").num_columns(2).show(ui, |ui| {
                    ui.label("Given names");
                    let r = ui.add(egui::TextEdit::singleline(&mut dlg.given).desired_width(240.0));
                    // Enter ends editing; keep the cursor here when it doesn't add anybody
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        r.request_focus();
                    }
                    // keep asking until it sticks: the first frames of a modal can't take focus yet
                    if dlg.focus_given {
                        if r.has_focus() {
                            dlg.focus_given = false;
                        } else {
                            r.request_focus();
                        }
                    }
                    ui.end_row();
                    ui.label("Surname");
                    ui.add(egui::TextEdit::singleline(&mut dlg.surname).desired_width(240.0));
                    ui.end_row();
                    ui.label("Gender");
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut dlg.gender, Gender::Male, "Male");
                        ui.radio_value(&mut dlg.gender, Gender::Female, "Female");
                        ui.radio_value(&mut dlg.gender, Gender::Unknown, "Unknown");
                    });
                    ui.end_row();
                });
            }

            if let Some((Relation::Child, anchor)) = &dlg.relation {
                let spouses = self.tree.spouses(anchor);
                if !spouses.is_empty() {
                    let label = |id: Option<&str>| {
                        id.and_then(|id| self.tree.person(id))
                            .map(|p| p.display_name())
                            .unwrap_or_else(|| "nobody".into())
                    };
                    ui.horizontal(|ui| {
                        ui.label("Other parent:");
                        egui::ComboBox::from_id_salt("co_parent")
                            .selected_text(label(dlg.co_parent.as_deref()))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut dlg.co_parent, None, "nobody");
                                for s in spouses {
                                    ui.selectable_value(&mut dlg.co_parent, Some(s.to_string()), label(Some(s)));
                                }
                            });
                    });
                }
            }

            dlg.couple_with = None;
            if let Some((Relation::Parent, anchor)) = &dlg.relation {
                if let [parent] = self.tree.parents(anchor)[..] {
                    let name_of = |id: &str| self.tree.person(id).map(|p| p.display_name()).unwrap_or_default();
                    let new_name = match dlg.chosen.as_deref().filter(|_| dlg.use_existing) {
                        Some(id) => name_of(id),
                        None if dlg.use_existing => String::new(),
                        None => format!("{} {}", dlg.given.trim(), dlg.surname.trim()).trim().to_string(),
                    };
                    let new_name = if new_name.is_empty() { "The new parent".to_string() } else { new_name };
                    ui.checkbox(&mut dlg.couple, format!("{new_name} and {} are a couple", name_of(parent)));
                    let is_same = dlg.use_existing && dlg.chosen.as_deref() == Some(parent);
                    dlg.couple_with = (dlg.couple && !is_same).then(|| parent.to_string());
                }
            }

            ui.separator();
            let ready = if dlg.use_existing {
                dlg.chosen.is_some()
            } else {
                !(dlg.given.trim().is_empty() && dlg.surname.trim().is_empty())
            };
            ui.horizontal(|ui| {
                submit |= ui.add_enabled(ready, egui::Button::new("Add")).clicked();
                // Enter needs a given name: the surname alone may just be prefilled
                submit |= ready
                    && !dlg.use_existing
                    && !dlg.given.trim().is_empty()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                done |= ui.button("Cancel").clicked();
            });
        });
        done |= modal.should_close();

        if submit {
            let dlg_ref = &dlg;
            if let Some(id) = self.apply("Add person", |doc| commit(doc, dlg_ref)) {
                let name = self.tree.person(&id).map(|p| p.display_name()).unwrap_or_default();
                if dlg.relation.is_none() {
                    self.select(&id);
                }
                self.form = None;
                self.toasts.undoable(format!("Added {name}"));
            }
            done = true;
        }
        if !done {
            self.relation_dialog = Some(dlg);
        }
    }
}

/// Apply the dialog; returns the id of the new or chosen person.
fn commit(doc: &mut Document, dlg: &RelationDialog) -> EditResult<Id> {
    let other = match (&dlg.chosen, dlg.use_existing) {
        (Some(id), true) => id.clone(),
        _ => edit::add_person(doc, dlg.given.trim(), dlg.surname.trim(), dlg.gender),
    };
    match &dlg.relation {
        None => {}
        Some((Relation::Parent, anchor)) => {
            edit::link_parent(doc, anchor, &other)?;
            if let Some(partner) = &dlg.couple_with {
                edit::link_spouse(doc, partner, &other)?;
            }
        }
        Some((Relation::Spouse, anchor)) => {
            edit::link_spouse(doc, anchor, &other)?;
        }
        Some((Relation::Child, anchor)) => {
            edit::link_parent(doc, &other, anchor)?;
            if let Some(co) = &dlg.co_parent {
                edit::link_parent(doc, &other, co)?;
            }
        }
    }
    Ok(other)
}

//! Right-hand panel: view and edit the selected person.

use eframe::egui::{self, Color32, RichText};
use familyx::edit::{self, FactInput};
use familyx::model::{fact_label, Date, Fact, Gender, Id, Person};

use crate::app::App;
use crate::relation_dialog::{Relation, RelationDialog};

/// Fact tags offered in "Add fact".
const FACT_TAGS: &[&str] = &["BIRT", "DEAT", "BURI", "CHR", "BAPM", "OCCU", "EDUC", "RESI", "ADOP", "EVEN"];

/// Text buffers for the selected person; committed on focus loss.
pub struct PersonForm {
    person_id: Id,
    given: String,
    surname: String,
    facts: Vec<FactForm>,
}

struct FactForm {
    id: Id,
    tag: String,
    year: String,
    month: String,
    day: String,
    about: bool,
    place: String,
    details: String,
}

impl FactForm {
    fn new(f: &Fact) -> Self {
        let num = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_default();
        FactForm {
            id: f.id.clone(),
            tag: f.tag.clone(),
            year: num(f.date.year.map(i64::from)),
            month: num(f.date.month.map(i64::from)),
            day: num(f.date.day.map(i64::from)),
            about: f.date.descriptor == "About",
            place: f.place.clone(),
            details: f.details.clone(),
        }
    }

    /// Parsed input, or `None` if a date part is invalid.
    fn input(&self) -> Option<FactInput> {
        fn part<T: std::str::FromStr + PartialOrd>(s: &str, range: std::ops::RangeInclusive<T>) -> Result<Option<T>, ()> {
            let s = s.trim();
            if s.is_empty() {
                return Ok(None);
            }
            s.parse().ok().filter(|v| range.contains(v)).map(Some).ok_or(())
        }
        Some(FactInput {
            date: Date {
                year: part(&self.year, 1..=9999).ok()?,
                month: part(&self.month, 1..=12).ok()?,
                day: part(&self.day, 1..=31).ok()?,
                descriptor: if self.about { "About" } else { "Span" }.into(),
            },
            place: self.place.trim().to_string(),
            details: self.details.trim().to_string(),
        })
    }
}

impl PersonForm {
    fn new(p: &Person) -> Self {
        let name = p.primary_name();
        PersonForm {
            person_id: p.id.clone(),
            given: name.map(|n| n.given.clone()).unwrap_or_default(),
            surname: name.map(|n| n.surname.clone()).unwrap_or_default(),
            facts: p.facts.iter().map(FactForm::new).collect(),
        }
    }
}

enum Action {
    Goto(Id),
    Focus(Id),
    Rename(String, String),
    Gender(Gender),
    Living(bool),
    UpdateFact(Id, FactInput),
    DeleteFact(Id),
    AddFact(String),
    AddCoupleFact(Id, &'static str),
    UnlinkParent(Id),
    UnlinkSpouse(Id),
    UnlinkChild(Id),
    AddRelation(Relation),
    Delete,
}

/// Draws a fact editor; returns a commit action when an edit finishes.
fn fact_editor(ui: &mut egui::Ui, form: &mut FactForm, original: Option<&Fact>) -> Option<Action> {
    let mut commit = false;
    let mut action = None;
    let valid = form.input().is_some();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new(fact_label(&form.tag)).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("🗑").on_hover_text("Delete this fact").clicked() {
                    action = Some(Action::DeleteFact(form.id.clone()));
                }
            });
        });
        ui.horizontal(|ui| {
            let field = |ui: &mut egui::Ui, text: &mut String, hint: &str, width: f32| {
                let r = ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(width));
                r.lost_focus()
            };
            commit |= field(ui, &mut form.year, "year", 44.0);
            commit |= field(ui, &mut form.month, "mm", 30.0);
            commit |= field(ui, &mut form.day, "dd", 30.0);
            commit |= ui.checkbox(&mut form.about, "about").changed();
            if !valid {
                ui.colored_label(Color32::LIGHT_RED, "invalid date");
            }
        });
        let wide = |ui: &mut egui::Ui, text: &mut String, hint: &str| {
            ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY)).lost_focus()
        };
        commit |= wide(ui, &mut form.place, "place");
        commit |= wide(ui, &mut form.details, "details");
    });
    if action.is_some() {
        return action;
    }
    let input = form.input()?;
    // a missing descriptor means an exact date, same as "Span"
    let normalized = |d: &Date| Date {
        descriptor: if d.descriptor.is_empty() { "Span".into() } else { d.descriptor.clone() },
        ..d.clone()
    };
    let unchanged = original
        .is_some_and(|f| normalized(&f.date) == input.date && f.place == input.place && f.details == input.details);
    (commit && !unchanged).then(|| Action::UpdateFact(form.id.clone(), input))
}

impl App {
    pub(crate) fn person_panel(&mut self, ui: &mut egui::Ui) {
        let Some(person) = self.selected.as_deref().and_then(|id| self.tree.person(id)).cloned() else {
            ui.weak("No person selected");
            return;
        };
        if self.form.as_ref().is_none_or(|f| f.person_id != person.id) {
            self.form = Some(PersonForm::new(&person));
        }
        let mut action = None;
        let mut form = self.form.take().expect("set above");

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if let (Some(pkg), Some(media)) = (&self.pkg, self.tree.primary_photo(&person)) {
                if let Some(tex) = self.photos.get(ui.ctx(), pkg, &media.part) {
                    ui.add(egui::Image::new(&tex).max_size(egui::vec2(ui.available_width(), 240.0)).corner_radius(6));
                }
            }

            // names
            egui::Grid::new("names").num_columns(2).show(ui, |ui| {
                ui.label("Given names");
                let g = ui.add(egui::TextEdit::singleline(&mut form.given).desired_width(f32::INFINITY));
                ui.end_row();
                ui.label("Surname");
                let s = ui.add(egui::TextEdit::singleline(&mut form.surname).desired_width(f32::INFINITY));
                ui.end_row();
                let current = person.primary_name();
                let changed = current.map(|n| (n.given.as_str(), n.surname.as_str()))
                    != Some((form.given.trim(), form.surname.trim()));
                if (g.lost_focus() || s.lost_focus()) && changed {
                    action = Some(Action::Rename(form.given.trim().into(), form.surname.trim().into()));
                }
            });

            ui.horizontal(|ui| {
                let mut gender = person.gender;
                for (g, label) in [(Gender::Male, "Male"), (Gender::Female, "Female"), (Gender::Unknown, "Unknown")] {
                    ui.radio_value(&mut gender, g, label);
                }
                if gender != person.gender {
                    action = Some(Action::Gender(gender));
                }
                ui.separator();
                let mut living = person.is_living;
                if ui.checkbox(&mut living, "Living").changed() {
                    action = Some(Action::Living(living));
                }
            });
            ui.horizontal(|ui| {
                if self.focus.as_deref() != Some(person.id.as_str()) && ui.button("Centre tree here").clicked() {
                    action = Some(Action::Focus(person.id.clone()));
                }
                if ui.button("Delete person…").clicked() {
                    action = Some(Action::Delete);
                }
            });

            if person.names.len() > 1 {
                ui.add_space(6.0);
                ui.label(RichText::new("Other names").strong());
                for n in person.names.iter().filter(|n| !n.is_primary) {
                    ui.weak(if n.name_type.is_empty() { n.display() } else { format!("{} ({})", n.display(), n.name_type) });
                }
            }

            // facts
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Facts").heading());
                egui::ComboBox::from_id_salt("add_fact").selected_text("+ Add fact").show_ui(ui, |ui| {
                    for tag in FACT_TAGS {
                        if ui.selectable_label(false, fact_label(tag)).clicked() {
                            action = Some(Action::AddFact(tag.to_string()));
                        }
                    }
                });
            });
            for fact_form in &mut form.facts {
                let original = person.facts.iter().find(|f| f.id == fact_form.id);
                if let Some(a) = fact_editor(ui, fact_form, original) {
                    action = Some(a);
                }
            }

            // family
            ui.add_space(8.0);
            ui.label(RichText::new("Family").heading());
            let relatives = |ui: &mut egui::Ui,
                             title: &str,
                             ids: Vec<&str>,
                             add: Relation,
                             unlink: fn(Id) -> Action,
                             action: &mut Option<Action>| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).strong());
                    if ui.small_button("+").on_hover_text(format!("Add {}", title.to_lowercase())).clicked() {
                        *action = Some(Action::AddRelation(add));
                    }
                });
                for id in ids {
                    let Some(p) = self.tree.person(id) else { continue };
                    ui.horizontal(|ui| {
                        if ui.link(format!("{}  {}", p.display_name(), p.lifespan())).clicked() {
                            *action = Some(Action::Goto(id.to_string()));
                        }
                        if ui.small_button("×").on_hover_text("Remove this relationship (keeps the person)").clicked() {
                            *action = Some(unlink(id.to_string()));
                        }
                    });
                }
            };
            relatives(ui, "Parents", self.tree.parents(&person.id), Relation::Parent, Action::UnlinkParent, &mut action);
            relatives(ui, "Spouses", self.tree.spouses(&person.id), Relation::Spouse, Action::UnlinkSpouse, &mut action);
            for link in self.tree.spouse_links_of(&person.id) {
                let spouse = self.tree.person(link.other(&person.id)).map(|p| p.display_name()).unwrap_or_default();
                for f in &link.facts {
                    ui.weak(format!("{} with {spouse}: {} {}", f.label(), f.date.display(), f.place));
                }
                if !link.facts.iter().any(|f| f.tag == "MARR") && ui.small_button(format!("+ Marriage with {spouse}")).clicked() {
                    action = Some(Action::AddCoupleFact(link.id.clone(), "MARR"));
                }
            }
            relatives(ui, "Children", self.tree.children(&person.id), Relation::Child, Action::UnlinkChild, &mut action);
        });
        self.form = Some(form);

        let pid = person.id.clone();
        match action {
            None => {}
            Some(Action::Goto(id)) => {
                self.select(&id);
                self.focus = Some(id);
            }
            Some(Action::Focus(id)) => self.focus = Some(id),
            Some(Action::Rename(given, surname)) => {
                self.apply(|d| edit::set_name(d, &pid, &given, &surname));
            }
            Some(Action::Gender(g)) => {
                self.apply(|d| edit::set_gender(d, &pid, g));
            }
            Some(Action::Living(l)) => {
                self.apply(|d| edit::set_living(d, &pid, l));
            }
            Some(Action::UpdateFact(id, input)) => {
                self.apply(|d| edit::update_fact(d, &id, &input));
            }
            Some(Action::DeleteFact(id)) => {
                self.apply(|d| edit::delete_fact(d, &id));
            }
            Some(Action::AddFact(tag)) => {
                self.apply(|d| edit::add_fact(d, &pid, &tag, &FactInput::default()));
            }
            Some(Action::AddCoupleFact(link, tag)) => {
                self.apply(|d| edit::add_couple_fact(d, &link, tag, &FactInput::default()));
            }
            Some(Action::UnlinkParent(parent)) => {
                self.apply(|d| edit::unlink_parent(d, &pid, &parent));
            }
            Some(Action::UnlinkChild(child)) => {
                self.apply(|d| edit::unlink_parent(d, &child, &pid));
            }
            Some(Action::UnlinkSpouse(spouse)) => {
                self.apply(|d| edit::unlink_spouse(d, &pid, &spouse));
            }
            Some(Action::AddRelation(kind)) => {
                self.relation_dialog = Some(RelationDialog::new(kind, &pid, &self.tree));
            }
            Some(Action::Delete) => self.confirm_delete = Some(pid),
        }
    }

    pub(crate) fn delete_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else { return };
        let name = self.tree.person(&id).map(|p| p.display_name()).unwrap_or_default();
        let mut close = false;
        let mut delete = false;
        egui::Modal::new(egui::Id::new("confirm_delete")).show(ctx, |ui| {
            ui.heading(format!("Delete {name}?"));
            ui.label("This removes the person and all their relationships. You can undo it with Ctrl+Z.");
            ui.horizontal(|ui| {
                delete = ui.button("Delete").clicked();
                close = ui.button("Cancel").clicked();
            });
        });
        if delete {
            self.apply(|d| edit::delete_person(d, &id));
            self.status = Some(format!("Deleted {name}"));
        }
        if delete || close {
            self.confirm_delete = None;
        }
    }
}

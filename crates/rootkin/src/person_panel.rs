//! Right-hand panel: view and edit the selected person.
//!
//! Text fields edit a [`PersonForm`]; [`PersonForm::pending_edits`] compares
//! it with the document, and the app applies the differences when a field
//! loses focus and before saving, undoing or navigating, so typing is never
//! lost. An unparseable date stays in its field (marked red) until fixed.

use eframe::egui::{self, Color32, RichText};
use familyx::edit::{self, FactInput};
use familyx::model::{fact_label, Date, Fact, Gender, Id, Person, Tree};

use crate::app::App;
use crate::relation_dialog::{Relation, RelationDialog};
use crate::tree_view::gender_symbol;

/// Events offered for a person, in menu order.
const PERSON_EVENTS: &[&str] = &[
    "BIRT", "BAPM", "CHR", "DEAT", "BURI", "CREM", "OCCU", "EDUC", "GRAD", "RESI", "EMIG", "IMMI", "NATU", "RELI",
    "ADOP", "RETI",
];
/// Events offered for a couple.
const COUPLE_EVENTS: &[&str] = &["MARR", "ENGA", "MARB", "DIV", "ANUL"];
/// The app's `NameType` values (writing others would break loading in the app).
const NAME_TYPES: &[&str] = &[
    "Birth",
    "Married",
    "Maiden",
    "Aka",
    "Nick",
    "FormerSurname",
    "Adopted",
    "Immigrant",
    "Professional",
    "Religious",
    "Custom",
];
/// The app's `ParentChildModifier` values; empty = biological.
const PARENT_KINDS: &[(&str, &str)] =
    &[("", "Biological"), ("Adopted", "Adoptive"), ("Foster", "Foster"), ("Step", "Step"), ("Guardian", "Guardian")];
/// The app's `SpouseRelationshipStatus` values.
const COUPLE_STATUS: &[(&str, &str)] = &[
    ("", "—"),
    ("Current", "Together"),
    ("Engaged", "Engaged"),
    ("Separated", "Separated"),
    ("Divorced", "Divorced"),
    ("Former", "Former partner"),
    ("Deceased", "Partner died"),
    ("Annulled", "Annulled"),
];

pub struct PersonForm {
    person_id: Id,
    given: String,
    surname: String,
    note: String,
    facts: Vec<FactForm>,
    names: Vec<NameForm>,
    /// Typed name for a new custom event.
    custom_event: String,
}

struct FactForm {
    id: Id,
    date: String,
    place: String,
    details: String,
    note: String,
}

struct NameForm {
    id: Id,
    given: String,
    surname: String,
    name_type: String,
}

/// A change found in the form, applied by the app.
pub enum FormEdit {
    Rename(String, String),
    PersonNote(String),
    Fact(Id, FactInput),
    FactNote(Id, String),
    Name(Id, String, String, String),
}

impl FactForm {
    fn new(f: &Fact) -> Self {
        FactForm {
            id: f.id.clone(),
            date: f.date.display(),
            place: f.place.clone(),
            details: f.details.clone(),
            note: f.notes.first().map(|n| n.plain_text()).unwrap_or_default(),
        }
    }

    fn input(&self) -> Result<FactInput, String> {
        Ok(FactInput {
            date: Date::parse(&self.date)?,
            place: self.place.trim().to_string(),
            details: self.details.trim().to_string(),
        })
    }
}

fn first_note(notes: &[familyx::model::Note]) -> String {
    notes.first().map(|n| n.plain_text()).unwrap_or_default()
}

impl PersonForm {
    fn new(p: &Person, tree: &Tree) -> Self {
        let name = p.primary_name();
        let couple_facts = tree.spouse_links_of(&p.id).into_iter().flat_map(|l| l.facts.iter());
        PersonForm {
            person_id: p.id.clone(),
            given: name.map(|n| n.given.clone()).unwrap_or_default(),
            surname: name.map(|n| n.surname.clone()).unwrap_or_default(),
            note: first_note(&p.notes),
            facts: p.facts.iter().chain(couple_facts).map(FactForm::new).collect(),
            names: p
                .names
                .iter()
                .filter(|n| Some(n.id.as_str()) != name.map(|n| n.id.as_str()))
                .map(|n| NameForm {
                    id: n.id.clone(),
                    given: n.given.clone(),
                    surname: n.surname.clone(),
                    name_type: n.name_type.clone(),
                })
                .collect(),
            custom_event: String::new(),
        }
    }

    pub fn person_id(&self) -> &str {
        &self.person_id
    }

    fn fact(&mut self, id: &str) -> Option<&mut FactForm> {
        self.facts.iter_mut().find(|f| f.id == id)
    }

    /// Differences between the form and the tree (invalid dates skipped).
    pub fn pending_edits(&self, tree: &Tree) -> Vec<FormEdit> {
        let Some(p) = tree.person(&self.person_id) else { return Vec::new() };
        let mut edits = Vec::new();
        let name = p.primary_name();
        if name.map(|n| (n.given.as_str(), n.surname.as_str())) != Some((self.given.trim(), self.surname.trim()))
            && !(name.is_none() && self.given.trim().is_empty() && self.surname.trim().is_empty())
        {
            edits.push(FormEdit::Rename(self.given.trim().into(), self.surname.trim().into()));
        }
        if first_note(&p.notes) != self.note.trim() {
            edits.push(FormEdit::PersonNote(self.note.trim().into()));
        }
        let all_facts: Vec<&Fact> =
            p.facts.iter().chain(tree.spouse_links_of(&p.id).into_iter().flat_map(|l| l.facts.iter())).collect();
        for form in &self.facts {
            let Some(fact) = all_facts.iter().find(|f| f.id == form.id) else { continue };
            if let Ok(input) = form.input() {
                if fact.date != input.date || fact.place != input.place || fact.details != input.details {
                    edits.push(FormEdit::Fact(form.id.clone(), input));
                }
            }
            if first_note(&fact.notes) != form.note.trim() {
                edits.push(FormEdit::FactNote(form.id.clone(), form.note.trim().into()));
            }
        }
        for form in &self.names {
            let Some(n) = p.names.iter().find(|n| n.id == form.id) else { continue };
            if (n.given.as_str(), n.surname.as_str(), n.name_type.as_str())
                != (form.given.trim(), form.surname.trim(), form.name_type.as_str())
            {
                edits.push(FormEdit::Name(
                    form.id.clone(),
                    form.given.trim().into(),
                    form.surname.trim().into(),
                    form.name_type.clone(),
                ));
            }
        }
        edits
    }
}

/// A relationship removal awaiting confirmation.
#[derive(Clone)]
pub enum Unlink {
    Parent { child: Id, parent: Id },
    Couple { a: Id, b: Id },
}

/// Structural actions (applied after drawing).
enum Action {
    Gender(Gender),
    Living(bool),
    HomePerson,
    DeleteFact(Id, String),
    AddFact(String),
    AddCustomFact(String),
    AddCoupleFact(Id, &'static str),
    CoupleStatus(Id, String),
    ParentKind(Id, String),
    AddName,
    RemoveName(Id),
    MoveChild(Id, i32),
    MoveSpouse(Id, i32),
    Unlink(Unlink),
    AddRelation(Relation, Option<Id>),
    AddPhoto,
    MainPhoto(Id),
    RemovePhoto(Id),
    Goto(Id),
    Delete,
}

fn wide_text(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    ui.add(egui::TextEdit::singleline(text).hint_text(hint).desired_width(f32::INFINITY))
}

/// Editor for one event; returns (focus lost, remove clicked).
fn fact_editor(ui: &mut egui::Ui, label: &str, form: &mut FactForm) -> (bool, bool) {
    let mut lost = false;
    let mut delete = false;
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(label).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                delete = ui.button("🗑 Remove").on_hover_text("Remove this event (you can undo)").clicked();
            });
        });
        let parsed = Date::parse(&form.date);
        let r = wide_text(ui, &mut form.date, "date, e.g. 12 Mar 1950, abt 1900, 1900-1910");
        lost |= r.lost_focus();
        match &parsed {
            Err(e) => {
                ui.colored_label(Color32::from_rgb(0xff, 0x80, 0x80), format!("⚠ {e} — not saved yet"));
            }
            Ok(d) if r.has_focus() && !d.is_empty() => {
                ui.weak(format!("Understood as: {}", d.display()));
            }
            _ => {}
        }
        lost |= wide_text(ui, &mut form.place, "place").lost_focus();
        lost |= wide_text(ui, &mut form.details, "description (e.g. occupation)").lost_focus();
        lost |= ui
            .add(
                egui::TextEdit::multiline(&mut form.note)
                    .hint_text("notes")
                    .desired_rows(1)
                    .desired_width(f32::INFINITY),
            )
            .lost_focus();
    });
    (lost, delete)
}

impl App {
    pub(crate) fn apply_form_edit(&mut self, person: &str, e: FormEdit) {
        match e {
            FormEdit::Rename(g, s) => self.apply("Rename", |d| edit::set_name(d, person, &g, &s)),
            FormEdit::PersonNote(t) => self.apply("Edit notes", |d| edit::set_person_note(d, person, &t)),
            FormEdit::Fact(id, input) => self.apply("Edit event", |d| edit::update_fact(d, &id, &input)),
            FormEdit::FactNote(id, t) => self.apply("Edit event notes", |d| edit::set_fact_note(d, &id, &t)),
            FormEdit::Name(id, g, s, t) => self.apply("Edit name", |d| edit::update_name(d, person, &id, &g, &s, &t)),
        };
    }

    pub(crate) fn person_panel(&mut self, ui: &mut egui::Ui) {
        let Some(person) = self.selected.as_deref().and_then(|id| self.tree.person(id)).cloned() else {
            ui.weak("No person selected");
            return;
        };
        if self.form.as_ref().is_none_or(|f| f.person_id != person.id) {
            self.form = Some(PersonForm::new(&person, &self.tree));
        }
        let mut form = self.form.take().expect("set above");
        let mut action = None;
        let mut lost_focus = false;
        let ctx = ui.ctx().clone();

        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            // header: photo, name, life span
            ui.horizontal(|ui| {
                if let Some(pkg) = &self.pkg {
                    if let Some((tex, uv)) = self.photos.face(&ctx, pkg, &self.tree, &person) {
                        ui.add(egui::Image::new((tex, egui::vec2(96.0, 96.0))).uv(uv).corner_radius(8));
                    }
                }
                ui.vertical(|ui| {
                    ui.heading(person.display_name());
                    ui.label(format!("{} {}", gender_symbol(person.gender), person.lifespan()));
                    if self.tree.home_person.as_deref() == Some(person.id.as_str()) {
                        ui.weak("⌂ Home person (opens first)");
                    } else if ui
                        .small_button("⌂ Make home person")
                        .on_hover_text("Open the tree at this person")
                        .clicked()
                    {
                        action = Some(Action::HomePerson);
                    }
                });
            });

            egui::CollapsingHeader::new(RichText::new("Details").heading()).default_open(true).show(ui, |ui| {
                egui::Grid::new("names").num_columns(2).show(ui, |ui| {
                    ui.label("First names");
                    lost_focus |= wide_text(ui, &mut form.given, "").lost_focus();
                    ui.end_row();
                    ui.label("Surname");
                    lost_focus |= wide_text(ui, &mut form.surname, "").lost_focus();
                    ui.end_row();
                    ui.label("Gender");
                    ui.horizontal_wrapped(|ui| {
                        let mut gender = person.gender;
                        for (g, label) in [
                            (Gender::Male, "Male"),
                            (Gender::Female, "Female"),
                            (Gender::Other, "Other"),
                            (Gender::Unknown, "Unknown"),
                        ] {
                            ui.radio_value(&mut gender, g, label);
                        }
                        if gender != person.gender {
                            action = Some(Action::Gender(gender));
                        }
                    });
                    ui.end_row();
                    ui.label("");
                    let mut living = person.is_living;
                    if ui
                        .checkbox(&mut living, "Still living")
                        .on_hover_text("Mark people who are alive today")
                        .changed()
                    {
                        action = Some(Action::Living(living));
                    }
                    ui.end_row();
                });
            });

            egui::CollapsingHeader::new(RichText::new("Other names").heading())
                .default_open(!form.names.is_empty())
                .show(ui, |ui| {
                    for n in &mut form.names {
                        ui.horizontal(|ui| {
                            let w = ((ui.available_width() - 190.0) / 2.0).max(60.0);
                            lost_focus |= ui
                                .add(egui::TextEdit::singleline(&mut n.given).hint_text("first names").desired_width(w))
                                .lost_focus();
                            lost_focus |= ui
                                .add(egui::TextEdit::singleline(&mut n.surname).hint_text("surname").desired_width(w))
                                .lost_focus();
                            egui::ComboBox::from_id_salt(("name_type", &n.id))
                                .width(90.0)
                                .selected_text(if n.name_type.is_empty() { "type" } else { n.name_type.as_str() })
                                .show_ui(ui, |ui| {
                                    for t in NAME_TYPES {
                                        if ui.selectable_value(&mut n.name_type, t.to_string(), *t).clicked() {
                                            lost_focus = true;
                                        }
                                    }
                                });
                            if ui.button("Remove").clicked() {
                                action = Some(Action::RemoveName(n.id.clone()));
                            }
                        });
                    }
                    if ui.button("+ Add another name (birth, married, …)").clicked() {
                        action = Some(Action::AddName);
                    }
                });

            egui::CollapsingHeader::new(RichText::new("Events").heading()).default_open(true).show(ui, |ui| {
                for fact in &person.facts {
                    let Some(f) = form.fact(&fact.id) else { continue };
                    let (lost, delete) = fact_editor(ui, fact.label(), f);
                    lost_focus |= lost;
                    if delete {
                        action = Some(Action::DeleteFact(fact.id.clone(), fact.label().to_string()));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("add_event").selected_text("+ Add event").show_ui(ui, |ui| {
                        for tag in PERSON_EVENTS {
                            if ui.selectable_label(false, fact_label(tag)).clicked() {
                                action = Some(Action::AddFact(tag.to_string()));
                            }
                        }
                    });
                    ui.add(
                        egui::TextEdit::singleline(&mut form.custom_event)
                            .hint_text("or a custom event name")
                            .desired_width(150.0),
                    );
                    if ui.add_enabled(!form.custom_event.trim().is_empty(), egui::Button::new("Add")).clicked() {
                        action = Some(Action::AddCustomFact(form.custom_event.trim().to_string()));
                    }
                });
            });

            egui::CollapsingHeader::new(RichText::new("Notes").heading()).default_open(!form.note.is_empty()).show(
                ui,
                |ui| {
                    if person.notes.first().is_some_and(|n| n.is_rich()) {
                        ui.weak(
                            "This note has formatting from the original app; editing it here keeps the text but \
                             removes the formatting.",
                        );
                    }
                    lost_focus |= ui
                        .add(
                            egui::TextEdit::multiline(&mut form.note)
                                .hint_text("Stories, sources, anything worth remembering")
                                .desired_rows(4)
                                .desired_width(f32::INFINITY),
                        )
                        .lost_focus();
                },
            );

            egui::CollapsingHeader::new(RichText::new("Photos").heading()).default_open(!person.media.is_empty()).show(
                ui,
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for media_id in &person.media {
                            let (Some(pkg), Some(media)) = (&self.pkg, self.tree.media.get(media_id)) else { continue };
                            ui.vertical(|ui| {
                                if let Some(tex) = self.photos.get(&ctx, pkg, &media.part) {
                                    let main = person.primary_image.as_deref() == Some(media_id.as_str());
                                    let img = egui::Image::new(&tex)
                                        .fit_to_exact_size(egui::vec2(110.0, 110.0))
                                        .maintain_aspect_ratio(true);
                                    let r = ui.add(egui::Button::image(img).selected(main)).on_hover_text(if main {
                                        "Main photo"
                                    } else {
                                        "Click to make this the main photo"
                                    });
                                    if r.clicked() && !main {
                                        action = Some(Action::MainPhoto(media_id.clone()));
                                    }
                                }
                                if ui.small_button("Remove").clicked() {
                                    action = Some(Action::RemovePhoto(media_id.clone()));
                                }
                            });
                        }
                    });
                    if ui.button("+ Add photo…").clicked() {
                        action = Some(Action::AddPhoto);
                    }
                },
            );

            egui::CollapsingHeader::new(RichText::new("Family").heading()).default_open(true).show(ui, |ui| {
                self.family_section(ui, &person, &mut form, &mut action, &mut lost_focus);
            });

            ui.add_space(20.0);
            ui.separator();
            let delete = egui::Button::new(RichText::new("Delete this person…").color(Color32::WHITE))
                .fill(Color32::from_rgb(0x9a, 0x2a, 0x2a));
            if ui.add(delete).clicked() {
                action = Some(Action::Delete);
            }
        });
        self.form = Some(form);

        if lost_focus || action.is_some() {
            self.flush_form();
        }
        if let Some(action) = action {
            self.run_action(&person, action);
        }
    }

    fn family_section(
        &mut self,
        ui: &mut egui::Ui,
        person: &Person,
        form: &mut PersonForm,
        action: &mut Option<Action>,
        lost_focus: &mut bool,
    ) {
        let tree = &self.tree;
        let link = |ui: &mut egui::Ui, id: &str, action: &mut Option<Action>| {
            if let Some(p) = tree.person(id) {
                if ui
                    .link(format!("{}  {}", p.display_name(), p.lifespan()))
                    .on_hover_text("Show this person's family")
                    .clicked()
                {
                    *action = Some(Action::Goto(id.to_string()));
                }
            }
        };

        ui.label(RichText::new("Parents").strong());
        for parent in tree.parents(&person.id) {
            ui.horizontal(|ui| {
                link(ui, parent, action);
                let current = tree
                    .parent_links
                    .iter()
                    .find(|l| l.parent == parent && l.child == person.id)
                    .map(|l| l.modifier.as_str())
                    .unwrap_or("");
                let current = if current == "Natural" { "" } else { current };
                let label = PARENT_KINDS.iter().find(|(v, _)| *v == current).map_or(current, |(_, l)| l);
                egui::ComboBox::from_id_salt(("kind", parent)).width(90.0).selected_text(label).show_ui(ui, |ui| {
                    for (value, label) in PARENT_KINDS {
                        if ui.selectable_label(*value == current, *label).clicked() && *value != current {
                            *action = Some(Action::ParentKind(parent.to_string(), value.to_string()));
                        }
                    }
                });
                if ui
                    .button("Remove…")
                    .on_hover_text("Remove this parent link (both people stay in the tree)")
                    .clicked()
                {
                    *action =
                        Some(Action::Unlink(Unlink::Parent { child: person.id.clone(), parent: parent.to_string() }));
                }
            });
        }
        if ui.button("+ Add parent").clicked() {
            *action = Some(Action::AddRelation(Relation::Parent, None));
        }

        ui.add_space(8.0);
        ui.label(RichText::new("Partners").strong());
        let links = tree.spouse_links_of(&person.id);
        let count = links.len();
        for (i, couple) in links.into_iter().enumerate() {
            let other = couple.other(&person.id).to_string();
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    link(ui, &other, action);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("Remove…")
                            .on_hover_text("Remove this partnership (both people stay in the tree)")
                            .clicked()
                        {
                            *action = Some(Action::Unlink(Unlink::Couple { a: person.id.clone(), b: other.clone() }));
                        }
                        if count > 1 {
                            if ui
                                .add_enabled(i + 1 < count, egui::Button::new("↓"))
                                .on_hover_text("Move down")
                                .clicked()
                            {
                                *action = Some(Action::MoveSpouse(other.clone(), 1));
                            }
                            if ui.add_enabled(i > 0, egui::Button::new("↑")).on_hover_text("Move up").clicked() {
                                *action = Some(Action::MoveSpouse(other.clone(), -1));
                            }
                        }
                    });
                });
                ui.horizontal(|ui| {
                    ui.label("Status");
                    let label = COUPLE_STATUS
                        .iter()
                        .find(|(v, _)| *v == couple.status)
                        .map_or(couple.status.as_str(), |(_, l)| l);
                    egui::ComboBox::from_id_salt(("status", &couple.id)).selected_text(label).show_ui(ui, |ui| {
                        for (value, label) in COUPLE_STATUS {
                            if ui.selectable_label(couple.status == *value, *label).clicked() && couple.status != *value
                            {
                                *action = Some(Action::CoupleStatus(couple.id.clone(), value.to_string()));
                            }
                        }
                    });
                });
                for fact in &couple.facts {
                    let Some(f) = form.fact(&fact.id) else { continue };
                    let (lost, delete) = fact_editor(ui, fact.label(), f);
                    *lost_focus |= lost;
                    if delete {
                        *action = Some(Action::DeleteFact(fact.id.clone(), fact.label().to_string()));
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt(("couple_event", &couple.id)).selected_text("+ Add event").show_ui(
                        ui,
                        |ui| {
                            for tag in COUPLE_EVENTS {
                                if ui.selectable_label(false, fact_label(tag)).clicked() {
                                    *action = Some(Action::AddCoupleFact(couple.id.clone(), tag));
                                }
                            }
                        },
                    );
                    let name =
                        tree.person(&other).and_then(|p| p.primary_name()).map(|n| n.given.clone()).unwrap_or_default();
                    if ui.button(format!("+ Add child with {name}")).clicked() {
                        *action = Some(Action::AddRelation(Relation::Child, Some(other.clone())));
                    }
                });
            });
        }
        if ui.button("+ Add partner").clicked() {
            *action = Some(Action::AddRelation(Relation::Spouse, None));
        }

        ui.add_space(8.0);
        ui.label(RichText::new("Children").strong());
        let kids = tree.children(&person.id);
        let n = kids.len();
        for (i, kid) in kids.into_iter().enumerate() {
            ui.horizontal(|ui| {
                link(ui, kid, action);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("Remove…")
                        .on_hover_text("Remove this child link (both people stay in the tree)")
                        .clicked()
                    {
                        *action =
                            Some(Action::Unlink(Unlink::Parent { child: kid.to_string(), parent: person.id.clone() }));
                    }
                    if n > 1 {
                        if ui.add_enabled(i + 1 < n, egui::Button::new("↓")).on_hover_text("Move down").clicked() {
                            *action = Some(Action::MoveChild(kid.to_string(), 1));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("↑")).on_hover_text("Move up").clicked() {
                            *action = Some(Action::MoveChild(kid.to_string(), -1));
                        }
                    }
                });
            });
        }
        if ui.button("+ Add child").clicked() {
            *action = Some(Action::AddRelation(Relation::Child, None));
        }
    }

    fn run_action(&mut self, person: &Person, action: Action) {
        let pid = person.id.clone();
        let name = person.display_name();
        let structural =
            !matches!(action, Action::Goto(_) | Action::Delete | Action::Unlink(_) | Action::AddRelation(..));
        match action {
            Action::Gender(g) => {
                self.apply("Change gender", |d| edit::set_gender(d, &pid, g));
            }
            Action::Living(l) => {
                self.apply("Change living status", |d| edit::set_living(d, &pid, l));
            }
            Action::HomePerson => {
                self.apply("Set home person", |d| edit::set_home_person(d, &pid));
            }
            Action::DeleteFact(id, label) => {
                if self.apply("Remove event", |d| edit::delete_fact(d, &id)).is_some() {
                    self.toasts.undoable(format!("Removed {} from {name}", label.to_lowercase()));
                }
            }
            Action::AddFact(tag) => {
                self.apply("Add event", |d| edit::add_fact(d, &pid, &tag, &FactInput::default()));
            }
            Action::AddCustomFact(label) => {
                self.apply("Add event", |d| edit::add_custom_fact(d, &pid, &label, &FactInput::default()));
            }
            Action::AddCoupleFact(link, tag) => {
                self.apply("Add event", |d| edit::add_couple_fact(d, &link, tag, &FactInput::default()));
            }
            Action::CoupleStatus(link, status) => {
                self.apply("Change partnership status", |d| edit::set_couple_status(d, &link, &status));
            }
            Action::ParentKind(parent, kind) => {
                self.apply("Change parent type", |d| edit::set_parent_modifier(d, &pid, &parent, &kind));
            }
            Action::AddName => {
                self.apply("Add name", |d| edit::add_alternate_name(d, &pid, "", "", ""));
            }
            Action::RemoveName(id) => {
                if self.apply("Remove name", |d| edit::remove_name(d, &pid, &id)).is_some() {
                    self.toasts.undoable("Removed a name");
                }
            }
            Action::MoveChild(child, delta) => {
                self.apply("Reorder children", |d| edit::move_child(d, &pid, &child, delta));
            }
            Action::MoveSpouse(other, delta) => {
                self.apply("Reorder partners", |d| edit::move_spouse(d, &pid, &other, delta));
            }
            Action::Unlink(u) => self.confirm_unlink = Some(u),
            Action::AddRelation(kind, co_parent) => {
                let mut dlg = RelationDialog::new(kind, &pid, &self.tree);
                if co_parent.is_some() {
                    dlg.set_co_parent(co_parent);
                }
                self.relation_dialog = Some(dlg);
            }
            Action::AddPhoto => self.add_photo(&pid),
            Action::MainPhoto(media) => {
                self.apply("Change main photo", |d| edit::set_primary_photo(d, &pid, &media));
            }
            Action::RemovePhoto(media) => {
                if self.apply_pkg("Remove photo", |p| p.remove_photo(&pid, &media)).is_some() {
                    self.toasts.info("Photo removed");
                }
            }
            Action::Goto(id) => self.select(&id),
            Action::Delete => self.confirm_delete = Some(pid),
        }
        if structural {
            self.form = None;
        }
    }

    fn add_photo(&mut self, person: &str) {
        let Some(path) = rfd::FileDialog::new().add_filter("Images", crate::photos::PICK_EXTENSIONS).pick_file() else {
            return;
        };
        let ext = path.extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_default();
        let file_name = crate::app::file_name(&path);
        let prepared =
            std::fs::read(&path).map_err(|e| e.to_string()).and_then(|b| crate::photos::prepare_for_tree(b, &ext));
        match prepared {
            Ok((bytes, ext)) => {
                let name = if file_name.ends_with(ext) { file_name } else { format!("{file_name}.{ext}") };
                self.apply_pkg("Add photo", |p| p.add_photo(person, bytes, ext, &name));
            }
            Err(e) => self.toasts.error(format!("Could not add the photo: {e}")),
        }
    }

    pub(crate) fn delete_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(id) = self.confirm_delete.clone() else { return };
        let name = self.tree.person(&id).map(|p| p.display_name()).unwrap_or_default();
        let mut close = false;
        let mut delete = false;
        egui::Modal::new(egui::Id::new("confirm_delete")).show(ctx, |ui| {
            ui.heading(format!("Delete {name}?"));
            ui.label("This removes the person and all their family links. You can undo it with Ctrl+Z.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                close = ui.button(RichText::new("Keep").strong()).clicked();
                delete = ui.button(RichText::new("Delete").color(Color32::from_rgb(0xff, 0x80, 0x80))).clicked();
            });
        });
        if delete && self.apply("Delete person", |d| edit::delete_person(d, &id)).is_some() {
            self.toasts.undoable(format!("Deleted {name}"));
        }
        if delete || close {
            self.confirm_delete = None;
        }
    }

    pub(crate) fn unlink_dialog_ui(&mut self, ctx: &egui::Context) {
        let Some(unlink) = self.confirm_unlink.clone() else { return };
        let name = |id: &str| self.tree.person(id).map(|p| p.display_name()).unwrap_or_default();
        let question = match &unlink {
            Unlink::Parent { child, parent } => format!("Remove {} as a parent of {}?", name(parent), name(child)),
            Unlink::Couple { a, b } => format!("Remove the partnership between {} and {}?", name(a), name(b)),
        };
        let (mut close, mut remove) = (false, false);
        egui::Modal::new(egui::Id::new("confirm_unlink")).show(ctx, |ui| {
            ui.heading(&question);
            ui.label("Both people stay in the tree; only the link between them is removed.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                close = ui.button(RichText::new("Keep").strong()).clicked();
                remove = ui.button("Remove link").clicked();
            });
        });
        if remove {
            let done = match &unlink {
                Unlink::Parent { child, parent } => {
                    self.apply("Remove parent link", |d| edit::unlink_parent(d, child, parent))
                }
                Unlink::Couple { a, b } => self.apply("Remove partnership", |d| edit::unlink_spouse(d, a, b)),
            };
            if done.is_some() {
                self.form = None;
                self.toasts.undoable("Link removed");
            }
        }
        if remove || close {
            self.confirm_unlink = None;
        }
    }
}

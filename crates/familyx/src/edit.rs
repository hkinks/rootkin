//! Edit operations on a family [`Document`].
//!
//! Each operation keeps the redundant link records in sync (relationship
//! links, per-person back-references) and inserts new elements in the
//! order .NET `XmlSerializer` writes them. Anything not touched is preserved.

use base64::Engine;
use rand::RngCore;

use crate::doc::{Document, Element};
use crate::model::{Date, Gender, Id};

// Child element order per type, derived from files the app writes.
const PERSON_ORDER: &[&str] = &[
    "PersonNameCollection",
    "Biology",
    "FamilyChildLinks",
    "FamilySpouseLinks",
    "PrimaryImageId",
    "Face",
    "IsLiving",
    "Facts",
    "Multimedia",
    "Notes",
];
const NAME_ORDER: &[&str] =
    &["Names", "SurnamePrefix", "Surname", "Suffix", "Nickname", "Notes", "IsPrimary", "NameType"];
const FACT_ORDER: &[&str] = &[
    "Tag",
    "CustomFactTag",
    "Details",
    "ReligiousAffiliation",
    "IsPrimary",
    "FactDatePeriod",
    "IsFamily",
    "FactDate",
    "SortDate",
    "PlaceDetails",
    "FullAddress",
    "AgeAtEvent",
    "Ages",
    "Multimedia",
    "Notes",
];
// CustomDate property order (the subset we write); see docs/design.md
const FACT_DATE_ORDER: &[&str] =
    &["StartDay", "StartMonth", "StartYear", "EndDay", "EndMonth", "EndYear", "DateDescriptor", "StartTime", "Text"];
const PLACE_ORDER: &[&str] = &["Description", "Longitude", "Latitude", "BoundingBox"];
const FAMILY_ORDER: &[&str] = &[
    "AuthorCollection",
    "PeopleCollection",
    "SourceCollection",
    "RepositoryCollection",
    "PackagedFileCollection",
    "TaskCollection",
    "RelationshipCollection",
    "TagCollection",
    "FileProperties",
    "FamilyProperties",
];

/// A new id in the app's format: a random GUID as 22-char unpadded base64url.
pub fn new_id() -> Id {
    let mut bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

/// UTC timestamp like the app's `Creation` attributes: `2025-12-16T17:35:24.6905393Z`.
pub(crate) fn utc_now() -> String {
    let now = chrono::Utc::now();
    format!("{}.{:07}Z", now.format("%Y-%m-%dT%H:%M:%S"), now.timestamp_subsec_nanos() / 100)
}

/// Local timestamp like the app's `LastSaved` attributes: `2021-08-06T00:45:45.0180957+03:00`.
pub(crate) fn local_now() -> String {
    let now = chrono::Local::now();
    format!("{}.{:07}{}", now.format("%Y-%m-%dT%H:%M:%S"), now.timestamp_subsec_nanos() / 100, now.format("%:z"))
}

pub(crate) fn record(name: &str) -> Element {
    Element::new(name).with_attr("Id", &new_id()).with_attr("Creation", &utc_now())
}

pub(crate) fn text_el(name: &str, value: &str) -> Element {
    let mut el = Element::new(name);
    el.set_text(value);
    el
}

/// Set a child's text, creating it in schema order; an empty value removes it.
fn set_child_text(parent: &mut Element, name: &str, value: &str, order: &[&str]) {
    if value.is_empty() {
        parent.remove_children_where(|e| e.name == name);
    } else {
        parent.ensure_child(name, order).set_text(value);
    }
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum EditError {
    #[error("no person with id {0}")]
    NoPerson(Id),
    #[error("no fact with id {0}")]
    NoFact(Id),
    #[error("{0}")]
    Invalid(&'static str),
}

pub type EditResult<T> = std::result::Result<T, EditError>;

fn collection<'a>(doc: &'a mut Document, name: &str) -> &'a mut Element {
    doc.root.ensure_child(name, FAMILY_ORDER)
}

fn person_mut<'a>(doc: &'a mut Document, id: &str) -> EditResult<&'a mut Element> {
    collection(doc, "PeopleCollection")
        .elements_mut()
        .find(|p| p.name == "Person" && p.attr("Id").as_deref() == Some(id))
        .ok_or_else(|| EditError::NoPerson(id.to_string()))
}

fn touch(person: &mut Element) {
    if person.attr("LastSaved").is_some() {
        person.set_attr("LastSaved", &local_now());
    }
}

fn person_exists(doc: &Document, id: &str) -> bool {
    doc.root
        .child("PeopleCollection")
        .is_some_and(|c| c.children_named("Person").any(|p| p.attr("Id").as_deref() == Some(id)))
}

// ---------------------------------------------------------------- document

/// An empty family document in the shape the app writes.
pub fn new_family_document() -> Document {
    // Hash of the app's built-in default password under a random salt: the
    // app treats any other FamilyId as "password protected" (see `protection`).
    let family_id = crate::protection::default_family_id();
    let now = utc_now();
    let xml = format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><Family \
         xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" \
         FamilyId=\"{family_id}\"><AuthorCollection><Author Id=\"{author}\" LastSaved=\"{local}\" Creation=\"{now}\">\
         <Name /><IsPrimary>true</IsPrimary></Author></AuthorCollection><PeopleCollection /><SourceCollection />\
         <RepositoryCollection /><PackagedFileCollection /><TaskCollection /><RelationshipCollection />\
         <TagCollection /><FileProperties Id=\"{fp}\" Creation=\"{now}\"><Version>14.8</Version>\
         <LastSavedDate>{now}</LastSavedDate><CreationDate>{now}</CreationDate></FileProperties>\
         <FamilyProperties Id=\"{fam}\" Creation=\"{now}\" /></Family>",
        family_id = crate::doc::escape(&family_id, true),
        author = new_id(),
        local = local_now(),
        fp = new_id(),
        fam = new_id(),
    );
    Document::parse(xml.as_bytes()).expect("static skeleton is valid XML")
}

pub fn set_home_person(doc: &mut Document, person_id: &str) -> EditResult<()> {
    if !person_exists(doc, person_id) {
        return Err(EditError::NoPerson(person_id.to_string()));
    }
    let props = doc.root.ensure_child("FamilyProperties", FAMILY_ORDER);
    props.ensure_child("HomePersonId", &["LastSavedPersonId", "HomePersonId"]).set_text(person_id);
    Ok(())
}

// ---------------------------------------------------------------- people

/// Add a person with a primary name; returns the new id.
pub fn add_person(doc: &mut Document, given: &str, surname: &str, gender: Gender) -> Id {
    let id = new_id();
    let mut person = Element::new("Person")
        .with_attr("Id", &id)
        .with_attr("LastSaved", &local_now())
        .with_attr("Creation", &utc_now());
    let mut name = record("Name");
    name.push(text_el("Names", given));
    name.push(Element::new("SurnamePrefix"));
    name.push(text_el("Surname", surname));
    name.push(Element::new("Suffix"));
    name.push(Element::new("Nickname"));
    name.push(text_el("IsPrimary", "true"));
    person.ensure_child("PersonNameCollection", PERSON_ORDER).ensure_child("Names", &[]).push(name);
    collection(doc, "PeopleCollection").push(person);
    // set_gender cannot fail: the person was just added
    let _ = set_gender(doc, &id, gender);
    id
}

pub fn set_name(doc: &mut Document, person_id: &str, given: &str, surname: &str) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    let names = person.ensure_child("PersonNameCollection", PERSON_ORDER).ensure_child("Names", &[]);
    let is_real = |n: &Element| n.child("Names").is_some() || n.child("Surname").is_some();
    let idx = names
        .elements()
        .position(|n| n.child_text("IsPrimary").as_deref() == Some("true"))
        .or_else(|| names.elements().position(is_real));
    let name = match idx {
        Some(i) => names.elements_mut().nth(i).expect("index from same iterator"),
        None => {
            let mut n = record("Name");
            n.push(text_el("IsPrimary", "true"));
            // keep the app's trailing empty <Name/> placeholder last
            let at =
                names.children.len().saturating_sub(usize::from(names.elements().last().is_some_and(|n| !is_real(n))));
            names.children.insert(at, crate::doc::Node::Element(n));
            match &mut names.children[at] {
                crate::doc::Node::Element(e) => e,
                _ => unreachable!(),
            }
        }
    };
    name.ensure_child("Names", NAME_ORDER).set_text(given);
    name.ensure_child("Surname", NAME_ORDER).set_text(surname);
    touch(person_mut(doc, person_id)?);
    Ok(())
}

/// Add a non-primary name, e.g. a birth or married name (`name_type` like `Married`).
pub fn add_alternate_name(
    doc: &mut Document,
    person_id: &str,
    given: &str,
    surname: &str,
    name_type: &str,
) -> EditResult<Id> {
    let person = person_mut(doc, person_id)?;
    let mut name = record("Name");
    let id = name.attr("Id").unwrap_or_default().into_owned();
    name.push(text_el("Names", given));
    name.push(Element::new("SurnamePrefix"));
    name.push(text_el("Surname", surname));
    name.push(Element::new("Suffix"));
    name.push(Element::new("Nickname"));
    if !name_type.is_empty() {
        name.push(text_el("NameType", name_type));
    }
    person.ensure_child("PersonNameCollection", PERSON_ORDER).ensure_child("Names", &[]).push(name);
    touch(person);
    Ok(id)
}

pub fn set_gender(doc: &mut Document, person_id: &str, gender: Gender) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    match gender {
        Gender::Unknown => {
            if let Some(b) = person.child_mut("Biology") {
                b.remove_children_where(|e| e.name == "Gender");
            }
        }
        g => {
            let biology = ensure_record(person, "Biology", PERSON_ORDER);
            let value = match g {
                Gender::Male => "Male",
                Gender::Female => "Female",
                _ => "Other",
            };
            biology.ensure_child("Gender", &[]).set_text(value);
        }
    }
    touch(person);
    Ok(())
}

pub fn set_living(doc: &mut Document, person_id: &str, living: bool) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    set_child_text(person, "IsLiving", if living { "true" } else { "" }, PERSON_ORDER);
    touch(person);
    Ok(())
}

/// Remove a person and every relationship record that refers to them.
pub fn delete_person(doc: &mut Document, person_id: &str) -> EditResult<()> {
    if !person_exists(doc, person_id) {
        return Err(EditError::NoPerson(person_id.to_string()));
    }
    let links: Vec<(Id, String)> = relationship_links(doc)
        .filter(|l| {
            l.attr("Person1Id").as_deref() == Some(person_id) || l.attr("Person2Id").as_deref() == Some(person_id)
        })
        .map(|l| (l.attr("Id").unwrap_or_default().into_owned(), l.attr("xsi:type").unwrap_or_default().into_owned()))
        .collect();
    for (link_id, kind) in links {
        if kind == "SpouseLink" {
            remove_spouse_link(doc, &link_id);
        } else {
            remove_relationship(doc, &link_id);
        }
    }
    collection(doc, "PeopleCollection")
        .remove_children_where(|p| p.name == "Person" && p.attr("Id").as_deref() == Some(person_id));
    if let Some(props) = doc.root.child_mut("FamilyProperties") {
        for field in ["HomePersonId", "LastSavedPersonId"] {
            if props.child_text(field).as_deref() == Some(person_id) {
                props.remove_children_where(|e| e.name == field);
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- facts

/// Fields of a fact as edited in the UI.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FactInput {
    pub date: Date,
    pub place: String,
    pub details: String,
}

/// Locate a fact element by id on any person or spouse link.
fn fact_mut<'a>(doc: &'a mut Document, fact_id: &str) -> EditResult<&'a mut Element> {
    fn contains(el: &Element, id: &str) -> bool {
        (el.name == "Fact" && el.attr("Id").as_deref() == Some(id)) || el.elements().any(|c| contains(c, id))
    }
    fn find<'b>(el: &'b mut Element, id: &str) -> Option<&'b mut Element> {
        if el.name == "Fact" && el.attr("Id").as_deref() == Some(id) {
            return Some(el);
        }
        el.elements_mut().find_map(|c| find(c, id))
    }
    let owner = ["PeopleCollection", "RelationshipCollection"]
        .into_iter()
        .find(|c| doc.root.child(c).is_some_and(|c| contains(c, fact_id)))
        .ok_or_else(|| EditError::NoFact(fact_id.to_string()))?;
    find(doc.root.child_mut(owner).expect("found above"), fact_id).ok_or_else(|| EditError::NoFact(fact_id.to_string()))
}

/// Add a fact (e.g. `BIRT`) to a person; returns its id.
pub fn add_fact(doc: &mut Document, person_id: &str, tag: &str, input: &FactInput) -> EditResult<Id> {
    let person = person_mut(doc, person_id)?;
    let is_first = !person
        .child("Facts")
        .is_some_and(|f| f.children_named("Fact").any(|f| f.child_text("Tag").as_deref() == Some(tag)));
    let mut fact = record("Fact");
    let id = fact.attr("Id").unwrap_or_default().into_owned();
    fact.push(text_el("Tag", tag));
    if is_first {
        fact.push(text_el("IsPrimary", "true"));
    }
    write_fact_fields(&mut fact, input);
    person.ensure_child("Facts", PERSON_ORDER).push(fact);
    touch(person);
    Ok(id)
}

/// Add a fact (e.g. `MARR`) to a couple; returns its id.
pub fn add_couple_fact(doc: &mut Document, link_id: &str, tag: &str, input: &FactInput) -> EditResult<Id> {
    let link = collection(doc, "RelationshipCollection")
        .elements_mut()
        .find(|l| l.attr("Id").as_deref() == Some(link_id) && l.attr("xsi:type").as_deref() == Some("SpouseLink"))
        .ok_or(EditError::Invalid("no such couple"))?;
    let facts = link.ensure_child("Facts", &["Facts", "SpouseRelationshipStatus", "NumberOfChildren"]);
    let is_first = !facts.children_named("Fact").any(|f| f.child_text("Tag").as_deref() == Some(tag));
    let mut fact = record("Fact");
    let id = fact.attr("Id").unwrap_or_default().into_owned();
    fact.push(text_el("Tag", tag));
    if is_first {
        fact.push(text_el("IsPrimary", "true"));
    }
    write_fact_fields(&mut fact, input);
    fact.ensure_child("IsFamily", FACT_ORDER).set_text("true");
    facts.push(fact);
    Ok(id)
}

pub fn update_fact(doc: &mut Document, fact_id: &str, input: &FactInput) -> EditResult<()> {
    write_fact_fields(fact_mut(doc, fact_id)?, input);
    Ok(())
}

pub fn delete_fact(doc: &mut Document, fact_id: &str) -> EditResult<()> {
    fn remove(el: &mut Element, id: &str) -> bool {
        if el.remove_children_where(|f| f.name == "Fact" && f.attr("Id").as_deref() == Some(id)) > 0 {
            return true;
        }
        el.elements_mut().any(|c| remove(c, id))
    }
    let found = ["PeopleCollection", "RelationshipCollection"]
        .into_iter()
        .any(|c| doc.root.child_mut(c).is_some_and(|c| remove(c, fact_id)));
    found.then_some(()).ok_or_else(|| EditError::NoFact(fact_id.to_string()))
}

fn ensure_record<'a>(parent: &'a mut Element, name: &str, order: &[&str]) -> &'a mut Element {
    let fresh = parent.child(name).is_none();
    let el = parent.ensure_child(name, order);
    if fresh {
        el.attrs = record(name).attrs;
    }
    el
}

fn write_fact_fields(fact: &mut Element, input: &FactInput) {
    // FactDatePeriod is always "Span" in files the app writes; the actual
    // qualifier lives in FactDate/DateDescriptor
    let period = ensure_record(fact, "FactDatePeriod", FACT_ORDER);
    if period.child("DateDescriptor").is_none() {
        period.ensure_child("DateDescriptor", &[]).set_text("Span");
    }

    let d = &input.date;
    let date = ensure_record(fact, "FactDate", FACT_ORDER);
    let num = |v: Option<i64>| v.map(|v| v.to_string()).unwrap_or_default();
    set_child_text(date, "StartDay", &num(d.day.map(i64::from)), FACT_DATE_ORDER);
    set_child_text(date, "StartMonth", &num(d.month.map(i64::from)), FACT_DATE_ORDER);
    set_child_text(date, "StartYear", &num(d.year.map(i64::from)), FACT_DATE_ORDER);
    set_child_text(date, "EndDay", &num(d.end_day.map(i64::from)), FACT_DATE_ORDER);
    set_child_text(date, "EndMonth", &num(d.end_month.map(i64::from)), FACT_DATE_ORDER);
    set_child_text(date, "EndYear", &num(d.end_year.map(i64::from)), FACT_DATE_ORDER);
    let descriptor = if d.descriptor == "Exact" { "" } else { d.descriptor.as_str() };
    set_child_text(date, "DateDescriptor", descriptor, FACT_DATE_ORDER);
    set_child_text(date, "Text", &d.text, FACT_DATE_ORDER);
    ensure_record(fact, "SortDate", FACT_ORDER);

    if input.place.is_empty() {
        if let Some(place) = fact.child_mut("PlaceDetails") {
            place.remove_children_where(|e| e.name == "Description");
        }
    } else {
        ensure_record(fact, "PlaceDetails", FACT_ORDER).ensure_child("Description", PLACE_ORDER).set_text(&input.place);
    }
    set_child_text(fact, "Details", &input.details, FACT_ORDER);
    ensure_record(fact, "AgeAtEvent", FACT_ORDER);
}

// ---------------------------------------------------------------- relationships

fn relationship_links(doc: &Document) -> impl Iterator<Item = &Element> {
    doc.root.child("RelationshipCollection").into_iter().flat_map(|c| c.children_named("RelationshipLink"))
}

fn link_kind_between(doc: &Document, kind: &str, a: &str, b: &str, ordered: bool) -> Option<Id> {
    relationship_links(doc)
        .find(|l| {
            let p1 = l.attr("Person1Id");
            let p2 = l.attr("Person2Id");
            l.attr("xsi:type").as_deref() == Some(kind)
                && ((p1.as_deref() == Some(a) && p2.as_deref() == Some(b))
                    || (!ordered && p1.as_deref() == Some(b) && p2.as_deref() == Some(a)))
        })
        .map(|l| l.attr("Id").unwrap_or_default().into_owned())
}

fn parents_of(doc: &Document, child: &str) -> Vec<Id> {
    relationship_links(doc)
        .filter(|l| {
            l.attr("xsi:type").as_deref() == Some("ParentChildLink") && l.attr("Person2Id").as_deref() == Some(child)
        })
        .filter_map(|l| l.attr("Person1Id").map(|s| s.into_owned()))
        .collect()
}

fn children_of(doc: &Document, parent: &str) -> Vec<Id> {
    relationship_links(doc)
        .filter(|l| {
            l.attr("xsi:type").as_deref() == Some("ParentChildLink") && l.attr("Person1Id").as_deref() == Some(parent)
        })
        .filter_map(|l| l.attr("Person2Id").map(|s| s.into_owned()))
        .collect()
}

fn new_link(kind: &str, p1: &str, p2: &str) -> Element {
    let now = local_now();
    Element::new("RelationshipLink")
        .with_attr("xsi:type", kind)
        .with_attr("Id", &new_id())
        .with_attr("LastSaved", &now)
        .with_attr("Creation", &utc_now())
        .with_attr("Person1Id", p1)
        .with_attr("Person2Id", p2)
}

/// Add `FamilyChildLink` on `child` → couple link, unless present.
fn add_family_child_link(doc: &mut Document, child: &str, family: &str) -> EditResult<()> {
    let person = person_mut(doc, child)?;
    let links = person.ensure_child("FamilyChildLinks", PERSON_ORDER);
    if !links.children_named("FamilyChildLink").any(|l| l.attr("FamilyId").as_deref() == Some(family)) {
        links.push(record("FamilyChildLink").with_attr("ChildId", child).with_attr("FamilyId", family));
    }
    Ok(())
}

fn validate_pair(doc: &Document, a: &str, b: &str) -> EditResult<()> {
    if a == b {
        return Err(EditError::Invalid("a person cannot be related to themselves"));
    }
    for id in [a, b] {
        if !person_exists(doc, id) {
            return Err(EditError::NoPerson(id.to_string()));
        }
    }
    Ok(())
}

/// Make `parent` a parent of `child`. Also links the child to any couple
/// formed by `parent` and another existing parent.
pub fn link_parent(doc: &mut Document, child: &str, parent: &str) -> EditResult<()> {
    validate_pair(doc, child, parent)?;
    if link_kind_between(doc, "ParentChildLink", parent, child, true).is_some() {
        return Ok(());
    }
    if link_kind_between(doc, "ParentChildLink", child, parent, true).is_some() {
        return Err(EditError::Invalid("that person is already a child of this one"));
    }
    let other_parents = parents_of(doc, child);
    collection(doc, "RelationshipCollection").push(new_link("ParentChildLink", parent, child));
    for other in other_parents {
        if let Some(family) = link_kind_between(doc, "SpouseLink", parent, &other, false) {
            add_family_child_link(doc, child, &family)?;
        }
    }
    touch(person_mut(doc, child)?);
    Ok(())
}

/// Make `a` and `b` a couple; returns the spouse link id. Children they
/// already share are attached to the new couple.
pub fn link_spouse(doc: &mut Document, a: &str, b: &str) -> EditResult<Id> {
    validate_pair(doc, a, b)?;
    if let Some(existing) = link_kind_between(doc, "SpouseLink", a, b, false) {
        return Ok(existing);
    }
    let link = new_link("SpouseLink", a, b);
    let link_id = link.attr("Id").unwrap_or_default().into_owned();
    collection(doc, "RelationshipCollection").push(link);
    for p in [a, b] {
        let person = person_mut(doc, p)?;
        person
            .ensure_child("FamilySpouseLinks", PERSON_ORDER)
            .push(record("FamilySpouseLink").with_attr("SpouseId", p).with_attr("FamilyId", &link_id));
        touch(person);
    }
    let b_children = children_of(doc, b);
    for child in children_of(doc, a).into_iter().filter(|c| b_children.contains(c)) {
        add_family_child_link(doc, &child, &link_id)?;
    }
    Ok(link_id)
}

/// Remove the parent→child link and the child's links to couples `parent` belongs to.
pub fn unlink_parent(doc: &mut Document, child: &str, parent: &str) -> EditResult<()> {
    let Some(link) = link_kind_between(doc, "ParentChildLink", parent, child, true) else {
        return Err(EditError::Invalid("not a parent of this person"));
    };
    remove_relationship(doc, &link);
    let parent_couples: Vec<Id> = relationship_links(doc)
        .filter(|l| {
            l.attr("xsi:type").as_deref() == Some("SpouseLink")
                && (l.attr("Person1Id").as_deref() == Some(parent) || l.attr("Person2Id").as_deref() == Some(parent))
        })
        .filter_map(|l| l.attr("Id").map(|s| s.into_owned()))
        .collect();
    let person = person_mut(doc, child)?;
    if let Some(links) = person.child_mut("FamilyChildLinks") {
        links.remove_children_where(|l| l.attr("FamilyId").is_some_and(|f| parent_couples.iter().any(|c| *c == f)));
    }
    touch(person);
    Ok(())
}

/// Add an existing photo to a person's photos (primary if they have none).
pub(crate) fn link_photo(doc: &mut Document, person_id: &str, media_id: &str) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    let list = person.ensure_child("Multimedia", PERSON_ORDER);
    if !list.children_named("string").any(|s| s.text() == media_id) {
        list.push(text_el("string", media_id));
    }
    if person.child("PrimaryImageId").is_none() {
        person.ensure_child("PrimaryImageId", PERSON_ORDER).set_text(media_id);
    }
    touch(person);
    Ok(())
}

/// Register a `PackagedFile` and make it `person_id`'s primary image.
pub(crate) fn attach_photo(doc: &mut Document, person_id: &str, media_id: &str, file: Element) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    person.ensure_child("PrimaryImageId", PERSON_ORDER).set_text(media_id);
    person.ensure_child("Multimedia", PERSON_ORDER).push(text_el("string", media_id));
    touch(person);
    collection(doc, "PackagedFileCollection").push(file);
    Ok(())
}

/// Mark a parent link, e.g. `Adopted`; empty clears it (biological).
pub fn set_parent_modifier(doc: &mut Document, child: &str, parent: &str, modifier: &str) -> EditResult<()> {
    let link_id = link_kind_between(doc, "ParentChildLink", parent, child, true)
        .ok_or(EditError::Invalid("not a parent of this person"))?;
    let link = collection(doc, "RelationshipCollection")
        .elements_mut()
        .find(|l| l.attr("Id").as_deref() == Some(link_id.as_str()))
        .expect("found above");
    set_child_text(link, "ParentChildModifier", modifier, &["CustomParentChildModifier", "ParentChildModifier"]);
    Ok(())
}

pub fn unlink_spouse(doc: &mut Document, a: &str, b: &str) -> EditResult<()> {
    let link = link_kind_between(doc, "SpouseLink", a, b, false).ok_or(EditError::Invalid("not a couple"))?;
    remove_spouse_link(doc, &link);
    Ok(())
}

fn remove_relationship(doc: &mut Document, link_id: &str) {
    collection(doc, "RelationshipCollection")
        .remove_children_where(|l| l.name == "RelationshipLink" && l.attr("Id").as_deref() == Some(link_id));
}

/// Remove a couple link and every back-reference to it.
fn remove_spouse_link(doc: &mut Document, link_id: &str) {
    remove_relationship(doc, link_id);
    for person in collection(doc, "PeopleCollection").elements_mut() {
        for list in ["FamilySpouseLinks", "FamilyChildLinks"] {
            if let Some(links) = person.child_mut(list) {
                links.remove_children_where(|l| l.attr("FamilyId").as_deref() == Some(link_id));
            }
        }
    }
}

// ---------------------------------------------------------------- notes

const NOTE_ORDER: &[&str] = &["Format", "Text", "IsStoryNote", "IsChartNote"];

/// Set the owner's first note to plain `text`; empty text removes all notes.
fn write_note(owner: &mut Element, order: &[&str], text: &str) {
    if text.trim().is_empty() {
        owner.remove_children_where(|e| e.name == "Notes");
        return;
    }
    let notes = owner.ensure_child("Notes", order);
    let list = notes.ensure_child("Notes", &["Notes", "Index"]);
    if list.child("Note").is_none() {
        let mut note = record("Note");
        note.push(text_el("Format", "text/plain"));
        note.push(Element::new("Text"));
        note.push(text_el("IsStoryNote", "false"));
        note.push(text_el("IsChartNote", "false"));
        list.push(note);
    }
    let note = list.child_mut("Note").expect("ensured above");
    note.ensure_child("Format", NOTE_ORDER).set_text("text/plain");
    note.ensure_child("Text", NOTE_ORDER).set_text(text);
    if notes.child("Index").is_none() {
        notes.ensure_child("Index", &["Notes", "Index"]).set_text("0");
    }
}

/// Replace the person's (first) note with plain text; empty removes notes.
pub fn set_person_note(doc: &mut Document, person_id: &str, text: &str) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    write_note(person, PERSON_ORDER, text);
    touch(person);
    Ok(())
}

pub fn set_fact_note(doc: &mut Document, fact_id: &str, text: &str) -> EditResult<()> {
    write_note(fact_mut(doc, fact_id)?, FACT_ORDER, text);
    Ok(())
}

// ---------------------------------------------------------------- names

fn name_mut<'a>(person: &'a mut Element, name_id: &str) -> EditResult<&'a mut Element> {
    person
        .path_mut("PersonNameCollection/Names")
        .and_then(|names| names.elements_mut().find(|n| n.attr("Id").as_deref() == Some(name_id)))
        .ok_or(EditError::Invalid("no such name"))
}

/// Update an alternate name; `name_type` must be one of the app's `NameType`
/// values (see docs/design.md) or empty.
pub fn update_name(
    doc: &mut Document,
    person_id: &str,
    name_id: &str,
    given: &str,
    surname: &str,
    name_type: &str,
) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    let name = name_mut(person, name_id)?;
    name.ensure_child("Names", NAME_ORDER).set_text(given);
    name.ensure_child("Surname", NAME_ORDER).set_text(surname);
    set_child_text(name, "NameType", name_type, NAME_ORDER);
    touch(person);
    Ok(())
}

/// Remove a non-primary name.
pub fn remove_name(doc: &mut Document, person_id: &str, name_id: &str) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    if name_mut(person, name_id)?.child_text("IsPrimary").as_deref() == Some("true") {
        return Err(EditError::Invalid("the primary name cannot be removed"));
    }
    if let Some(names) = person.path_mut("PersonNameCollection/Names") {
        names.remove_children_where(|n| n.attr("Id").as_deref() == Some(name_id));
    }
    touch(person);
    Ok(())
}

// ---------------------------------------------------------------- couples & order

/// `status` is one of the app's `SpouseRelationshipStatus` values or empty.
pub fn set_couple_status(doc: &mut Document, link_id: &str, status: &str) -> EditResult<()> {
    let link = collection(doc, "RelationshipCollection")
        .elements_mut()
        .find(|l| l.attr("Id").as_deref() == Some(link_id) && l.attr("xsi:type").as_deref() == Some("SpouseLink"))
        .ok_or(EditError::Invalid("no such couple"))?;
    set_child_text(
        link,
        "SpouseRelationshipStatus",
        status,
        &["Facts", "SpouseRelationshipStatus", "NumberOfChildren"],
    );
    Ok(())
}

/// Swap the link matching `is_target` with its neighbour among links matching
/// `in_group` (`delta` -1 = earlier, +1 = later). Returns false at the ends.
fn move_link(
    doc: &mut Document,
    in_group: impl Fn(&Element) -> bool,
    is_target: impl Fn(&Element) -> bool,
    delta: i32,
) -> bool {
    let coll = collection(doc, "RelationshipCollection");
    let group: Vec<usize> = coll
        .children
        .iter()
        .enumerate()
        .filter(|(_, n)| matches!(n, crate::doc::Node::Element(e) if in_group(e)))
        .map(|(i, _)| i)
        .collect();
    let Some(pos) =
        group.iter().position(|&i| matches!(&coll.children[i], crate::doc::Node::Element(e) if is_target(e)))
    else {
        return false;
    };
    let Some(&other) = group.get((pos as i32 + delta) as usize) else { return false };
    coll.children.swap(group[pos], other);
    true
}

/// Move `child` earlier (`delta` -1) or later (+1) in `parent`'s children.
pub fn move_child(doc: &mut Document, parent: &str, child: &str, delta: i32) -> EditResult<bool> {
    let is_parent_link = |l: &Element| {
        l.attr("xsi:type").as_deref() == Some("ParentChildLink") && l.attr("Person1Id").as_deref() == Some(parent)
    };
    Ok(move_link(doc, is_parent_link, |l| is_parent_link(l) && l.attr("Person2Id").as_deref() == Some(child), delta))
}

/// Move the couple of `person` and `spouse` earlier or later among `person`'s couples.
pub fn move_spouse(doc: &mut Document, person: &str, spouse: &str, delta: i32) -> EditResult<bool> {
    let involves =
        |l: &Element, p: &str| l.attr("Person1Id").as_deref() == Some(p) || l.attr("Person2Id").as_deref() == Some(p);
    let is_couple = |l: &Element| l.attr("xsi:type").as_deref() == Some("SpouseLink") && involves(l, person);
    Ok(move_link(doc, is_couple, |l| is_couple(l) && involves(l, spouse), delta))
}

// ---------------------------------------------------------------- custom events

/// Add a user-named event (stored as `EVEN` with `CustomFactTag`); returns its id.
pub fn add_custom_fact(doc: &mut Document, person_id: &str, name: &str, input: &FactInput) -> EditResult<Id> {
    if name.trim().is_empty() {
        return Err(EditError::Invalid("the event needs a name"));
    }
    let id = add_fact(doc, person_id, "EVEN", input)?;
    fact_mut(doc, &id)?.ensure_child("CustomFactTag", FACT_ORDER).set_text(name.trim());
    Ok(id)
}

// ---------------------------------------------------------------- photos

pub fn set_primary_photo(doc: &mut Document, person_id: &str, media_id: &str) -> EditResult<()> {
    let person = person_mut(doc, person_id)?;
    let has = person.child("Multimedia").is_some_and(|m| m.children_named("string").any(|s| s.text() == media_id));
    if !has {
        return Err(EditError::Invalid("that photo is not attached to this person"));
    }
    person.ensure_child("PrimaryImageId", PERSON_ORDER).set_text(media_id);
    // the face crop belongs to the previous photo
    person.remove_children_where(|e| e.name == "Face");
    touch(person);
    Ok(())
}

/// Detach a photo from a person. Returns true if no person or fact refers to
/// it anymore (the caller may then drop the file, see `Package::remove_photo`).
pub fn detach_photo(doc: &mut Document, person_id: &str, media_id: &str) -> EditResult<bool> {
    let person = person_mut(doc, person_id)?;
    if let Some(list) = person.child_mut("Multimedia") {
        list.remove_children_where(|s| s.name == "string" && s.text() == media_id);
    }
    if person.child_text("PrimaryImageId").as_deref() == Some(media_id) {
        let next = person.child("Multimedia").and_then(|m| m.children_named("string").next()).map(Element::text);
        person.remove_children_where(|e| e.name == "Face");
        match next {
            Some(next) => person.ensure_child("PrimaryImageId", PERSON_ORDER).set_text(&next),
            None => {
                person.remove_children_where(|e| e.name == "PrimaryImageId");
            }
        }
    }
    touch(person);
    fn refers(el: &Element, id: &str) -> bool {
        ((el.name == "string" || el.name == "PrimaryImageId") && el.text() == id)
            || el.elements().any(|c| refers(c, id))
    }
    let used = ["PeopleCollection", "RelationshipCollection", "SourceCollection"]
        .iter()
        .any(|c| doc.root.child(c).is_some_and(|c| refers(c, media_id)));
    Ok(!used)
}

pub(crate) fn remove_packaged_file(doc: &mut Document, media_id: &str) -> Option<String> {
    let files = doc.root.child_mut("PackagedFileCollection")?;
    let path = files.elements().find(|f| f.attr("Id").as_deref() == Some(media_id))?.child_text("RelativePath");
    files.remove_children_where(|f| f.attr("Id").as_deref() == Some(media_id));
    path.map(|p| p.replace('\\', "/"))
}

// ---------------------------------------------------------------- file

/// Record save time and the person last worked on, as the app does on save.
pub fn touch_file(doc: &mut Document, last_person: Option<&str>) {
    let props = doc.root.ensure_child("FileProperties", FAMILY_ORDER);
    if props.child("LastSavedDate").is_some() {
        props.ensure_child("LastSavedDate", &[]).set_text(&utc_now());
    }
    if let Some(p) = last_person {
        doc.root
            .ensure_child("FamilyProperties", FAMILY_ORDER)
            .ensure_child("LastSavedPersonId", &["LastSavedPersonId", "HomePersonId"])
            .set_text(p);
    }
}

/// Dangling references and inconsistencies, as human-readable messages.
pub fn check(doc: &Document) -> Vec<String> {
    let tree = crate::model::Tree::from_doc(doc);
    let mut problems = Vec::new();
    let spouse_ids: std::collections::HashSet<&str> = tree.spouse_links.iter().map(|l| l.id.as_str()).collect();
    for l in &tree.parent_links {
        for p in [&l.parent, &l.child] {
            if tree.person(p).is_none() {
                problems.push(format!("parent link {} refers to missing person {p}", l.id));
            }
        }
    }
    for l in &tree.spouse_links {
        for p in [&l.person1, &l.person2] {
            if tree.person(p).is_none() {
                problems.push(format!("spouse link {} refers to missing person {p}", l.id));
            }
        }
    }
    for p in doc.root.child("PeopleCollection").into_iter().flat_map(|c| c.children_named("Person")) {
        let pid = p.attr("Id").unwrap_or_default();
        for list in ["FamilySpouseLinks", "FamilyChildLinks"] {
            for l in p.child(list).into_iter().flat_map(Element::elements) {
                let fam = l.attr("FamilyId").unwrap_or_default();
                if !spouse_ids.contains(fam.as_ref()) {
                    problems.push(format!("{pid}: {} refers to missing couple {fam}", l.name));
                }
            }
        }
    }
    problems
}

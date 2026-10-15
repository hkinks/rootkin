//! Fact-level conversions of the schema migration: LDS facts, the obsolete `Biology` fields, witnesses,
//! fact addresses and citations, `FactType`, and the obsolete members inside citations.
//!
//! Every rule here was derived from the app's conversion chain and checked against a differential run of that
//! chain on small synthetic inputs (see docs/design.md, "Schema migration"); the constants below are the
//! values those runs produced.

use super::*;
use crate::edit::{CITATION_COLLECTION_ORDER, CITATION_ORDER, NAME_ORDER};
use crate::lds_temples::temple_name;

/// `Role` enum of the app (witnesses/associations, citations).
const ROLES: [&str; 35] = [
    "Witness",
    "GodParent",
    "Sponsor",
    "Relative",
    "Friend",
    "Bestman",
    "Bridesmaid",
    "PageBoy",
    "Husband",
    "Wife",
    "SpousePartner",
    "Father",
    "Mother",
    "Parent",
    "Son",
    "Daughter",
    "Child",
    "Brother",
    "Sister",
    "Sibling",
    "GrandFather",
    "GrandMother",
    "GrandParent",
    "GrandSon",
    "GrandDaughter",
    "GrandChild",
    "Cousin",
    "Uncle",
    "Aunt",
    "Neighbor",
    "Officiator",
    "MultipleBirth",
    "Clergy",
    "Custom",
    "Blank",
];

/// `EventType` enum of the app (GEDCOM tags), the old `Citation/Event` text.
const EVENT_TYPES: &[&str] = &[
    "EVEN", "ADOP", "BIRT", "BAPM", "BARM", "BASM", "BLES", "BURI", "CHR", "CHRA", "CONF", "CREM", "DEAT", "EMIG",
    "FCOM", "GRAD", "IMMI", "NATU", "ORDN", "RETI", "PROB", "WILL", "ANUL", "CENS", "DIV", "DIVF", "ENGA", "MARR",
    "MARB", "MARC", "MARL", "MARS", "FACT", "CAST", "EDUC", "NATI", "OCCU", "PROP", "RELI", "RESI", "TITL", "DSCR",
    "NCHI", "NMR", "SSN", "IDNO", "Blank",
];

/// `LDSDateStatus` enum of the app.
const LDS_DATE_STATUS: [&str; 15] = [
    "Bic",
    "Canceled",
    "Child",
    "Dns",
    "DnsCan",
    "Stillborn",
    "Completed",
    "Pre1970",
    "Infant",
    "Excluded",
    "Submitted",
    "Uncleared",
    "Qualified",
    "Cleared",
    "Blank",
];

/// Child order of an `Association` record.
pub(crate) const ASSOCIATION_ORDER: &[&str] =
    &["PersonId", "PersonName", "Role", "CustomRole", "AssociationCitationCollection", "Notes"];

/// Case-insensitive match of an enum value; `None` if `text` is not one of `names`.
fn parse_enum(text: &str, names: &[&str]) -> Option<String> {
    let t = text.trim();
    names.iter().find(|n| !n.is_empty() && n.eq_ignore_ascii_case(t)).map(|n| n.to_string())
}

/// A new `Association` record.
pub(super) fn association(
    person_id: &str,
    person_name: Option<&str>,
    role: &str,
    custom_role: Option<&str>,
) -> Element {
    let mut a = record("Association");
    if !person_id.is_empty() {
        a.push(text_el("PersonId", person_id));
    }
    if let Some(n) = person_name {
        a.push(text_el("PersonName", n));
    }
    a.push(text_el("Role", role));
    if let Some(c) = custom_role {
        a.push(text_el("CustomRole", c));
    }
    a
}

/// Append `association` to the fact's `Associations`.
pub(super) fn add_association(fact: &mut Element, association: Element) {
    fact.ensure_child("Associations", FACT_ORDER).push(association);
}

// ------------------------------------------------------------------ one fact

/// The conversions every old fact (person or couple) goes through besides place, date and note:
/// `FactType`, the three old address forms, the single `Citation`, and `Witnesses` (-> `Associations`).
pub(super) fn migrate_fact_members(fact: &mut Element, version: Option<f64>, report: &mut Report) {
    // `FactType` Individual|Family: the app turns "Family" into the `IsFamily` flag. Files older than 7.0 used
    // the element differently and the app ignored it there.
    if let Some(kind) = take_text(fact, "FactType") {
        if kind == "Family" && version.is_none_or(|v| v >= 7.0) && fact.child_text("IsFamily").is_none() {
            fact.ensure_child("IsFamily", FACT_ORDER).set_text("true");
        }
    }
    migrate_fact_address(fact);
    if let Some(mut citation) = take(fact, "Citation") {
        if citation.elements().any(|e| e.name == "Source" || e.name == "Page") {
            ensure_record_attrs(&mut citation);
            citation_collection(fact, "FactCitationCollection", FACT_ORDER).push(citation);
        }
    }
    if let Some(witnesses) = take(fact, "Witnesses") {
        for w in witnesses.elements() {
            add_association(fact, witness_association(w));
            report.associations += 1;
        }
    }
}

/// The `Citations` list of a citation collection wrapper, created in `order` with its `Index`.
fn citation_collection<'a>(owner: &'a mut Element, wrapper: &str, order: &[&str]) -> &'a mut Element {
    let w = owner.ensure_child(wrapper, order);
    if w.child("Index").is_none() {
        w.ensure_child("Index", CITATION_COLLECTION_ORDER).set_text("0");
    }
    w.ensure_child("Citations", CITATION_COLLECTION_ORDER)
}

/// An old `FactWitness` becomes an `Association` of the same fact: `Type`/`CustomType` (older: strings) are the
/// role and its custom text; an unknown `Type` is a custom role, a missing one a plain witness.
fn witness_association(w: &Element) -> Element {
    let role = match nonblank(w.child_text("Type")) {
        Some(t) => parse_enum(&t, &ROLES).unwrap_or_else(|| "Custom".to_string()),
        None => nonblank(w.child_text("Role")).and_then(|r| parse_enum(&r, &ROLES)).unwrap_or_else(|| "Witness".into()),
    };
    let custom = nonblank(w.child_text("CustomType")).or_else(|| nonblank(w.child_text("CustomRole")));
    let mut a = association(
        &w.child_text("PersonId").unwrap_or_default(),
        nonblank(w.child_text("PersonName")).as_deref(),
        &role,
        custom.as_deref(),
    );
    if let Some(c) = w.child("WitnessCitationCollection") {
        let mut c = c.clone();
        c.name = "AssociationCitationCollection".into();
        a.ensure_child("AssociationCitationCollection", ASSOCIATION_ORDER).children = c.children;
    }
    if let Some(n) = w.child("Notes") {
        *a.ensure_child("Notes", ASSOCIATION_ORDER) = n.clone();
    }
    a
}

/// Fact addresses: `Address` (a string, oldest), `Addresses/Addresses/Address` (a list, first one wins) and
/// `FullAddress` with its address lines all end up as `FullAddress` whose `Details` hold the lines.
fn migrate_fact_address(fact: &mut Element) {
    let text_address = take(fact, "Address").map(|e| e.text()).and_then(|t| nonblank(Some(t)));
    let list_address = take(fact, "Addresses").and_then(|list| {
        let inner = list.child("Addresses").unwrap_or(&list);
        inner.children_named("Address").next().cloned()
    });
    if let Some(text) = text_address {
        if fact.child("FullAddress").is_none() {
            let mut a = record("FullAddress");
            a.push(text_el("Details", &text));
            *fact.ensure_child("FullAddress", FACT_ORDER) = a;
        }
    }
    if let Some(mut first) = list_address {
        first.name = "FullAddress".into();
        ensure_record_attrs(&mut first);
        *fact.ensure_child("FullAddress", FACT_ORDER) = first;
    }
    if let Some(address) = fact.child_mut("FullAddress") {
        join_address_lines(address);
    }
}

/// `ContactName`, `Line1-3`, `City`, `State`, `PostalCode`, `Country` -> `Details`, one per line.
pub(super) fn join_address_lines(address: &mut Element) {
    let mut lines = Vec::new();
    for field in ["ContactName", "Line1", "Line2", "Line3", "City", "State", "PostalCode", "Country"] {
        if let Some(v) = nonblank(take_text(address, field)) {
            lines.push(v);
        }
    }
    if !lines.is_empty() {
        let order = ["Details", "Phone", "Fax", "Email", "WWW"];
        address.ensure_child("Details", &order).set_text(&lines.join("\n"));
    }
}

// ------------------------------------------------------------------ citations

/// Obsolete members of every `Citation` record: `Event`/`CustomEvent` -> `EventType`/`CustomEventType`,
/// `RoleInEvent` -> `Role`, `Text` -> `ActualTexts`.
pub(super) fn migrate_citations(el: &mut Element) {
    if el.name == "Citation" {
        migrate_citation_members(el);
    }
    for child in el.elements_mut() {
        migrate_citations(child);
    }
}

fn migrate_citation_members(c: &mut Element) {
    if let Some(event) = take_text(c, "Event") {
        // a text that is not an event type becomes Blank, which is the default and not written
        if let Some(e) = parse_enum(&event, EVENT_TYPES).filter(|e| e != "Blank") {
            c.ensure_child("EventType", CITATION_ORDER).set_text(&e);
        }
    }
    if let Some(custom) = nonblank(take_text(c, "CustomEvent")) {
        c.ensure_child("CustomEventType", CITATION_ORDER).set_text(&custom);
    }
    if let Some(role) = take_text(c, "RoleInEvent") {
        let mapped = match role.as_str() {
            "CitationRoleFATH" => Some("Father"),
            "CitationRoleMOTH" => Some("Mother"),
            "CitationRoleHUSB" => Some("Husband"),
            "CitationRoleWIFE" => Some("Wife"),
            "CitationRoleCHIL" => Some("Child"),
            "CitationRoleSPOU" => Some("SpousePartner"),
            "Custom" => Some("Custom"),
            _ => None,
        };
        if let (Some(r), None) = (mapped, c.child("Role")) {
            c.ensure_child("Role", CITATION_ORDER).set_text(r);
        }
    }
    if let Some(text) = nonblank(take_text(c, "Text")) {
        let mut note = record("FlatNote");
        note.push(text_el("Format", "text/plain"));
        note.push(text_el("Text", &text));
        let texts = c.ensure_child("ActualTexts", CITATION_ORDER);
        texts.ensure_child("FlatNotes", &["FlatNotes", "Index"]).push(note);
        if texts.child("Index").is_none() {
            texts.ensure_child("Index", &["FlatNotes", "Index"]).set_text("0");
        }
    }
}

/// The old single `Citation` of a name becomes the name's `NameCitationCollection`.
pub(super) fn migrate_name_citation(name: &mut Element) {
    if let Some(mut citation) = take(name, "Citation") {
        if citation.elements().any(|e| e.name == "Source" || e.name == "Page") {
            ensure_record_attrs(&mut citation);
            citation_collection(name, "NameCitationCollection", NAME_ORDER).push(citation);
        }
    }
}

// ------------------------------------------------------------------ LDS facts

/// Tags the app maps back from the `EVEN_` prefix the LDS conversion gives (confirmation, endowment, child
/// sealing, baptism, spouse sealing); any other tag keeps the prefix.
const LDS_TAGS: [&str; 5] = ["CONL", "ENDL", "SLGC", "BAPL", "SLGS"];

/// An old `LDSFacts` entry (`xsi:type` LDSBaptismFact, LDSEndowmentFact, LDSChildSealingFact,
/// LDSSpouseSealingFact) as a current `Fact`: religious affiliation "LDS Church", the temple (by code) as the
/// place, the per-type date status as `DateStatus`; date, place, notes and citations are carried over, anything
/// else (details, files, flags) is not, as in the app.
pub(super) fn lds_fact(old: &Element, family: bool, version: Option<f64>, report: &mut Report) -> Element {
    let xsi = old.attr("xsi:type").map(|t| t.into_owned()).unwrap_or_default();
    let kind = xsi.rsplit(':').next().unwrap_or("");
    let (inferred_tag, status_field) = match kind {
        "LDSBaptismFact" => ("BAPL", Some("BaptismDateStatus")),
        "LDSEndowmentFact" => ("ENDL", Some("EndowmentDateStatus")),
        "LDSChildSealingFact" => ("SLGC", Some("ChildSealingDateStatus")),
        "LDSSpouseSealingFact" => ("SLGS", Some("SpouseSealingDateStatus")),
        _ => ("", None),
    };
    let old_tag = nonblank(old.child_text("Tag")).unwrap_or_else(|| inferred_tag.to_string());
    let tag = if LDS_TAGS.contains(&old_tag.as_str()) { old_tag } else { format!("EVEN_{old_tag}") };

    let mut fact = record("Fact");
    fact.push(text_el("Tag", &tag));
    fact.push(text_el("ReligiousAffiliation", "LDS Church"));
    if family {
        fact.ensure_child("IsFamily", FACT_ORDER).set_text("true");
    }
    for name in ["FactDate", "PlaceDetails", "FactCitationCollection", "Notes"] {
        if let Some(src) = old.child(name) {
            *fact.ensure_child(name, FACT_ORDER) = src.clone();
        }
    }
    if let Some(code) = nonblank(old.child_text("TempleCode")).filter(|c| c != "BLANK") {
        let place = ensure_record(&mut fact, "PlaceDetails", FACT_ORDER);
        place
            .ensure_child("Description", &["Description", "Longitude", "Latitude", "BoundingBox"])
            .set_text(&temple_name(&code));
    }
    if let Some(status) = status_field
        .and_then(|f| old.child_text(f))
        .and_then(|s| parse_enum(&s, &LDS_DATE_STATUS))
        .filter(|s| s != "Blank")
    {
        fact.ensure_child("DateStatus", FACT_ORDER).set_text(&status);
    }
    migrate_fact(&mut fact, version, report);
    report.facts += 1;
    fact
}

// ------------------------------------------------------------------ biology

/// The obsolete `Biology` members: blood group, eye and hair colour become facts (`FACT_BloodGroup`,
/// `FACT_RightEyeColor`, `FACT_LeftEyeColor`, `FACT_HairColor`, text in `Details`; a value of "Unknown" or
/// "NotYetKnown" is no fact), the haplogroups become `YDNA`/`MtDNA` entries. The facts are returned in the order
/// the app adds them.
pub(super) fn biology_facts(biology: &mut Element) -> Vec<Element> {
    let mut facts = Vec::new();
    for (field, tag) in [
        ("BloodGroup", "FACT_BloodGroup"),
        ("RightEyeColor", "FACT_RightEyeColor"),
        ("LeftEyeColor", "FACT_LeftEyeColor"),
        ("HairColor", "FACT_HairColor"),
    ] {
        // an empty element is a (detail-less) fact, a missing one is nothing
        let Some(value) = take_text(biology, field) else { continue };
        if value == "Unknown" || value == "NotYetKnown" {
            continue;
        }
        let mut fact = record("Fact");
        fact.push(text_el("Tag", tag));
        if !value.is_empty() {
            fact.ensure_child("Details", FACT_ORDER).set_text(&value);
        }
        facts.push(fact);
    }
    for (field, wrapper) in [("HaplogroupY", "YDNA"), ("HaplogroupM", "MtDNA")] {
        let Some(group) = nonblank(take_text(biology, field)).filter(|g| g != "Unknown") else { continue };
        let mut test = record(wrapper);
        test.push(text_el("Haplogroup", &group));
        if wrapper == "YDNA" {
            test.push(Element::new("STRs"));
        }
        test.push(Element::new("SNPs"));
        const ORDER: &[&str] = &["Gender", "YDNA", "AtDNA", "MtDNA", "Notes"];
        biology.ensure_child(wrapper, ORDER).push(test);
    }
    facts
}

// ------------------------------------------------------------------ helpers for the callers

/// True if two elements are equal apart from their `Id`/`Creation` attributes.
pub(super) fn same_content(a: &Element, b: &Element) -> bool {
    fn strip(el: &mut Element) {
        el.attrs.retain(|(k, _)| k != "Id" && k != "Creation");
        for c in el.elements_mut() {
            strip(c);
        }
    }
    let (mut a, mut b) = (a.clone(), b.clone());
    strip(&mut a);
    strip(&mut b);
    a == b
}

/// The person's birth fact (the primary `BIRT`, else the first); a new one if there is none.
pub(super) fn birth_fact<'a>(doc: &'a mut Document, person_id: &str) -> &'a mut Element {
    let people = doc.root.child_mut("PeopleCollection").expect("a person was found in it");
    let person = people
        .elements_mut()
        .find(|p| p.name == "Person" && p.attr("Id").as_deref() == Some(person_id))
        .expect("the person exists");
    let list = person.ensure_child("Facts", PERSON_ORDER);
    let is_birth = |f: &Element| f.name == "Fact" && f.child_text("Tag").as_deref() == Some("BIRT");
    let position = list
        .elements()
        .position(|f| is_birth(f) && f.child_text("IsPrimary").as_deref() == Some("true"))
        .or_else(|| list.elements().position(is_birth));
    match position {
        Some(i) => list.elements_mut().nth(i).expect("position found above"),
        None => {
            let mut fact = record("Fact");
            fact.push(text_el("Tag", "BIRT"));
            fact.push(text_el("IsPrimary", "true"));
            list.push(fact)
        }
    }
}

//! Schema migration of older `.familyx` trees.
//!
//! Versions of the app before 11.3 serialized relationships per person
//! (`Person/Relationships`), names as `Person/Name` + `Person/AlternativeName`,
//! photos as `Person/Photos`, LDS facts, biology details, fact witnesses and
//! addresses in members that were later replaced, and so on. The current app reads such files into
//! its current classes (the old members survive as `Obsolete*` properties) and
//! runs a version-gated conversion chain (`FileConversions`) before showing the
//! tree. This module performs the structural part of that chain on the XML
//! [`Document`], so the model, the editor and `edit::check` see ordinary
//! current-schema data. The mapping is documented in `docs/design.md`.
//!
//! The conversion is applied to every tree whose `FileProperties/Version` is
//! older than [`CURRENT_VERSION`] (or missing, with obsolete elements present), and
//! it bumps `Version`: the app gates its own conversions on that number and
//! would otherwise run them a second time over already converted data.

use std::collections::{HashMap, HashSet};

use crate::doc::{Document, Element, Node};
use crate::edit::{self, record, text_el, FACT_DATE_ORDER, FACT_ORDER, FAMILY_ORDER, NAME_ORDER, PERSON_ORDER};

pub(crate) mod facts;
pub mod schema14;
mod stories;

/// Schema version written by the current app (`FileProperties/Version`).
pub const CURRENT_VERSION: f64 = 14.8;

/// Root-level elements of the old schema.
const LEGACY_ROOT: [&str; 5] =
    ["Author", "PhotoCollection", "AttachmentCollection", "AudioVideoCollection", "PackagedFilePropertiesCollection"];
/// Person-level elements of the old schema.
const LEGACY_PERSON: [&str; 9] = [
    "Relationships",
    "Photos",
    "Attachments",
    "AudioVideo",
    "LDSFacts",
    "Story",
    "Name",
    "AlternativeName",
    "ContactAddress",
];

/// What a migration did, for display and tests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// `FileProperties/Version` before migration (0 if absent).
    pub from_version: String,
    pub names: usize,
    pub spouse_links: usize,
    pub parent_links: usize,
    pub media: usize,
    pub notes: usize,
    /// Facts made from `LDSFacts` and the obsolete `Biology` fields.
    pub facts: usize,
    /// Associations made from godparent/sponsor/witness relations and fact witnesses.
    pub associations: usize,
    /// Old data that was kept but could not be converted (see docs/design.md).
    pub warnings: Vec<String>,
    /// Old data that was dropped because the app drops it too (relations to people that do not exist, ...).
    pub dropped: Vec<String>,
    /// Package parts that were absorbed into the tree (the story database); saving drops them.
    pub consumed_parts: Vec<String>,
}

/// Obsolete members of a `Fact`.
const LEGACY_FACT: [&str; 7] = ["FactType", "Place", "Address", "Addresses", "Witnesses", "Citation", "Note"];
/// Obsolete members of `Biology`.
const LEGACY_BIOLOGY: [&str; 6] =
    ["HairColor", "LeftEyeColor", "RightEyeColor", "BloodGroup", "HaplogroupY", "HaplogroupM"];

/// True if the tree still contains elements of the old schema (anything [`migrate`] converts, or keeps
/// because it cannot, like a `Story` whose database is unreadable).
pub fn has_legacy_elements(doc: &Document) -> bool {
    let legacy_fact = |facts: &Element| {
        facts.children_named("Fact").any(|f| f.elements().any(|e| LEGACY_FACT.contains(&e.name.as_str())))
    };
    doc.root.elements().any(|e| LEGACY_ROOT.contains(&e.name.as_str()))
        || doc.root.path("FamilyProperties/FamilyCrest").is_some()
        || doc.root.child("PeopleCollection").is_some_and(|c| {
            c.elements().any(|p| {
                p.elements().any(|e| LEGACY_PERSON.contains(&e.name.as_str()))
                    || p.child("Biology")
                        .is_some_and(|b| b.elements().any(|e| LEGACY_BIOLOGY.contains(&e.name.as_str())))
                    || p.child("Facts").is_some_and(legacy_fact)
            })
        })
        || doc
            .root
            .child("RelationshipCollection")
            .is_some_and(|c| c.elements().any(|l| l.child("Facts").is_some_and(legacy_fact)))
}

fn file_version(doc: &Document) -> Option<f64> {
    doc.root.child_text("FileProperties/Version").and_then(|v| v.trim().parse().ok())
}

/// Convert an old-schema tree in place. `parts` looks up a package part by
/// name (photos, story files). Returns `None` if nothing needed converting.
pub fn migrate(doc: &mut Document, parts: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<Report> {
    // Trees the app (or this crate) wrote at the current version are done, even if
    // some unconvertible old elements were kept; unversioned trees need old elements.
    match file_version(doc) {
        Some(v) if v >= CURRENT_VERSION => return None,
        None if !has_legacy_elements(doc) => return None,
        _ => {}
    }
    let mut report =
        Report { from_version: doc.root.child_text("FileProperties/Version").unwrap_or_default(), ..Report::default() };

    let version = file_version(doc);
    normalize_ids(doc);
    move_author(doc);
    let crest = migrate_media(doc, parts, &mut report);
    migrate_people(doc, parts, version, &mut report);
    migrate_relationships(doc, version, &mut report);
    assign_family_crest(doc, crest);
    facts::migrate_citations(&mut doc.root);
    schema14::upgrade(doc, version);
    // The `Version` bump makes the app skip its own (already done) conversions.
    let props = doc.root.ensure_child("FileProperties", FAMILY_ORDER);
    props.ensure_child("Version", &["Version", "LastSavedDate", "CreationDate"]).set_text(&CURRENT_VERSION.to_string());
    Some(report)
}

// ------------------------------------------------------------------ helpers

fn take(el: &mut Element, name: &str) -> Option<Element> {
    let idx = el.children.iter().position(|n| matches!(n, Node::Element(e) if e.name == name))?;
    match el.children.remove(idx) {
        Node::Element(e) => Some(e),
        _ => None,
    }
}

fn take_text(el: &mut Element, name: &str) -> Option<String> {
    take(el, name).filter(|e| e.attr("xsi:nil").as_deref() != Some("true")).map(|e| e.text())
}

fn nonblank(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

fn is_true(s: &Option<String>) -> bool {
    s.as_deref().is_some_and(|s| s.trim().eq_ignore_ascii_case("true"))
}

/// Give a record (`Id`, `Creation`) the attributes the app always writes.
fn ensure_record_attrs(el: &mut Element) {
    if el.attr("Id").is_none_or(|v| v.is_empty()) {
        el.set_attr("Id", &edit::new_id());
    }
    if el.attr("Creation").is_none() {
        el.set_attr("Creation", &edit::utc_now());
    }
}

fn ensure_record<'a>(parent: &'a mut Element, name: &str, order: &[&str]) -> &'a mut Element {
    let fresh = parent.child(name).is_none();
    let el = parent.ensure_child(name, order);
    if fresh {
        el.attrs = record(name).attrs;
    }
    el
}

fn set_text_in(parent: &mut Element, name: &str, value: &str, order: &[&str]) {
    parent.ensure_child(name, order).set_text(value);
}

/// Append a note to `owner/Notes/Notes`, creating the wrapper in `order`.
fn push_note(owner: &mut Element, order: &[&str], text: &str, format: &str, story: bool) {
    let notes = owner.ensure_child("Notes", order);
    let list = notes.ensure_child("Notes", &["Notes", "Index"]);
    let mut note = record("Note");
    note.push(text_el("Format", format));
    let mut body = Element::new("Text");
    body.set_text(text);
    note.push(body);
    note.push(text_el("IsStoryNote", if story { "true" } else { "false" }));
    note.push(text_el("IsChartNote", "false"));
    list.push(note);
    if notes.child("Index").is_none() {
        notes.ensure_child("Index", &["Notes", "Index"]).set_text("0");
    }
}

/// `Note` (a plain string on every object in the old schema) becomes a `Notes` entry.
fn migrate_note_string(owner: &mut Element, order: &[&str], report: &mut Report) {
    if let Some(text) = nonblank(take_text(owner, "Note")) {
        push_note(owner, order, &text, "text/plain", false);
        report.notes += 1;
    }
}

// ------------------------------------------------------------------ ids

/// .NET `Guid.ToByteArray()` as unpadded base64url: the app's 22-character ids.
fn guid_to_id(s: &str) -> Option<String> {
    use base64::Engine;
    let b = s.as_bytes();
    if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
        return None;
    }
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok();
    let mut bytes: Vec<u8> = (0..16).map(byte).collect::<Option<_>>()?;
    bytes[0..4].reverse();
    bytes[4..6].reverse();
    bytes[6..8].reverse();
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

/// Very old trees used 36-character GUID ids; the app rewrites them to the
/// 22-character form everywhere (`FileConversions`, version < 2.1).
fn normalize_ids(doc: &mut Document) {
    fn collect(el: &Element, map: &mut HashMap<String, String>) {
        if let Some(id) = el.attr("Id") {
            if let Some(new) = guid_to_id(&id) {
                map.insert(id.into_owned(), new);
            }
        }
        for c in el.elements() {
            collect(c, map);
        }
    }
    fn apply(el: &mut Element, map: &HashMap<String, String>) {
        for (_, v) in el.attrs.iter_mut() {
            if let Some(new) = map.get(v.as_str()) {
                *v = new.clone();
            }
        }
        if el.elements().next().is_none() {
            let t = el.text();
            if let Some(new) = map.get(&t) {
                el.set_text(new);
            }
        }
        for c in el.elements_mut() {
            apply(c, map);
        }
    }
    let mut map = HashMap::new();
    collect(&doc.root, &mut map);
    if !map.is_empty() {
        apply(&mut doc.root, &map);
    }
}

// ------------------------------------------------------------------ author

fn move_author(doc: &mut Document) {
    let Some(author) = take(&mut doc.root, "Author") else { return };
    let coll = doc.root.ensure_child("AuthorCollection", FAMILY_ORDER);
    let id = author.attr("Id");
    if id.is_none() || !coll.elements().any(|a| a.attr("Id") == id) {
        coll.push(author);
    }
}

// ------------------------------------------------------------------ dates

const DATE_ORDER: &[&str] = FACT_DATE_ORDER;
/// `CalendarDescriptor` enum of the app (the old `CalendarType` text).
const CALENDARS: [&str; 28] = [
    "GregorianCalendar",
    "JulianCalendar",
    "FrenchRepublicanCalendar",
    "SwedishCalendar",
    "QuakerCalendar",
    "IcelandicCalendar",
    "HebrewCalendar",
    "AstronomicalSolarHijriCalendar",
    "LunarHijriCalendar",
    "UmmAlQuraCalendar",
    "IsoCalendar",
    "ArmenianCalendar",
    "BahaiCalendar",
    "HinduSolarCalendar",
    "HinduLunisolarCalendar",
    "ThaiBuddhistCalendar",
    "TibetanLunisolarCalendar",
    "JapaneseCalendar",
    "JapaneseLunisolarCalendar",
    "KoreanCalendar",
    "KoreanLunisolarCalendar",
    "TaiwanCalendar",
    "TaiwanLunisolarCalendar",
    "ChineseLunisolarCalendar",
    "VietnameseLunisolarCalendar",
    "CopticCalendar",
    "EthiopicCalendar",
    "EgyptianCalendar",
];
/// `DateDescriptor` enum of the app.
const DESCRIPTORS: [&str; 13] = [
    "Exact",
    "About",
    "Estimated",
    "Calculated",
    "ToOnly",
    "FromOnly",
    "Span",
    "Before",
    "After",
    "Range",
    "EitherOr",
    "Interpreted",
    "TextOnly",
];

type DateTime = (i32, u32, u32, u32, u32, u32);

/// `2001-02-03T04:05:06[.fff][Z|+hh:mm]`; `0001-01-01` (`DateTime.MinValue`) counts as unset.
fn parse_datetime(s: &str) -> Option<DateTime> {
    let s = s.trim();
    let (date, time) = s.split_once('T').unwrap_or((s, "00:00:00"));
    let mut d = date.splitn(3, '-');
    let year: i32 = d.next()?.parse().ok()?;
    let month: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    let mut t = time.split([':', '.', 'Z', '+', '-']);
    let mut next = || t.next().and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let (h, m, sec) = (next(), next(), next());
    ((year, month, day) != (1, 1, 1) && (1..=12).contains(&month) && (1..=31).contains(&day))
        .then_some((year, month, day, h, m, sec))
}

/// Old `CustomDate` members (`StartDate`, `EndDate`, `Descriptor`, `StartDateDescriptor`, `IsTextDate`,
/// `IsDateRange`, `IsStartBC`, `IsEndBC`, `CalendarType`) become the current `Start*`/`End*` fields and a
/// `DateDescriptor`, as the app's oldest conversion does (checked against a differential run of it: a date
/// range is `Span`, or `Range` if its `StartDateDescriptor` is "Between"; otherwise `Descriptor` is the enum
/// value, an unknown one `Exact`).
fn migrate_date(date: &mut Element) {
    let start = take(date, "StartDate");
    let end = take(date, "EndDate");
    let descriptor = take_text(date, "Descriptor");
    let start_descriptor = take_text(date, "StartDateDescriptor");
    let _ = take(date, "EndDateDescriptor");
    let text_date = take_text(date, "IsTextDate");
    let range = take_text(date, "IsDateRange");
    let start_bc = take_text(date, "IsStartBC");
    let end_bc = take_text(date, "IsEndBC");
    let calendar = take_text(date, "CalendarType");
    let parse = |e: &Option<Element>| {
        e.as_ref().filter(|e| e.attr("xsi:nil").as_deref() != Some("true")).and_then(|e| parse_datetime(&e.text()))
    };
    let (start, end) = (parse(&start), parse(&end));
    let any = start.is_some()
        || end.is_some()
        || descriptor.is_some()
        || start_descriptor.is_some()
        || text_date.is_some()
        || range.is_some()
        || calendar.is_some();
    if !any {
        return;
    }
    for (prefix, value) in [("Start", start), ("End", end)] {
        let Some((y, m, d, h, mi, s)) = value else { continue };
        let name = |f: &str| format!("{prefix}{f}");
        set_text_in(date, &name("Day"), &d.to_string(), DATE_ORDER);
        set_text_in(date, &name("Month"), &m.to_string(), DATE_ORDER);
        set_text_in(date, &name("Year"), &y.to_string(), DATE_ORDER);
        if (h, mi, s) != (0, 0, 0) {
            set_text_in(date, &name("Time"), &format!("0001-01-01T{h:02}:{mi:02}:{s:02}"), DATE_ORDER);
        }
    }
    // Era 1 = AD (the default); 0 = BC.
    if is_true(&start_bc) {
        set_text_in(date, "StartEra", "0", DATE_ORDER);
    }
    if is_true(&end_bc) {
        set_text_in(date, "EndEra", "0", DATE_ORDER);
    }
    let known = nonblank(calendar).and_then(|c| CALENDARS.iter().find(|k| k.eq_ignore_ascii_case(c.trim())).copied());
    if let Some(cal) = known.filter(|c| *c != "GregorianCalendar") {
        set_text_in(date, "StartCalendar", cal, DATE_ORDER);
        set_text_in(date, "EndCalendar", cal, DATE_ORDER);
    }
    let kind = if is_true(&text_date) {
        "TextOnly"
    } else if is_true(&range) {
        match start_descriptor.as_deref().map(str::trim) {
            Some("Between") => "Range",
            Some("To") => "Exact",
            _ => "Span",
        }
    } else {
        // the app also reads the plain words "From"/"To" as the open-ended descriptors
        let d = descriptor.as_deref().map(str::trim).map(|d| match d {
            "From" => "FromOnly",
            "To" => "ToOnly",
            d => d,
        });
        d.and_then(|d| DESCRIPTORS.iter().find(|k| k.eq_ignore_ascii_case(d)).copied()).unwrap_or("Exact")
    };
    if kind != "Exact" {
        set_text_in(date, "DateDescriptor", kind, DATE_ORDER);
    }
}

// ------------------------------------------------------------------ facts

/// Bring one `Fact` (person or couple) to the current shape.
fn migrate_fact(fact: &mut Element, version: Option<f64>, report: &mut Report) {
    ensure_record_attrs(fact);
    facts::migrate_fact_members(fact, version, report);
    if let Some(place) = nonblank(take_text(fact, "Place")) {
        let details = ensure_record(fact, "PlaceDetails", FACT_ORDER);
        if details.child_text("Description").is_none_or(|d| d.is_empty()) {
            details
                .ensure_child("Description", &["Description", "Longitude", "Latitude", "BoundingBox"])
                .set_text(&place);
        }
    }
    for name in ["FactDate", "FactDatePeriod"] {
        if let Some(date) = fact.child_mut(name) {
            migrate_date(date);
        }
    }
    migrate_note_string(fact, FACT_ORDER, report);
}

// ------------------------------------------------------------------ people

fn migrate_people(
    doc: &mut Document,
    parts: &dyn Fn(&str) -> Option<Vec<u8>>,
    version: Option<f64>,
    report: &mut Report,
) {
    let Some(people) = doc.root.child_mut("PeopleCollection") else { return };
    let mut stories = stories::StoryDb::new(parts, version);
    for person in people.elements_mut().filter(|p| p.name == "Person") {
        migrate_person_names(person, report);
        if let Some(facts) = person.child_mut("Facts") {
            for fact in facts.elements_mut().filter(|f| f.name == "Fact") {
                migrate_fact(fact, version, report);
            }
        }
        // new facts are appended in the order the app's conversion chain adds them: LDS facts, then biology
        let mut added = Vec::new();
        if let Some(lds) = take(person, "LDSFacts") {
            added.extend(lds.elements().map(|f| facts::lds_fact(f, false, version, report)));
        }
        if let Some(biology) = person.child_mut("Biology") {
            let from_biology = facts::biology_facts(biology);
            report.facts += from_biology.len();
            added.extend(from_biology);
            migrate_note_string(biology, &["Gender", "YDNA", "AtDNA", "MtDNA", "Notes"], report);
        }
        if !added.is_empty() {
            let facts = person.ensure_child("Facts", PERSON_ORDER);
            for fact in added {
                facts.push(fact);
            }
        }
        migrate_contact_address(person);
        migrate_story(person, parts, &mut stories, report);
    }
    if stories.used && !stories.unresolved {
        report.consumed_parts.push(stories::PART.to_string());
    }
}

fn migrate_person_names(person: &mut Element, report: &mut Report) {
    let mut old = Vec::new();
    if let Some(n) = take(person, "Name") {
        old.push((n, true));
    }
    if let Some(n) = take(person, "AlternativeName") {
        old.push((n, false));
    }
    if old.is_empty() {
        return;
    }
    let existing_primary = person
        .path("PersonNameCollection/Names")
        .is_some_and(|names| names.elements().any(|n| n.child_text("IsPrimary").as_deref() == Some("true")));
    let names = person.ensure_child("PersonNameCollection", PERSON_ORDER).ensure_child("Names", &[]);
    for (mut name, primary) in old {
        name.name = "Name".into();
        ensure_record_attrs(&mut name);
        let order = NAME_ORDER;
        if primary && !existing_primary {
            set_text_in(&mut name, "IsPrimary", "true", order);
        } else if name.child_text("IsPrimary").as_deref() == Some("true") {
            // an alternative name is never the primary one
            set_text_in(&mut name, "IsPrimary", "false", order);
        }
        facts::migrate_name_citation(&mut name);
        migrate_note_string(&mut name, order, report);
        // The primary name goes first, as the app keeps it.
        if primary {
            let at = names.children.iter().position(|n| matches!(n, Node::Element(_))).unwrap_or(0);
            names.children.insert(at, Node::Element(name));
        } else {
            names.push(name);
        }
        report.names += 1;
    }
}

/// `ContactAddress` (an `Address` with lines) becomes a `FACT_ContactAddress` fact.
fn migrate_contact_address(person: &mut Element) {
    let Some(address) = take(person, "ContactAddress") else { return };
    let mut lines = Vec::new();
    if let Some(details) = nonblank(address.child_text("Details")) {
        lines.push(details);
    } else {
        for field in ["ContactName", "Line1", "Line2", "Line3", "City", "State", "PostalCode", "Country"] {
            if let Some(v) = nonblank(address.child_text(field)) {
                lines.push(v);
            }
        }
    }
    if lines.is_empty() {
        return;
    }
    let mut fact = record("Fact");
    fact.push(text_el("Tag", "FACT_ContactAddress"));
    fact.push(text_el("IsPrimary", "true"));
    let mut place = record("PlaceDetails");
    place.push(text_el("Description", &lines.join("\n")));
    fact.push(place);
    person.ensure_child("Facts", PERSON_ORDER).push(fact);
}

/// A story file's bytes as text: UTF-8, a byte order mark is skipped (the app reads it with a `StreamReader`).
fn story_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim_start_matches('\u{feff}').to_string()
}

/// `Story` pointed at a file with the person's story, held the text itself (`GedcomImportStoryText`) or
/// named files in the story database part (`FileId`, `AnnotationsFileId`); it becomes a story note.
fn migrate_story(
    person: &mut Element,
    parts: &dyn Fn(&str) -> Option<Vec<u8>>,
    stories: &mut stories::StoryDb,
    report: &mut Report,
) {
    use stories::Lookup;
    let Some(story) = person.child("Story") else { return };
    let person_id = person.attr("Id").unwrap_or_default().into_owned();
    let mut remaining = story.clone();
    let mut notes = Vec::new();
    let mut warned = false;
    if let Some(text) = nonblank(take_text(&mut remaining, "GedcomImportStoryText")) {
        notes.push((text, "text/plain".to_string()));
    }
    if let Some(path) = nonblank(take_text(&mut remaining, "RelativePath")) {
        let found = part_candidates(&path).into_iter().find_map(|c| parts(&c));
        match found {
            Some(bytes) => {
                // The app reads every story file as rich text, whatever its name.
                notes.push((story_text(&bytes), "text/richtext".to_string()));
            }
            None => {
                report.warnings.push(format!("story file {path} of {person_id} is not in the package"));
                remaining.children.push(Node::Element(text_el("RelativePath", &path)));
                warned = true;
            }
        }
    }
    let mut story_resolved = true;
    if let Some(file_id) = nonblank(take_text(&mut remaining, "FileId")) {
        match stories.read(&file_id) {
            // The app skips empty files (it only looks at files with content): no note, nothing lost.
            Lookup::Found(bytes) => {
                if !bytes.is_empty() {
                    notes.push((story_text(&bytes), "text/richtext".to_string()));
                }
            }
            Lookup::Missing => {
                report.warnings.push(format!("story {file_id} of {person_id} is not in the story database"));
                story_resolved = false;
            }
            Lookup::Unavailable(why) => {
                report.warnings.push(format!("story {file_id} of {person_id} was not read: {why}"));
                story_resolved = false;
            }
        }
        if !story_resolved {
            remaining.children.push(Node::Element(text_el("FileId", &file_id)));
            stories.unresolved = true;
            warned = true;
        }
    }
    if let Some(file_id) = nonblank(take_text(&mut remaining, "AnnotationsFileId")) {
        // Annotations are WPF sticky notes (XAML) on the story text; they are not converted.
        let keep = !story_resolved
            || match stories.read(&file_id) {
                Lookup::Found(bytes) => {
                    if !bytes.is_empty() {
                        report
                            .warnings
                            .push(format!("annotations of the story of {person_id} are not converted (dropped)"));
                    }
                    false
                }
                Lookup::Missing => false,
                Lookup::Unavailable(_) => true,
            };
        if keep {
            remaining.children.push(Node::Element(text_el("AnnotationsFileId", &file_id)));
            stories.unresolved = true;
        }
    }
    for (text, format) in notes {
        push_note(person, PERSON_ORDER, &text, &format, true);
        report.notes += 1;
    }
    let leftover = remaining.elements().any(|e| !e.text().trim().is_empty());
    if leftover {
        if !warned {
            report.warnings.push(format!("story of {person_id} kept as is (unknown content)"));
        }
        if let Some(slot) = person.child_mut("Story") {
            *slot = remaining;
        }
    } else {
        take(person, "Story");
    }
}

// ------------------------------------------------------------------ media

/// Names under which an old relative path may be found in the package.
fn part_candidates(rel: &str) -> Vec<String> {
    let rel = rel.replace('\\', "/");
    let base = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    let mut out = vec![rel.clone(), rel.trim_start_matches('/').to_string(), format!("Multimedia/{rel}")];
    out.push(format!("Multimedia/{base}"));
    out.dedup();
    out
}

/// Photos, attachments and audio/video files, in the root collections and per
/// person, become `PackagedFile` entries (`Type` Photo/Attachment/AudioVideo),
/// referenced from `Person/Multimedia`; the avatar becomes `PrimaryImageId`. The family crest
/// (`FamilyProperties/FamilyCrest`) becomes a photo too; its id is returned for [`assign_family_crest`].
fn migrate_media(doc: &mut Document, parts: &dyn Fn(&str) -> Option<Vec<u8>>, report: &mut Report) -> Option<String> {
    let kinds = [
        ("Photos", "PhotoCollection", "Photo"),
        ("Attachments", "AttachmentCollection", "Attachment"),
        ("AudioVideo", "AudioVideoCollection", "AudioVideo"),
    ];
    // Properties recorded separately per old file id.
    let mut properties: HashMap<String, Element> = HashMap::new();
    if let Some(coll) = take(&mut doc.root, "PackagedFilePropertiesCollection") {
        for mut item in coll.children.into_iter().filter_map(|n| if let Node::Element(e) = n { Some(e) } else { None })
        {
            let id = item.attr("Id").map(|v| v.into_owned()).or_else(|| item.child_text("Id"));
            if let (Some(id), Some(props)) = (id, take(&mut item, "Properties")) {
                properties.insert(id, props);
            }
        }
    }

    let mut by_path: HashMap<String, String> = HashMap::new(); // normalized path -> new id
    let mut created: Vec<Element> = Vec::new();
    let mut register = |mut src: Element, kind: &str, properties: &mut HashMap<String, Element>| -> Option<String> {
        let path = nonblank(src.child_text("RelativePath"))?;
        let key = path.replace('\\', "/").to_lowercase();
        let old_id = src.attr("Id").map(|v| v.into_owned());
        if let Some(id) = by_path.get(&key) {
            return Some(id.clone());
        }
        let id = edit::new_id();
        by_path.insert(key, id.clone());
        let mut props = take(&mut src, "Properties");
        if let Some(extra) = old_id.as_ref().and_then(|o| properties.remove(o)) {
            props = Some(extra);
        }
        created.push(packaged_file(src, kind, &id, &path, props, parts));
        Some(id)
    };

    // root collections first (the app lists them before per-person files)
    for (_, root_name, kind) in kinds {
        if let Some(coll) = take(&mut doc.root, root_name) {
            for item in coll.children.into_iter().filter_map(|n| if let Node::Element(e) = n { Some(e) } else { None })
            {
                register(item, kind, &mut properties);
            }
        }
    }
    if let Some(people) = doc.root.child_mut("PeopleCollection") {
        for person in people.elements_mut().filter(|p| p.name == "Person") {
            let mut ids: Vec<String> = Vec::new();
            let mut avatar: Option<String> = None;
            for (field, _, kind) in kinds {
                let Some(list) = take(person, field) else { continue };
                for item in
                    list.children.into_iter().filter_map(|n| if let Node::Element(e) = n { Some(e) } else { None })
                {
                    let is_avatar = is_true(&item.child_text("IsAvatar"));
                    if let Some(id) = register(item, kind, &mut properties) {
                        if is_avatar && avatar.is_none() {
                            avatar = Some(id.clone());
                        }
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
            if ids.is_empty() {
                continue;
            }
            let multimedia = person.ensure_child("Multimedia", PERSON_ORDER);
            let have: HashSet<String> = multimedia.children_named("string").map(Element::text).collect();
            for id in &ids {
                if !have.contains(id) {
                    multimedia.push(text_el("string", id));
                }
            }
            if let Some(avatar) = avatar {
                if person.child("PrimaryImageId").is_none() {
                    set_text_in(person, "PrimaryImageId", &avatar, PERSON_ORDER);
                }
            }
        }
    }
    // The crest file must be in the package; the app drops a crest whose file it cannot find.
    let crest = doc.root.child_mut("FamilyProperties").and_then(|f| take(f, "FamilyCrest"));
    let crest_id = crest.and_then(|c| {
        let path = nonblank(c.child_text("RelativePath"))?;
        if part_candidates(&path).iter().all(|p| parts(p).is_none()) {
            report.dropped.push(format!("family crest {path} is not in the package"));
            return None;
        }
        register(c, "Photo", &mut properties)
    });
    report.media = created.len();
    if !created.is_empty() {
        let files = doc.root.ensure_child("PackagedFileCollection", FAMILY_ORDER);
        for f in created {
            files.push(f);
        }
    }
    crest_id
}

/// The app attaches the family crest to the first person of every separate tree (`FamilyCrestId`, also in the
/// person's `Multimedia`). A tree is a set of people connected by couple or parent links.
fn assign_family_crest(doc: &mut Document, crest: Option<String>) {
    let Some(crest) = crest else { return };
    let ids: Vec<String> = doc
        .root
        .child("PeopleCollection")
        .into_iter()
        .flat_map(|c| c.children_named("Person"))
        .map(|p| p.attr("Id").unwrap_or_default().into_owned())
        .collect();
    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    let mut group: Vec<usize> = (0..ids.len()).collect();
    fn root(group: &mut [usize], mut i: usize) -> usize {
        while group[i] != i {
            group[i] = group[group[i]];
            i = group[i];
        }
        i
    }
    for link in doc.root.child("RelationshipCollection").into_iter().flat_map(Element::elements) {
        let (Some(a), Some(b)) = (
            link.attr("Person1Id").and_then(|p| index.get(p.as_ref()).copied()),
            link.attr("Person2Id").and_then(|p| index.get(p.as_ref()).copied()),
        ) else {
            continue;
        };
        let (ra, rb) = (root(&mut group, a), root(&mut group, b));
        group[ra.max(rb)] = ra.min(rb);
    }
    let mut seen = HashSet::new();
    let firsts: HashSet<usize> = (0..ids.len()).filter(|&i| seen.insert(root(&mut group, i))).collect();
    if let Some(people) = doc.root.child_mut("PeopleCollection") {
        for (i, person) in people.elements_mut().filter(|p| p.name == "Person").enumerate() {
            if !firsts.contains(&i) {
                continue;
            }
            let multimedia = person.ensure_child("Multimedia", PERSON_ORDER);
            if !multimedia.children_named("string").any(|s| s.text() == crest) {
                multimedia.push(text_el("string", &crest));
            }
            set_text_in(person, "FamilyCrestId", &crest, PERSON_ORDER);
        }
    }
}

/// Build a current `PackagedFile` from an old `Photo`/`Attachment`/`AudioVideo` element.
fn packaged_file(
    mut src: Element,
    kind: &str,
    id: &str,
    path: &str,
    props: Option<Element>,
    parts: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Element {
    let windows_path = part_candidates(path)
        .into_iter()
        .find(|c| parts(c).is_some())
        .unwrap_or_else(|| path.replace('/', "\\"))
        .replace('/', "\\");
    let file_name = windows_path.rsplit('\\').next().unwrap_or(&windows_path).to_string();
    let mut file = Element::new("PackagedFile").with_attr("xsi:type", kind);
    for (k, v) in &src.attrs {
        if k != "Id" && k != "xsi:type" {
            file.attrs.push((k.clone(), v.clone()));
        }
    }
    file.set_attr("Id", id);
    if file.attr("Creation").is_none() {
        file.set_attr("Creation", &edit::utc_now());
    }
    let original = nonblank(take_text(&mut src, "OriginalFileName")).unwrap_or_else(|| windows_path.clone());
    let name = nonblank(take_text(&mut src, "FileName")).unwrap_or(file_name);
    for n in ["RelativePath", "IsTranslation", "Type", "IsAvatar"] {
        take(&mut src, n);
    }
    file.push(text_el("OriginalFileName", &original));
    file.push(text_el("RelativePath", &windows_path));
    file.push(text_el("FileName", &name));
    file.push(text_el("IsTranslation", "false"));
    file.push(text_el("Type", kind));
    file.push(file_properties(props, &name));
    // anything else (privacy, notes, citations, image metadata) is kept
    file.children.extend(src.children);
    file
}

fn file_properties(props: Option<Element>, file_name: &str) -> Element {
    let mut p = props.unwrap_or_else(|| record("Properties"));
    ensure_record_attrs(&mut p);
    const ORDER: &[&str] = &[
        "Description",
        "Title",
        "PlaceDetails",
        "Rating",
        "Date",
        "SortDate",
        "Mime",
        "MultimediaType",
        "CustomMultimediaType",
        "MediaType",
    ];
    if p.child("Title").is_none() {
        let title = file_name.rsplit_once('.').map_or(file_name, |(stem, _)| stem);
        p.ensure_child("Title", ORDER).set_text(title);
    }
    if let Some(place) = nonblank(take_text(&mut p, "Place")) {
        let details = ensure_record(&mut p, "PlaceDetails", ORDER);
        details.ensure_child("Description", &["Description"]).set_text(&place);
    }
    if let Some(taken) = take(&mut p, "DateTaken") {
        if let Some((y, m, d, ..)) = parse_datetime(&taken.text()) {
            let date = ensure_record(&mut p, "Date", ORDER);
            set_text_in(date, "StartDay", &d.to_string(), DATE_ORDER);
            set_text_in(date, "StartMonth", &m.to_string(), DATE_ORDER);
            set_text_in(date, "StartYear", &y.to_string(), DATE_ORDER);
        }
    } else if p.child("Date").is_none() {
        let fresh = record("Date");
        p.ensure_child("Date", ORDER).attrs = fresh.attrs;
    }
    if let Some(date) = p.child_mut("Date") {
        migrate_date(date);
    }
    if p.child("Rating").is_none() {
        p.ensure_child("Rating", ORDER).attrs.push(("xsi:nil".into(), "true".into()));
    }
    // MediaType (old string) -> MultimediaType (enum)
    let media_type = p.child_text("MediaType").unwrap_or_else(|| "Blank".into());
    if p.child("MultimediaType").is_none() {
        let known = [
            "Audio",
            "Book",
            "Card",
            "Digital",
            "Electronic",
            "Fiche",
            "Film",
            "Magazine",
            "Manuscript",
            "Map",
            "Newspaper",
            "Online",
            "Photo",
            "Tombstone",
            "Video",
            "Custom",
            "Blank",
        ];
        let value = known.iter().find(|k| k.eq_ignore_ascii_case(media_type.trim())).copied().unwrap_or("Custom");
        p.ensure_child("MultimediaType", ORDER).set_text(value);
    }
    if p.child("CustomMultimediaType").is_none() {
        p.ensure_child("CustomMultimediaType", ORDER);
    }
    if p.child("MediaType").is_none() {
        p.ensure_child("MediaType", ORDER).set_text(&media_type);
    }
    p
}

// ------------------------------------------------------------------ relationships

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Spouse,
    Child,
    Parent,
    Sibling,
}

#[derive(Clone)]
struct Rel {
    kind: Kind,
    /// `PersonId`: the relative.
    other: String,
    el: Element,
}

fn rel_kind(el: &Element) -> Option<Kind> {
    let ty = el.attr("xsi:type").map(|t| t.into_owned()).unwrap_or_else(|| el.name.clone());
    let ty = ty.rsplit(':').next().unwrap_or(&ty).to_string();
    let ty = if ty == "Relationship" { el.child_text("RelationshipType").unwrap_or_default() } else { ty };
    match ty.trim_end_matches("Relationship") {
        "Spouse" => Some(Kind::Spouse),
        "Child" => Some(Kind::Child),
        "Parent" => Some(Kind::Parent),
        "Sibling" => Some(Kind::Sibling),
        _ => None,
    }
}

const MODIFIERS: [&str; 7] = ["Natural", "Adopted", "Foster", "Step", "Guardian", "Sealing", "Custom"];

/// Old relations whose `ParentChildModifier` is one of these (exact, case-sensitive) are not parent links but
/// associations on the child's birth fact.
const ROLE_MODIFIERS: [&str; 3] = ["GodParent", "Sponsor", "Witness"];

fn role_modifier(el: &Element) -> Option<String> {
    let raw = el.child_text("ParentChildModifier")?;
    ROLE_MODIFIERS.contains(&raw.as_str()).then_some(raw)
}

/// The parent-link modifier the app derives from the old text: an empty string for natural; the matching
/// modifier (case-insensitive) otherwise; an unknown text counts as natural.
fn parent_modifier(el: &Element) -> String {
    let raw = el.child_text("ParentChildModifier").unwrap_or_default();
    MODIFIERS
        .iter()
        .find(|m| m.eq_ignore_ascii_case(raw.trim()))
        .filter(|m| **m != "Natural")
        .map(|m| m.to_string())
        .unwrap_or_default()
}

fn copy_rel(r: &Rel, owner: &str, kind: Kind) -> Rel {
    let mut el = r.el.clone();
    if let Some(p) = el.child_mut("PersonId") {
        p.set_text(owner);
    }
    Rel { kind, other: owner.to_string(), el }
}

fn migrate_relationships(doc: &mut Document, version: Option<f64>, report: &mut Report) {
    // 1. read every person's relationship list
    let mut ids: Vec<String> = Vec::new();
    let mut rels: Vec<Vec<Rel>> = Vec::new();
    if let Some(people) = doc.root.child("PeopleCollection") {
        for person in people.children_named("Person") {
            ids.push(person.attr("Id").unwrap_or_default().into_owned());
            let owner = ids.last().cloned().unwrap_or_default();
            let mut list = Vec::new();
            for el in person.child("Relationships").into_iter().flat_map(Element::elements) {
                match (rel_kind(el), el.child_text("PersonId")) {
                    (Some(Kind::Sibling), _) => {} // derived from shared parents; the app drops them
                    (Some(kind), Some(other)) => list.push(Rel { kind, other, el: el.clone() }),
                    _ => report.dropped.push(format!("{owner}: relationship without a kind or a person")),
                }
            }
            rels.push(list);
        }
    }
    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    // relations to people that do not exist are dropped, as the app does
    for (i, list) in rels.iter_mut().enumerate() {
        let owner = ids[i].clone();
        list.retain(|r| {
            let known = index.contains_key(r.other.as_str()) && r.other != owner;
            if !known {
                report.dropped.push(format!("relationship of {owner} to unknown person {}", r.other));
            }
            known
        });
    }

    // 2. make the relations symmetric, as the app does before pairing
    let has = |rels: &Vec<Vec<Rel>>, owner: usize, kind: Kind, other: &str| {
        rels[owner].iter().any(|r| r.kind == kind && r.other == other)
    };
    let mut additions: Vec<(usize, Rel)> = Vec::new();
    for (kind, mirror) in [(Kind::Spouse, Kind::Spouse), (Kind::Child, Kind::Parent), (Kind::Parent, Kind::Child)] {
        for (i, list) in rels.iter().enumerate() {
            for r in list.iter().filter(|r| r.kind == kind) {
                let j = index[r.other.as_str()];
                if !has(&rels, j, mirror, &ids[i])
                    && !additions.iter().any(|(k, a)| *k == j && a.kind == mirror && a.other == ids[i])
                {
                    additions.push((j, copy_rel(r, &ids[i], mirror)));
                }
            }
        }
    }
    for (j, r) in additions {
        rels[j].push(r);
    }

    // 2b. godparent/sponsor/witness relations are associations on the child's birth fact, not parent links
    //     (the app converts them from the child entries, once the lists are symmetric, and drops the entries)
    let mut associations: Vec<(usize, String, String)> = Vec::new(); // (child, associate, role)
    for (i, list) in rels.iter_mut().enumerate() {
        for r in list.iter().filter(|r| r.kind == Kind::Child) {
            if let Some(role) = role_modifier(&r.el) {
                let entry = (index[r.other.as_str()], ids[i].clone(), role);
                if !associations.contains(&entry) {
                    associations.push(entry);
                }
            }
        }
        list.retain(|r| r.kind == Kind::Spouse || role_modifier(&r.el).is_none());
    }

    // 3. couples, then parent-child links, in file order
    let mut done: HashSet<(usize, usize)> = HashSet::new();
    for i in 0..rels.len() {
        let spouses: Vec<Rel> = rels[i].iter().filter(|r| r.kind == Kind::Spouse).cloned().collect();
        for r1 in spouses {
            let j = index[r1.other.as_str()];
            if !done.insert((i.min(j), i.max(j))) {
                continue;
            }
            let r2 = rels[j]
                .iter()
                .find(|r| r.kind == Kind::Spouse && r.other == ids[i])
                .cloned()
                .unwrap_or_else(|| r1.clone());
            let Ok(link_id) = edit::link_spouse(doc, &ids[i], &ids[j]) else { continue };
            fill_couple(doc, &link_id, &r1, &r2, version, report);
            report.spouse_links += 1;
        }
    }
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for i in 0..rels.len() {
        let children: Vec<Rel> = rels[i].iter().filter(|r| r.kind == Kind::Child).cloned().collect();
        for r in children {
            let j = index[r.other.as_str()];
            if !seen.insert((i, j)) {
                continue;
            }
            if edit::link_parent(doc, &ids[j], &ids[i]).is_err() {
                report.warnings.push(format!("parent link {} -> {} skipped (cycle or duplicate)", ids[i], ids[j]));
                continue;
            }
            // The modifier is the one on the child's own `Parent` entry (what the app does; the `Child` entry
            // of the parent is only its mirror and counts when there is no `Parent` entry).
            let entry = rels[j].iter().find(|p| p.kind == Kind::Parent && p.other == ids[i]).unwrap_or(&r);
            let modifier = parent_modifier(&entry.el);
            if !modifier.is_empty() {
                let _ = edit::set_parent_modifier(doc, &ids[j], &ids[i], &modifier);
            }
            report.parent_links += 1;
        }
    }
    for (child, associate, role) in associations {
        let birth = facts::birth_fact(doc, &ids[child]);
        facts::add_association(birth, facts::association(&associate, None, &role, None));
        report.associations += 1;
    }

    // 4. drop the old lists
    if let Some(people) = doc.root.child_mut("PeopleCollection") {
        for person in people.elements_mut().filter(|p| p.name == "Person") {
            take(person, "Relationships");
        }
    }
}

/// Any text below the fact other than its tag, flags and the `Span` period marker.
fn has_content(fact: &Element) -> bool {
    fn text_below(el: &Element) -> bool {
        el.elements().any(|c| {
            !matches!(c.name.as_str(), "Tag" | "IsPrimary" | "IsFamily" | "DateDescriptor")
                && (!c.text().trim().is_empty() || text_below(c))
        })
    }
    text_below(fact)
}

/// Copy status, facts, notes and files of the two old spouse entries onto the new couple link.
fn fill_couple(doc: &mut Document, link_id: &str, r1: &Rel, r2: &Rel, version: Option<f64>, report: &mut Report) {
    let Some(link) = doc
        .root
        .child_mut("RelationshipCollection")
        .and_then(|c| c.elements_mut().find(|l| l.attr("Id").as_deref() == Some(link_id)))
    else {
        return;
    };
    const ORDER: &[&str] = &["Facts", "SpouseRelationshipStatus", "NumberOfChildren", "Notes", "Multimedia"];
    let status = nonblank(r1.el.child_text("SpouseRelationshipStatus"))
        .or_else(|| nonblank(r1.el.child_text("SpouseModifier")))
        .and_then(|s| {
            [
                "Engaged",
                "Current",
                "Separated",
                "DivorceFiled",
                "Deceased",
                "Former",
                "Divorced",
                "Annulled",
                "Dissolved",
            ]
            .iter()
            .find(|k| k.eq_ignore_ascii_case(s.trim()))
            .map(|k| k.to_string())
        });
    if let Some(status) = status {
        set_text_in(link, "SpouseRelationshipStatus", &status, ORDER);
    }

    let mut facts: Vec<Element> =
        r1.el.child("Facts").into_iter().flat_map(|f| f.children_named("Fact").cloned()).collect();
    for (field, tag) in [("Marriage", "MARR"), ("Divorce", "DIV")] {
        if let Some(mut f) = r1.el.child(field).cloned() {
            f.name = "Fact".into();
            if f.child_text("Tag").is_none_or(|t| t.is_empty()) {
                set_text_in(&mut f, "Tag", tag, FACT_ORDER);
            }
            if f.child_text("IsPrimary").is_none() {
                set_text_in(&mut f, "IsPrimary", "true", FACT_ORDER);
            }
            // an empty placeholder (no date, place, details or notes) is not a fact
            if has_content(&f) {
                facts.push(f);
            }
        }
    }
    if let Some(n) = r1.el.child_text("NumberOfChildren").and_then(|n| n.trim().parse::<u32>().ok()) {
        let mut f = record("Fact");
        f.push(text_el("Tag", "NCHI"));
        f.push(text_el("NumberDetails", &n.to_string()));
        facts.push(f);
    }
    for f in &mut facts {
        migrate_fact(f, version, report);
    }
    // Sealing facts of the couple. The app's own conversion loses these when it converts a file older than
    // 11.1 (a later step removes them); they are family facts here.
    let mut sealings: Vec<Element> = Vec::new();
    for r in [r1, r2] {
        for old in r.el.child("LDSFacts").into_iter().flat_map(Element::elements) {
            let fact = facts::lds_fact(old, true, version, report);
            // both partners carry a copy of the same sealing
            if sealings.iter().any(|s| facts::same_content(s, &fact)) {
                report.facts -= 1;
            } else {
                sealings.push(fact);
            }
        }
    }
    facts.extend(sealings);
    if !facts.is_empty() {
        let list = link.ensure_child("Facts", ORDER);
        for mut f in facts {
            set_text_in(&mut f, "IsFamily", "true", FACT_ORDER);
            list.push(f);
        }
    }

    // notes and files of both directions, without duplicates
    let mut notes: Vec<Element> = Vec::new();
    for r in [r1, r2] {
        for n in r.el.path("Notes/Notes").into_iter().flat_map(|n| n.children_named("Note")) {
            let text = n.child_text("Text").unwrap_or_default();
            if !notes.iter().any(|k| k.child_text("Text").unwrap_or_default() == text) {
                notes.push(n.clone());
            }
        }
    }
    if !notes.is_empty() {
        report.notes += notes.len();
        let wrapper = link.ensure_child("Notes", ORDER);
        let list = wrapper.ensure_child("Notes", &["Notes", "Index"]);
        for n in notes {
            list.push(n);
        }
        wrapper.ensure_child("Index", &["Notes", "Index"]).set_text("0");
    }
    let mut files: Vec<String> = Vec::new();
    for r in [r1, r2] {
        for s in r.el.child("Multimedia").into_iter().flat_map(|m| m.children_named("string")) {
            if !files.contains(&s.text()) {
                files.push(s.text());
            }
        }
    }
    if !files.is_empty() {
        let list = link.ensure_child("Multimedia", ORDER);
        for f in files {
            list.push(text_el("string", &f));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact_date(inner: &str) -> Element {
        Document::parse(format!("<FactDate>{inner}</FactDate>").as_bytes()).unwrap().root
    }

    #[test]
    fn guid_ids_use_the_dotnet_byte_order() {
        // Guid.ToByteArray() of 00000001-0002-0003-0405-060708090a0b, base64url without padding
        assert_eq!(guid_to_id("00000001-0002-0003-0405-060708090a0b").as_deref(), Some("AQAAAAIAAwAEBQYHCAkKCw"));
        assert_eq!(guid_to_id("AQAAAAIAAwAEBQYHCAkKCw"), None);
        assert_eq!(guid_to_id("0000000x-0002-0003-0405-060708090a0b"), None);
    }

    #[test]
    fn old_dates_are_converted() {
        let mut d = fact_date(
            "<StartDate>1901-05-03T10:30:00</StartDate><Descriptor>about</Descriptor><IsStartBC>true</IsStartBC>",
        );
        migrate_date(&mut d);
        assert_eq!(d.child_text("StartYear").as_deref(), Some("1901"));
        assert_eq!(d.child_text("StartTime").as_deref(), Some("0001-01-01T10:30:00"));
        assert_eq!(d.child_text("StartEra").as_deref(), Some("0"));
        assert_eq!(d.child_text("DateDescriptor").as_deref(), Some("About"));
        assert!(d.child("StartDate").is_none() && d.child("Descriptor").is_none());

        // the app's conversion (differential run): a range is a Span, a "Between" range a Range; an open end
        // does not make it FromOnly/ToOnly
        let mut d = fact_date("<EndDate>1950-01-02T00:00:00</EndDate><IsDateRange>true</IsDateRange>");
        migrate_date(&mut d);
        assert_eq!(d.child_text("DateDescriptor").as_deref(), Some("Span"));
        let mut d = fact_date(
            "<StartDate>1850-01-02T00:00:00</StartDate><EndDate>1860-03-04T00:00:00</EndDate>\
             <IsDateRange>true</IsDateRange><StartDateDescriptor>Between</StartDateDescriptor>",
        );
        migrate_date(&mut d);
        assert_eq!(d.child_text("DateDescriptor").as_deref(), Some("Range"));
        // without a range the descriptor is the enum value, an unknown one is exact
        let mut d = fact_date("<StartDate>1850-01-02T00:00:00</StartDate><Descriptor>From</Descriptor>");
        migrate_date(&mut d);
        assert_eq!(d.child_text("DateDescriptor").as_deref(), Some("FromOnly"));
        let mut d = fact_date("<StartDate>1850-01-02T00:00:00</StartDate><Descriptor>Between</Descriptor>");
        migrate_date(&mut d);
        assert!(d.child("DateDescriptor").is_none());
        // the children come out in the app's order
        let mut d = fact_date(
            "<StartDate>1850-01-02T10:00:00</StartDate><EndDate>1860-03-04T00:00:00</EndDate>\
             <IsEndBC>true</IsEndBC><CalendarType>JulianCalendar</CalendarType><Descriptor>Before</Descriptor>",
        );
        migrate_date(&mut d);
        let names: Vec<&str> = d.elements().map(|e| e.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "StartDay",
                "StartMonth",
                "StartYear",
                "EndDay",
                "EndMonth",
                "EndYear",
                "StartTime",
                "DateDescriptor",
                "StartCalendar",
                "EndCalendar",
                "EndEra"
            ]
        );
        // a calendar the app does not have is ignored rather than written
        let mut d = fact_date("<StartDate>1850-01-02T00:00:00</StartDate><CalendarType>MartianCalendar</CalendarType>");
        migrate_date(&mut d);
        assert!(d.child("StartCalendar").is_none());

        // already current: untouched
        let mut d = fact_date("<StartDay>1</StartDay><StartMonth>2</StartMonth><StartYear>1900</StartYear>");
        let before = d.clone();
        migrate_date(&mut d);
        assert_eq!(d, before);
    }
}

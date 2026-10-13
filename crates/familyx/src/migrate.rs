//! Schema migration of older `.familyx` trees.
//!
//! Versions of the app before 11.3 serialized relationships per person
//! (`Person/Relationships`), names as `Person/Name` + `Person/AlternativeName`,
//! photos as `Person/Photos`, and so on. The current app reads such files into
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
use crate::edit::{self, record, text_el, FACT_ORDER, FAMILY_ORDER, PERSON_ORDER};

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
    /// Old data that was kept but could not be converted (see docs/design.md).
    pub warnings: Vec<String>,
}

/// True if the tree still contains elements of the pre-link schema.
pub fn has_legacy_elements(doc: &Document) -> bool {
    doc.root.elements().any(|e| LEGACY_ROOT.contains(&e.name.as_str()))
        || doc
            .root
            .child("PeopleCollection")
            .is_some_and(|c| c.elements().any(|p| p.elements().any(|e| LEGACY_PERSON.contains(&e.name.as_str()))))
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

    normalize_ids(doc);
    move_author(doc);
    migrate_media(doc, parts, &mut report);
    migrate_people(doc, parts, &mut report);
    migrate_relationships(doc, &mut report);
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

const DATE_ORDER: &[&str] = &[
    "StartDay",
    "StartMonth",
    "StartYear",
    "StartEra",
    "StartCalendar",
    "EndDay",
    "EndMonth",
    "EndYear",
    "EndEra",
    "EndCalendar",
    "DateDescriptor",
    "StartTime",
    "EndTime",
    "Text",
];
const DESCRIPTORS: [&str; 13] = [
    "Exact",
    "About",
    "Estimated",
    "Calculated",
    "Before",
    "After",
    "Range",
    "Span",
    "FromOnly",
    "ToOnly",
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

/// Old `CustomDate` members (`StartDate`, `EndDate`, `Descriptor`, `IsTextDate`,
/// `IsDateRange`, `IsStartBC`, `IsEndBC`, `CalendarType`) become the current
/// `Start*`/`End*` fields and a `DateDescriptor`.
fn migrate_date(date: &mut Element) {
    let start = take(date, "StartDate");
    let end = take(date, "EndDate");
    let descriptor = take_text(date, "Descriptor");
    let _ = (take(date, "StartDateDescriptor"), take(date, "EndDateDescriptor"));
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
    if let Some(cal) = nonblank(calendar).filter(|c| c != "GregorianCalendar") {
        set_text_in(date, "StartCalendar", &cal, DATE_ORDER);
        set_text_in(date, "EndCalendar", &cal, DATE_ORDER);
    }
    let kind = if is_true(&text_date) {
        "TextOnly".to_string()
    } else if is_true(&range) {
        match (start.is_some(), end.is_some()) {
            (true, false) => "FromOnly".into(),
            (false, true) => "ToOnly".into(),
            _ => "Range".into(),
        }
    } else {
        descriptor
            .and_then(|d| DESCRIPTORS.iter().find(|k| k.eq_ignore_ascii_case(d.trim())).map(|k| k.to_string()))
            .unwrap_or_else(|| "Exact".into())
    };
    if kind != "Exact" {
        set_text_in(date, "DateDescriptor", &kind, DATE_ORDER);
    }
}

// ------------------------------------------------------------------ facts

/// Bring one `Fact` (person or couple) to the current shape.
fn migrate_fact(fact: &mut Element, report: &mut Report) {
    ensure_record_attrs(fact);
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

fn migrate_people(doc: &mut Document, parts: &dyn Fn(&str) -> Option<Vec<u8>>, report: &mut Report) {
    let Some(people) = doc.root.child_mut("PeopleCollection") else { return };
    for person in people.elements_mut().filter(|p| p.name == "Person") {
        migrate_person_names(person, report);
        if let Some(facts) = person.child_mut("Facts") {
            for fact in facts.elements_mut().filter(|f| f.name == "Fact") {
                migrate_fact(fact, report);
            }
        }
        migrate_contact_address(person);
        migrate_story(person, parts, report);
        if let Some(biology) = person.child_mut("Biology") {
            migrate_note_string(biology, &["Gender", "Notes"], report);
        }
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
        let order = ["Names", "SurnamePrefix", "Surname", "Suffix", "Nickname", "Notes", "IsPrimary", "NameType"];
        if primary && !existing_primary {
            set_text_in(&mut name, "IsPrimary", "true", &order);
        } else if name.child_text("IsPrimary").as_deref() == Some("true") {
            // an alternative name is never the primary one
            set_text_in(&mut name, "IsPrimary", "false", &order);
        }
        migrate_note_string(&mut name, &order, report);
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

/// `Story` pointed at a file with the person's story; it becomes a story note.
fn migrate_story(person: &mut Element, parts: &dyn Fn(&str) -> Option<Vec<u8>>, report: &mut Report) {
    let Some(story) = person.child("Story") else { return };
    let person_id = person.attr("Id").unwrap_or_default().into_owned();
    let mut remaining = story.clone();
    let mut notes = Vec::new();
    if let Some(text) = nonblank(take_text(&mut remaining, "GedcomImportStoryText")) {
        notes.push((text, "text/plain".to_string()));
    }
    if let Some(path) = nonblank(take_text(&mut remaining, "RelativePath")) {
        let found = part_candidates(&path).into_iter().find_map(|c| parts(&c));
        match found {
            Some(bytes) => {
                let text = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string();
                let rtf = path.to_ascii_lowercase().ends_with(".rtf") || text.starts_with("{\\rtf");
                notes.push((text, if rtf { "text/richtext" } else { "text/plain" }.to_string()));
            }
            None => {
                report.warnings.push(format!("story file {path} of {person_id} is not in the package"));
                remaining.children.push(Node::Element(text_el("RelativePath", &path)));
            }
        }
    }
    for (text, format) in notes {
        push_note(person, PERSON_ORDER, &text, &format, true);
        report.notes += 1;
    }
    let leftover = remaining.elements().any(|e| !e.text().trim().is_empty());
    if leftover {
        report.warnings.push(format!("story of {person_id} kept as is (database-backed or file missing)"));
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
/// referenced from `Person/Multimedia`; the avatar becomes `PrimaryImageId`.
fn migrate_media(doc: &mut Document, parts: &dyn Fn(&str) -> Option<Vec<u8>>, report: &mut Report) {
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
    report.media = created.len();
    if !created.is_empty() {
        let files = doc.root.ensure_child("PackagedFileCollection", FAMILY_ORDER);
        for f in created {
            files.push(f);
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

/// `Some("")` for natural, `Some(name)` for a modifier the current app knows,
/// `None` for anything else (old godparent/sponsor/witness roles, which the app
/// turns into associations instead of parent links).
fn parent_modifier(el: &Element) -> Option<String> {
    let raw = el.child_text("ParentChildModifier").unwrap_or_default();
    let raw = raw.trim();
    if raw.is_empty() {
        return Some(String::new());
    }
    MODIFIERS.iter().find(|m| m.eq_ignore_ascii_case(raw)).map(|m| {
        if *m == "Natural" {
            String::new()
        } else {
            m.to_string()
        }
    })
}

fn copy_rel(r: &Rel, owner: &str, kind: Kind) -> Rel {
    let mut el = r.el.clone();
    if let Some(p) = el.child_mut("PersonId") {
        p.set_text(owner);
    }
    Rel { kind, other: owner.to_string(), el }
}

fn migrate_relationships(doc: &mut Document, report: &mut Report) {
    // 1. read every person's relationship list
    let mut ids: Vec<String> = Vec::new();
    let mut rels: Vec<Vec<Rel>> = Vec::new();
    let mut leftover: Vec<Vec<Element>> = Vec::new();
    if let Some(people) = doc.root.child("PeopleCollection") {
        for person in people.children_named("Person") {
            ids.push(person.attr("Id").unwrap_or_default().into_owned());
            let mut list = Vec::new();
            let mut rest = Vec::new();
            for el in person.child("Relationships").into_iter().flat_map(Element::elements) {
                match (rel_kind(el), el.child_text("PersonId")) {
                    (Some(Kind::Sibling), _) => {} // derived from shared parents; the app drops them
                    (Some(kind), Some(other)) => list.push(Rel { kind, other, el: el.clone() }),
                    _ => rest.push(el.clone()),
                }
            }
            rels.push(list);
            leftover.push(rest);
        }
    }
    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();

    // unconvertible entries stay in a trimmed `Relationships` element
    let keep = |r: &Rel, report: &mut Report, owner: &str| -> bool {
        let known = index.contains_key(r.other.as_str()) && r.other != owner;
        if !known {
            report.warnings.push(format!("relationship of {owner} to unknown person {} kept as is", r.other));
            return false;
        }
        if r.kind != Kind::Spouse && parent_modifier(&r.el).is_none() {
            report.warnings.push(format!("{owner}: {} role kept as is (no association support)", r.other));
            return false;
        }
        true
    };
    for (i, list) in rels.iter_mut().enumerate() {
        let owner = ids[i].clone();
        let (good, bad): (Vec<Rel>, Vec<Rel>) = std::mem::take(list).into_iter().partition(|r| keep(r, report, &owner));
        *list = good;
        leftover[i].extend(bad.into_iter().map(|r| r.el));
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
            fill_couple(doc, &link_id, &r1, &r2, report);
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
            if let Some(modifier) = parent_modifier(&r.el).filter(|m| !m.is_empty()) {
                let _ = edit::set_parent_modifier(doc, &ids[j], &ids[i], &modifier);
            }
            report.parent_links += 1;
        }
    }

    // 4. drop the old lists, keeping what could not be converted
    if let Some(people) = doc.root.child_mut("PeopleCollection") {
        for (person, rest) in people.elements_mut().filter(|p| p.name == "Person").zip(leftover) {
            take(person, "Relationships");
            if !rest.is_empty() {
                let mut kept = Element::new("Relationships");
                kept.children = rest.into_iter().map(Node::Element).collect();
                person.push(kept);
            }
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
fn fill_couple(doc: &mut Document, link_id: &str, r1: &Rel, r2: &Rel, report: &mut Report) {
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
    if !facts.is_empty() {
        let list = link.ensure_child("Facts", ORDER);
        for mut f in facts {
            migrate_fact(&mut f, report);
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

        let mut d = fact_date("<EndDate>1950-01-02T00:00:00</EndDate><IsDateRange>true</IsDateRange>");
        migrate_date(&mut d);
        assert_eq!(d.child_text("DateDescriptor").as_deref(), Some("ToOnly"));

        // already current: untouched
        let mut d = fact_date("<StartDay>1</StartDay><StartMonth>2</StartMonth><StartYear>1900</StartYear>");
        let before = d.clone();
        migrate_date(&mut d);
        assert_eq!(d, before);
    }
}

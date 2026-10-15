//! Gramps XML 1.7.1 (`.gramps`, gzip-compressed) export and import.
//!
//! Places are written flat (the whole place string as one `pname`); on import
//! a place hierarchy is joined back into `Name, Parent, Country`. Sources,
//! repositories and citations (`citationref` on people and events) are mapped;
//! a citation's confidence is not.

use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};

use familyx::doc::{escape, Document, Element};
use familyx::edit::{self, FactInput, NameExtras};
use familyx::model::{Citation, Fact, Gender, Tree};
use familyx::Package;

use crate::dates::{self, DateValue, Kind};
use crate::families::{self, Fam};
use crate::vocab::{self, GRAMPS_TYPES};
use crate::{Dropped, Error, Findings, ImportReport, Result};

const NS: &str = "http://gramps-project.org/xml/1.7.1/";

// ---------------------------------------------------------------- export

fn text(s: &str) -> String {
    escape(s, false)
}

fn attr(s: &str) -> String {
    escape(s, true)
}

fn gramps_type(fact: &Fact) -> String {
    match GRAMPS_TYPES.iter().find(|(t, _)| *t == fact.tag) {
        Some((_, ty)) => (*ty).to_string(),
        // a user-named event keeps its name as the Gramps custom type
        None if fact.tag == "EVEN" && fact.custom_tag.is_empty() => "Event".to_string(),
        None => fact.label().to_string(),
    }
}

fn gramps_name_type(ty: &str) -> String {
    match ty {
        "" => "Also Known As".into(),
        "Birth" => "Birth Name".into(),
        "Aka" => "Also Known As".into(),
        "Married" => "Married Name".into(),
        other => other.to_string(),
    }
}

/// Gramps `frel` / `mrel` for a parent-link modifier.
fn child_rel(modifier: &str) -> &'static str {
    match modifier {
        "" | "Natural" => "Birth",
        "Adopted" => "Adopted",
        "Foster" => "Foster",
        "Step" => "Stepchild",
        _ => "Unknown",
    }
}

struct Event<'a> {
    fact: &'a Fact,
}

pub(crate) fn export(pkg: &Package, media: &crate::ExportMedia) -> Result<Vec<u8>> {
    let tree = Tree::from_doc(&pkg.doc);
    let fams = families::build(&tree);
    let change = chrono::Utc::now().timestamp();

    // events: one per fact; places: one per distinct place string
    let mut events: Vec<Event> = Vec::new();
    let person_events: Vec<Vec<usize>> = tree
        .people
        .iter()
        .map(|p| {
            p.facts
                .iter()
                .map(|fact| {
                    events.push(Event { fact });
                    events.len() - 1
                })
                .collect()
        })
        .collect();
    let fam_events: Vec<Vec<usize>> = fams
        .iter()
        .map(|f| {
            f.link
                .iter()
                .flat_map(|l| &l.facts)
                .map(|fact| {
                    events.push(Event { fact });
                    events.len() - 1
                })
                .collect()
        })
        .collect();
    let mut places: Vec<&str> = Vec::new();
    for e in events.iter().filter(|e| !e.fact.place.is_empty()) {
        if !places.contains(&e.fact.place.as_str()) {
            places.push(&e.fact.place);
        }
    }

    // citation id -> number of its `_c<n>` object (citations of unknown sources are left out)
    let source_numbers: HashMap<&str, usize> =
        tree.sources.iter().enumerate().map(|(i, s)| (s.id.as_str(), i + 1)).collect();
    let mut all_citations: Vec<&Citation> = Vec::new();
    let mut citation_numbers: HashMap<&str, usize> = HashMap::new();
    let owners =
        tree.people.iter().flat_map(|p| std::iter::once(&p.citations).chain(p.facts.iter().map(|f| &f.citations)));
    let couple_facts = fams.iter().flat_map(|f| f.link.iter().flat_map(|l| &l.facts)).map(|f| &f.citations);
    for c in owners.chain(couple_facts).flatten().filter(|c| source_numbers.contains_key(c.source.as_str())) {
        all_citations.push(c);
        citation_numbers.insert(&c.id, all_citations.len());
    }
    let citation_refs = |citations: &[Citation], indent: &str| -> String {
        citations
            .iter()
            .filter_map(|c| citation_numbers.get(c.id.as_str()))
            .map(|n| format!("{indent}<citationref hlink=\"_c{n}\"/>\n"))
            .collect()
    };
    let person_handle = |id: &str| tree.people.iter().position(|p| p.id == id).map(|i| format!("_i{}", i + 1));
    // Gramps notes: (text, type); person notes first, then source notes
    let mut notes: Vec<(String, &str)> = Vec::new();
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<!DOCTYPE database PUBLIC \"-//Gramps//DTD Gramps XML 1.7.1//EN\"\n");
    out.push_str("\"http://gramps-project.org/xml/1.7.1/grampsxml.dtd\">\n");
    out.push_str(&format!("<database xmlns=\"{NS}\">\n"));
    out.push_str(&format!(
        "  <header>\n    <created date=\"{}\" version=\"5.1.0\"/>\n    <researcher/>\n  </header>\n",
        chrono::Utc::now().format("%Y-%m-%d")
    ));

    if !events.is_empty() {
        out.push_str("  <events>\n");
        for (i, e) in events.iter().enumerate() {
            out.push_str(&format!("    <event handle=\"_e{0}\" id=\"E{0:04}\" change=\"{change}\">\n", i + 1));
            out.push_str(&format!("      <type>{}</type>\n", text(&gramps_type(e.fact))));
            let date = dates::from_model(&e.fact.date).to_gramps_xml();
            if !date.is_empty() {
                out.push_str(&format!("      {date}\n"));
            }
            if let Some(p) = places.iter().position(|p| *p == e.fact.place) {
                out.push_str(&format!("      <place hlink=\"_pl{}\"/>\n", p + 1));
            }
            if !e.fact.details.is_empty() {
                out.push_str(&format!("      <description>{}</description>\n", text(&e.fact.details)));
            }
            out.push_str(&citation_refs(&e.fact.citations, "      "));
            out.push_str("    </event>\n");
        }
        out.push_str("  </events>\n");
    }

    out.push_str("  <people>\n");
    for (i, person) in tree.people.iter().enumerate() {
        out.push_str(&format!("    <person handle=\"_i{0}\" id=\"I{0:04}\" change=\"{change}\">\n", i + 1));
        let gender = match person.gender {
            Gender::Male => "M",
            Gender::Female => "F",
            Gender::Other | Gender::Unknown => "U",
        };
        out.push_str(&format!("      <gender>{gender}</gender>\n"));
        let primary = person.primary_name();
        for name in primary.into_iter().chain(person.names.iter().filter(|n| Some(*n) != primary)) {
            let is_alt = Some(name) != primary;
            let ty = if is_alt { gramps_name_type(&name.name_type) } else { "Birth Name".into() };
            let alt = if is_alt { " alt=\"1\"" } else { "" };
            out.push_str(&format!("      <name{alt} type=\"{}\">\n", attr(&ty)));
            if !name.given.is_empty() {
                out.push_str(&format!("        <first>{}</first>\n", text(&name.given)));
            }
            if !name.surname.is_empty() || !name.surname_prefix.is_empty() {
                let prefix = if name.surname_prefix.is_empty() {
                    String::new()
                } else {
                    format!(" prefix=\"{}\"", attr(&name.surname_prefix))
                };
                out.push_str(&format!("        <surname{prefix}>{}</surname>\n", text(&name.surname)));
            }
            if !name.suffix.is_empty() {
                out.push_str(&format!("        <suffix>{}</suffix>\n", text(&name.suffix)));
            }
            if !name.title.is_empty() {
                out.push_str(&format!("        <title>{}</title>\n", text(&name.title)));
            }
            if !name.nickname.is_empty() {
                out.push_str(&format!("        <nick>{}</nick>\n", text(&name.nickname)));
            }
            out.push_str("      </name>\n");
        }
        for e in &person_events[i] {
            out.push_str(&format!("      <eventref hlink=\"_e{}\" role=\"Primary\"/>\n", e + 1));
        }
        let photos = person
            .primary_image
            .iter()
            .chain(person.media.iter().filter(|m| Some(*m) != person.primary_image.as_ref()));
        for m in photos {
            if let Some(r) = media.refs.get(m) {
                out.push_str(&format!("      <objref hlink=\"_o{}\"/>\n", r.number));
            }
        }
        for (f, fam) in fams.iter().enumerate() {
            if fam.children.iter().any(|c| c.id == person.id) {
                out.push_str(&format!("      <childof hlink=\"_f{}\"/>\n", f + 1));
            }
        }
        for (f, fam) in fams.iter().enumerate() {
            if fam.husb == Some(&person.id) || fam.wife == Some(&person.id) {
                out.push_str(&format!("      <parentin hlink=\"_f{}\"/>\n", f + 1));
            }
        }
        for note in person.notes.iter().map(|n| n.plain_text()).filter(|n| !n.trim().is_empty()) {
            notes.push((note, "Person Note"));
            out.push_str(&format!("      <noteref hlink=\"_n{}\"/>\n", notes.len()));
        }
        out.push_str(&citation_refs(&person.citations, "      "));
        out.push_str("    </person>\n");
    }
    out.push_str("  </people>\n");

    if !fams.is_empty() {
        out.push_str("  <families>\n");
        for (f, fam) in fams.iter().enumerate() {
            write_family(&mut out, f + 1, fam, &fam_events[f], &person_handle, change);
        }
        out.push_str("  </families>\n");
    }

    if !all_citations.is_empty() {
        out.push_str("  <citations>\n");
        for (i, c) in all_citations.iter().enumerate() {
            out.push_str(&format!("    <citation handle=\"_c{0}\" id=\"C{0:04}\" change=\"{change}\">\n", i + 1));
            if !c.page.is_empty() {
                out.push_str(&format!("      <page>{}</page>\n", text(&c.page)));
            }
            // required by the DTD; 2 is Gramps' "Normal"
            out.push_str("      <confidence>2</confidence>\n");
            out.push_str(&format!("      <sourceref hlink=\"_s{}\"/>\n", source_numbers[c.source.as_str()]));
            out.push_str("    </citation>\n");
        }
        out.push_str("  </citations>\n");
    }
    if !tree.sources.is_empty() {
        out.push_str("  <sources>\n");
        for (i, src) in tree.sources.iter().enumerate() {
            out.push_str(&format!("    <source handle=\"_s{0}\" id=\"S{0:04}\" change=\"{change}\">\n", i + 1));
            // DTD order: stitle, sauthor, spubinfo, sabbrev
            for (tag, value) in [
                ("stitle", &src.title),
                ("sauthor", &src.author),
                ("spubinfo", &src.publisher),
                ("sabbrev", &src.abbreviation),
            ] {
                if !value.is_empty() {
                    out.push_str(&format!("      <{tag}>{}</{tag}>\n", text(value)));
                }
            }
            for note in &src.notes {
                notes.push((note.plain_text(), "Source Note"));
                out.push_str(&format!("      <noteref hlink=\"_n{}\"/>\n", notes.len()));
            }
            if let Some(r) = src.repository.as_deref().and_then(|r| tree.repositories.iter().position(|x| x.id == r)) {
                let callno = if src.call_number.is_empty() {
                    String::new()
                } else {
                    format!(" callno=\"{}\"", attr(&src.call_number))
                };
                out.push_str(&format!("      <reporef hlink=\"_r{}\"{callno} medium=\"Unknown\"/>\n", r + 1));
            }
            out.push_str("    </source>\n");
        }
        out.push_str("  </sources>\n");
    }

    if !places.is_empty() {
        out.push_str("  <places>\n");
        for (i, place) in places.iter().enumerate() {
            out.push_str(&format!(
                "    <placeobj handle=\"_pl{0}\" id=\"P{0:04}\" change=\"{change}\" type=\"Unknown\">\n",
                i + 1
            ));
            out.push_str(&format!("      <ptitle>{}</ptitle>\n", text(place)));
            out.push_str(&format!("      <pname value=\"{}\"/>\n", attr(place)));
            out.push_str("    </placeobj>\n");
        }
        out.push_str("  </places>\n");
    }
    if !media.order.is_empty() {
        out.push_str("  <objects>\n");
        for id in &media.order {
            let r = &media.refs[id];
            let mime = if r.ext == "png" { "image/png" } else { "image/jpeg" };
            out.push_str(&format!("    <object handle=\"_o{0}\" id=\"O{0:04}\" change=\"{change}\">\n", r.number));
            out.push_str(&format!(
                "      <file src=\"{}\" mime=\"{mime}\" description=\"{}\"/>\n",
                attr(&r.path),
                attr(&r.title)
            ));
            out.push_str("    </object>\n");
        }
        out.push_str("  </objects>\n");
    }
    if !tree.repositories.is_empty() {
        out.push_str("  <repositories>\n");
        for (i, r) in tree.repositories.iter().enumerate() {
            out.push_str(&format!("    <repository handle=\"_r{0}\" id=\"R{0:04}\" change=\"{change}\">\n", i + 1));
            out.push_str(&format!("      <rname>{}</rname>\n      <type>Unknown</type>\n", text(&r.title)));
            out.push_str("    </repository>\n");
        }
        out.push_str("  </repositories>\n");
    }
    if !notes.is_empty() {
        out.push_str("  <notes>\n");
        for (i, (note, ty)) in notes.iter().enumerate() {
            out.push_str(&format!(
                "    <note handle=\"_n{0}\" id=\"N{0:04}\" type=\"{ty}\" change=\"{change}\">\n      <text>{1}</text>\n    </note>\n",
                i + 1,
                text(note)
            ));
        }
        out.push_str("  </notes>\n");
    }
    out.push_str("</database>\n");

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(out.as_bytes())?;
    Ok(gz.finish()?)
}

fn write_family(
    out: &mut String,
    number: usize,
    fam: &Fam,
    events: &[usize],
    person_handle: &dyn Fn(&str) -> Option<String>,
    change: i64,
) {
    out.push_str(&format!("    <family handle=\"_f{number}\" id=\"F{number:04}\" change=\"{change}\">\n"));
    let rel = fam
        .link
        .and_then(|l| l.facts.iter().find(|f| familyx::edit::is_marriage_tag(&f.tag)))
        .map_or("Unknown", |f| vocab::gramps_relationship(&f.tag));
    out.push_str(&format!("      <rel type=\"{rel}\"/>\n"));
    for (tag, id) in [("father", fam.husb), ("mother", fam.wife)] {
        if let Some(h) = id.and_then(person_handle) {
            out.push_str(&format!("      <{tag} hlink=\"{h}\"/>\n"));
        }
    }
    for e in events {
        out.push_str(&format!("      <eventref hlink=\"_e{}\" role=\"Family\"/>\n", e + 1));
    }
    for child in &fam.children {
        if let Some(h) = person_handle(child.id) {
            out.push_str(&format!(
                "      <childref hlink=\"{h}\" frel=\"{}\" mrel=\"{}\"/>\n",
                child_rel(child.husb_mod),
                child_rel(child.wife_mod)
            ));
        }
    }
    out.push_str("    </family>\n");
}

// ---------------------------------------------------------------- import

struct GEvent {
    ty: String,
    date: DateValue,
    place: Option<String>,
    description: String,
    /// citation handles
    citations: Vec<String>,
}

struct GPlace {
    name: String,
    title: String,
    parent: Option<String>,
}

struct Importer<'a> {
    pkg: Package,
    resolve: crate::Resolve<'a>,
    /// object handle -> file reference
    objects: HashMap<String, String>,
    dropped: Dropped,
    warnings: Vec<String>,
    events: HashMap<String, GEvent>,
    places: HashMap<String, GPlace>,
    people: HashMap<String, String>,
    /// source handle -> source id
    sources: HashMap<String, String>,
    /// citation handle -> (source id, page)
    citations: HashMap<String, (String, String)>,
    /// note handle -> text, and the notes that were attached to something
    notes: HashMap<String, String>,
    used_notes: HashSet<String>,
}

fn handle(el: &Element) -> String {
    el.attr("hlink").map(|h| h.trim_start_matches('_').to_string()).unwrap_or_default()
}

/// Gramps handles may or may not start with `_`; compare without it.
fn key(h: &str) -> String {
    h.trim_start_matches('_').to_string()
}

fn gunzip_if_needed(bytes: &[u8]) -> Result<Vec<u8>> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes).read_to_end(&mut out).map_err(|e| Error::Gramps(format!("gzip: {e}")))?;
        Ok(out)
    } else {
        Ok(bytes.to_vec())
    }
}

fn parse_date(ev: &Element) -> DateValue {
    for el in ev.elements() {
        let a = |n: &str| el.attr(n).map(|v| v.into_owned()).unwrap_or_default();
        match el.name.as_str() {
            "dateval" => {
                // `quality` says how the date was found; it wins over a plain "about"
                let kind = match (a("type").as_str(), a("quality").as_str()) {
                    ("before", _) => Kind::Before,
                    ("after", _) => Kind::After,
                    (_, "estimated") => Kind::Estimated,
                    (_, "calculated") => Kind::Calculated,
                    ("about", _) => Kind::About,
                    _ => Kind::Exact,
                };
                return DateValue { kind, start: dates::parse_gramps_ymd(&a("val")), ..Default::default() };
            }
            "daterange" | "datespan" => {
                let kind = if el.name == "daterange" { Kind::Range } else { Kind::Span };
                let (start, end) = (dates::parse_gramps_ymd(&a("start")), dates::parse_gramps_ymd(&a("stop")));
                return DateValue { kind, start, end, ..Default::default() };
            }
            "datestr" => return DateValue::from_gedcom(&a("val")),
            _ => {}
        }
    }
    DateValue::default()
}

pub fn import(bytes: &[u8], resolve: crate::Resolve) -> Result<(Package, ImportReport)> {
    let xml = gunzip_if_needed(bytes)?;
    let doc = Document::parse(&xml).map_err(|e| Error::Gramps(e.to_string()))?;
    let root = &doc.root;
    if root.name != "database" {
        return Err(Error::Gramps(format!("unexpected root element <{}>, expected <database>", root.name)));
    }
    let mut imp = Importer {
        pkg: Package::new(),
        resolve,
        objects: HashMap::new(),
        dropped: Dropped::default(),
        warnings: Vec::new(),
        events: HashMap::new(),
        places: HashMap::new(),
        people: HashMap::new(),
        sources: HashMap::new(),
        citations: HashMap::new(),
        notes: HashMap::new(),
        used_notes: HashSet::new(),
    };

    for ev in root.child("events").into_iter().flat_map(|c| c.children_named("event")) {
        let text = |n: &str| ev.child_text(n).unwrap_or_default();
        imp.events.insert(
            key(&ev.attr("handle").unwrap_or_default()),
            GEvent {
                ty: text("type"),
                date: parse_date(ev),
                place: ev.child("place").map(handle),
                description: text("description"),
                citations: ev.children_named("citationref").map(handle).collect(),
            },
        );
        for c in ev.elements() {
            if !["type", "dateval", "daterange", "datespan", "datestr", "place", "description", "citationref"]
                .contains(&c.name.as_str())
            {
                imp.dropped.add(format!("event.{}", c.name));
            }
        }
    }
    for p in root.child("places").into_iter().flat_map(|c| c.children_named("placeobj")) {
        let name = p.child("pname").and_then(|n| n.attr("value")).map(|v| v.into_owned()).unwrap_or_default();
        imp.places.insert(
            key(&p.attr("handle").unwrap_or_default()),
            GPlace { name, title: p.child_text("ptitle").unwrap_or_default(), parent: p.child("placeref").map(handle) },
        );
    }

    for o in root.child("objects").into_iter().flat_map(|c| c.children_named("object")) {
        if let Some(src) = o.child("file").and_then(|f| f.attr("src")) {
            imp.objects.insert(key(&o.attr("handle").unwrap_or_default()), src.into_owned());
        }
    }

    imp.sources_and_citations(root)?;
    let mut report = ImportReport::default();
    for p in root.child("people").into_iter().flat_map(|c| c.children_named("person")) {
        imp.person(p)?;
        report.people += 1;
    }
    for f in root.child("families").into_iter().flat_map(|c| c.children_named("family")) {
        imp.family(f);
        report.families += 1;
    }
    // only notes on sources and people are carried over
    let unused = imp.notes.len().saturating_sub(imp.used_notes.len());
    if unused > 0 {
        imp.dropped.add_n("notes", unused);
    }
    for section in root.elements() {
        if ![
            "header",
            "events",
            "people",
            "families",
            "places",
            "objects",
            "sources",
            "citations",
            "repositories",
            "notes",
        ]
        .contains(&section.name.as_str())
        {
            let n = section.elements().count();
            if n > 0 {
                imp.dropped.add_n(section.name.clone(), n);
            }
        }
    }

    if let Some(first) = imp.pkg.doc.root.child("PeopleCollection").and_then(|c| c.child("Person")) {
        let id = first.attr("Id").unwrap_or_default().into_owned();
        let _ = edit::set_home_person(&mut imp.pkg.doc, &id);
    }
    for problem in edit::check(&imp.pkg.doc) {
        imp.warnings.push(format!("inconsistency after import: {problem}"));
    }
    let findings = Findings { dropped: imp.dropped, warnings: imp.warnings, ..Default::default() };
    let report = crate::report::finish(&imp.pkg, report, findings);
    Ok((imp.pkg, report))
}

impl Importer<'_> {
    /// `Name, Parent, Country` from the place hierarchy.
    fn place_title(&self, handle: &str) -> String {
        let mut parts = Vec::new();
        let mut seen = HashSet::new();
        let mut cur = Some(key(handle));
        let mut fallback = String::new();
        while let Some(h) = cur {
            let Some(p) = self.places.get(&h).filter(|_| seen.insert(h.clone())) else { break };
            if fallback.is_empty() {
                fallback = p.title.clone();
            }
            if !p.name.is_empty() {
                parts.push(p.name.clone());
            }
            cur = p.parent.as_deref().map(key);
        }
        if parts.is_empty() {
            fallback
        } else {
            parts.join(", ")
        }
    }

    /// Create sources (with their repository) and remember citations by handle.
    fn sources_and_citations(&mut self, root: &Element) -> Result<()> {
        let mut repos: HashMap<String, String> = HashMap::new();
        for r in root.child("repositories").into_iter().flat_map(|c| c.children_named("repository")) {
            let name = r.child_text("rname").unwrap_or_default();
            let name = if name.trim().is_empty() { "(unnamed repository)".to_string() } else { name };
            repos.insert(key(&r.attr("handle").unwrap_or_default()), edit::add_repository(&mut self.pkg.doc, &name)?);
            for c in r.elements().filter(|c| !["rname", "type"].contains(&c.name.as_str())) {
                self.dropped.add(format!("repository.{}", c.name));
            }
        }
        self.notes = root
            .child("notes")
            .into_iter()
            .flat_map(|c| c.children_named("note"))
            .map(|n| (key(&n.attr("handle").unwrap_or_default()), n.child_text("text").unwrap_or_default()))
            .collect();
        for s in root.child("sources").into_iter().flat_map(|c| c.children_named("source")) {
            let mut input = edit::SourceInput::default();
            let mut note_texts = Vec::new();
            for c in s.elements() {
                match c.name.as_str() {
                    "stitle" => input.title = c.text().trim().to_string(),
                    "sauthor" => input.author = c.text().trim().to_string(),
                    "spubinfo" => input.publisher = c.text().trim().to_string(),
                    "sabbrev" => input.abbreviation = c.text().trim().to_string(),
                    "noteref" => {
                        self.used_notes.insert(handle(c));
                        note_texts.extend(self.notes.get(&handle(c)).cloned());
                    }
                    "reporef" => match repos.get(&handle(c)) {
                        Some(id) if input.repository.is_none() => {
                            input.repository = Some(id.clone());
                            input.call_number = c.attr("callno").map(|v| v.into_owned()).unwrap_or_default();
                        }
                        Some(_) => self.dropped.add("source.reporef beyond the first"),
                        None => self.dropped.add("source.reporef to unknown repository"),
                    },
                    other => self.dropped.add(format!("source.{other}")),
                }
            }
            let note = note_texts.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("\n\n");
            input.note = Some(note).filter(|n| !n.is_empty());
            let id = edit::add_source(&mut self.pkg.doc, &input)?;
            self.sources.insert(key(&s.attr("handle").unwrap_or_default()), id);
        }
        for c in root.child("citations").into_iter().flat_map(|c| c.children_named("citation")) {
            let source = c.child("sourceref").and_then(|r| self.sources.get(&handle(r))).cloned();
            let Some(source) = source else {
                self.dropped.add("citation without a known source");
                continue;
            };
            for child in c.elements().filter(|e| !["page", "sourceref", "confidence"].contains(&e.name.as_str())) {
                self.dropped.add(format!("citation.{}", child.name));
            }
            self.citations
                .insert(key(&c.attr("handle").unwrap_or_default()), (source, c.child_text("page").unwrap_or_default()));
        }
        Ok(())
    }

    /// Cite the citations with these handles on `owner` (a person or fact id).
    fn cite(&mut self, owner: &str, handles: &[String]) {
        for h in handles {
            match self.citations.get(&key(h)).cloned() {
                Some((source, page)) => {
                    if let Err(e) = edit::add_citation(&mut self.pkg.doc, owner, &source, &page) {
                        self.warnings.push(format!("citation: {e}"));
                    }
                }
                None => self.dropped.add("citationref to an unknown citation"),
            }
        }
    }

    fn event_citations(&self, event_handle: &str) -> Vec<String> {
        self.events.get(&key(event_handle)).map(|e| e.citations.clone()).unwrap_or_default()
    }

    /// The fact tag, its content and, for an event type Gramps users made up,
    /// its name (people keep it as a named event; couples fold it into the details).
    fn fact_for(&self, event_handle: &str) -> Option<(String, FactInput, Option<String>)> {
        let ev = self.events.get(&key(event_handle))?;
        let known = GRAMPS_TYPES
            .iter()
            .find(|(_, ty)| ty.eq_ignore_ascii_case(ev.ty.trim()))
            .map(|(t, _)| *t)
            .or_else(|| vocab::relationship_tag(&ev.ty));
        let generic = matches!(ev.ty.trim(), "" | "Event" | "Custom" | "Unknown");
        let (tag, details, custom) = match known {
            Some(tag) => (tag, ev.description.clone(), None),
            None => ("EVEN", ev.description.clone(), (!generic).then(|| ev.ty.trim().to_string())),
        };
        let place = ev.place.as_deref().map(|h| self.place_title(h)).unwrap_or_default();
        Some((tag.to_string(), FactInput { date: dates::to_model(&ev.date), place, details }, custom))
    }

    fn person(&mut self, p: &Element) -> Result<()> {
        let gender = match p.child_text("gender").as_deref().map(str::trim) {
            Some("M") => Gender::Male,
            Some("F") => Gender::Female,
            Some("X") => Gender::Other,
            _ => Gender::Unknown,
        };
        let names: Vec<&Element> = p.children_named("name").collect();
        let primary = names.iter().position(|n| n.attr("alt").is_none()).unwrap_or(0);
        let (given, surname, extras) = names.get(primary).map(|n| self.name_parts(n)).unwrap_or_default();
        let id = edit::add_person(&mut self.pkg.doc, &given, &surname, gender);
        if !extras.is_empty() {
            edit::set_name_extras(&mut self.pkg.doc, &id, None, &extras)?;
        }
        self.people.insert(key(&p.attr("handle").unwrap_or_default()), id.clone());
        for (i, n) in names.iter().enumerate().filter(|(i, _)| *i != primary) {
            let _ = i;
            let (g, s, extras) = self.name_parts(n);
            let ty = n.attr("type").map(|t| vocab::name_type(&t)).unwrap_or("");
            let name_id = edit::add_alternate_name(&mut self.pkg.doc, &id, &g, &s, ty)?;
            if !extras.is_empty() {
                edit::set_name_extras(&mut self.pkg.doc, &id, Some(&name_id), &extras)?;
            }
        }

        let (mut dead, mut birth_year) = (false, None);
        let mut person_citations = Vec::new();
        let mut online_photos = Vec::new();
        let mut person_notes = Vec::new();
        for c in p.elements() {
            match c.name.as_str() {
                "eventref" => {
                    let role = c.attr("role").map(|r| r.into_owned()).unwrap_or_default();
                    if !role.is_empty() && role != "Primary" {
                        self.dropped.add(format!("person.eventref with role {role}"));
                        continue;
                    }
                    let Some((tag, input, custom)) = self.fact_for(&handle(c)) else {
                        self.warnings.push(format!("person {}: eventref to unknown event", id));
                        continue;
                    };
                    if tag == "BIRT" {
                        birth_year = birth_year.or(input.date.year);
                    }
                    dead |= matches!(tag.as_str(), "DEAT" | "BURI" | "CREM");
                    let fact = match custom {
                        Some(name) => edit::add_custom_fact(&mut self.pkg.doc, &id, &name, &input)?,
                        None => edit::add_fact(&mut self.pkg.doc, &id, &tag, &input)?,
                    };
                    let cites = self.event_citations(&handle(c));
                    self.cite(&fact, &cites);
                }
                "objref" => match self.objects.get(&handle(c)).cloned() {
                    Some(file) => {
                        let online = crate::import_photo(&mut self.pkg, &id, &file, self.resolve, &mut self.dropped);
                        online_photos.extend(online.map(|url| format!("Online photo: {url}")));
                    }
                    None => self.dropped.add("person.objref to unknown object"),
                },
                "citationref" => person_citations.push(handle(c)),
                "noteref" => {
                    self.used_notes.insert(handle(c));
                    person_notes.extend(self.notes.get(&handle(c)).cloned().filter(|n| !n.trim().is_empty()));
                }
                "gender" | "name" | "childof" | "parentin" => {}
                other => self.dropped.add(format!("person.{other}")),
            }
        }
        self.cite(&id, &person_citations);
        person_notes.extend(online_photos);
        edit::set_person_note(&mut self.pkg.doc, &id, &person_notes.join("\n\n"))?;
        edit::set_living(&mut self.pkg.doc, &id, vocab::guess_living(dead, birth_year))?;
        Ok(())
    }

    fn name_parts(&mut self, n: &Element) -> (String, String, NameExtras) {
        let surname: Vec<String> = n
            .children_named("surname")
            .map(|s| {
                let prefix = s.attr("prefix").map(|p| p.into_owned()).unwrap_or_default();
                format!("{prefix} {}", s.text()).trim().to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();
        for c in n.elements() {
            let kept = ["first", "surname", "suffix", "title", "nick", "dateval", "daterange", "datespan", "datestr"];
            if !kept.contains(&c.name.as_str()) {
                self.dropped.add(format!("person.name.{}", c.name));
            }
        }
        let part = |tag: &str| n.child_text(tag).unwrap_or_default().trim().to_string();
        let extras = NameExtras { title: part("title"), suffix: part("suffix"), nickname: part("nick") };
        (part("first"), surname.join(" "), extras)
    }

    fn family(&mut self, f: &Element) {
        let person =
            |el: Option<&Element>, people: &HashMap<String, String>| el.and_then(|e| people.get(&handle(e))).cloned();
        let (father, mother) = (person(f.child("father"), &self.people), person(f.child("mother"), &self.people));
        let link = match (&father, &mother) {
            (Some(a), Some(b)) => match edit::link_spouse(&mut self.pkg.doc, a, b) {
                Ok(id) => Some(id),
                Err(e) => {
                    self.warnings.push(format!("family: cannot link spouses: {e}"));
                    None
                }
            },
            _ => None,
        };

        let mut has_marriage = false;
        for c in f.elements() {
            match c.name.as_str() {
                "eventref" => {
                    let Some((tag, mut input, custom)) = self.fact_for(&handle(c)) else { continue };
                    has_marriage |= edit::is_marriage_tag(&tag);
                    if let Some(name) = custom {
                        input.details =
                            [name, input.details].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("; ");
                    }
                    match &link {
                        Some(link) => match edit::add_couple_fact(&mut self.pkg.doc, link, &tag, &input) {
                            Ok(fact) => {
                                let cites = self.event_citations(&handle(c));
                                self.cite(&fact, &cites);
                            }
                            Err(e) => self.warnings.push(format!("family: {tag}: {e}")),
                        },
                        None => self.dropped.add(format!("family event {tag} without two spouses")),
                    }
                }
                "rel" | "father" | "mother" | "childref" => {}
                other => self.dropped.add(format!("family.{other}")),
            }
        }

        // the family's relationship type (Married, Civil Union, Unmarried) without an event for it
        if let (Some(link), false) = (&link, has_marriage) {
            let rel = f.child("rel").and_then(|r| r.attr("type")).unwrap_or_default();
            if let Some(tag) = vocab::relationship_from_gramps(&rel) {
                if let Err(e) = edit::add_couple_fact(&mut self.pkg.doc, link, tag, &FactInput::default()) {
                    self.warnings.push(format!("family: {tag}: {e}"));
                }
            }
        }

        for c in f.children_named("childref") {
            let Some(child) = self.people.get(&handle(c)).cloned() else {
                self.warnings.push("family: childref to unknown person".into());
                continue;
            };
            for (parent, rel) in [(&father, c.attr("frel")), (&mother, c.attr("mrel"))] {
                let Some(parent) = parent else { continue };
                if let Err(e) = edit::link_parent(&mut self.pkg.doc, &child, parent) {
                    self.warnings.push(format!("family: cannot link child: {e}"));
                    continue;
                }
                let modifier = match rel.as_deref() {
                    Some("Adopted") => "Adopted",
                    Some("Foster") => "Foster",
                    Some("Stepchild") => "Step",
                    Some("Birth" | "Unknown") | None => "",
                    Some(other) => {
                        self.dropped.add(format!("childref relationship {other}"));
                        ""
                    }
                };
                if !modifier.is_empty() {
                    let _ = edit::set_parent_modifier(&mut self.pkg.doc, &child, parent, modifier);
                }
            }
        }
    }
}

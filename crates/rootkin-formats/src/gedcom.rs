//! GEDCOM 5.5.1 and 7.0 export, and tolerant import of 5.5 / 5.5.1 / 7.0 and vendor variants.
//!
//! Mapping notes (see `docs/design.md`): attributes (`OCCU`, `EDUC`, ...) keep
//! their text in the fact's details; events keep it as a `NOTE`. Photos are
//! `OBJE` records pointing at files; sources and repositories are `SOUR` and
//! `REPO` records, cited with `SOUR` / `PAGE` on people and facts. Notes on people
//! and `_CUSTOM` tags are not imported.

use std::collections::{HashMap, HashSet};

use familyx::edit::{self, FactInput};
use familyx::model::{Citation, Fact, Gender, Tree};
use familyx::Package;

use crate::dates::{self, DateValue};
use crate::families::{self, Fam};
use crate::vocab::{self, FAM_EVENTS, INDI_ATTRIBUTES, INDI_EVENTS};
use crate::{Dropped, Error, ExportMedia, ImportReport, Resolve, Result};

/// Longest value chunk per physical line (the limit is 255 including level and tag).
const CHUNK: usize = 240;

// ---------------------------------------------------------------- export

struct Writer {
    out: String,
    /// GEDCOM 7: no line length limit, so no `CONC`.
    v7: bool,
}

impl Writer {
    fn record(&mut self, xref: &str, tag: &str) {
        self.out.push_str(&format!("0 @{xref}@ {tag}\n"));
    }

    fn raw(&mut self, level: usize, tag: &str, value: &str) {
        if value.is_empty() {
            self.out.push_str(&format!("{level} {tag}\n"));
        } else {
            self.out.push_str(&format!("{level} {tag} {value}\n"));
        }
    }

    /// A line whose value may be long or multi-line: split with CONT / CONC.
    fn line(&mut self, level: usize, tag: &str, value: &str) {
        let escaped = value.replace('@', "@@").replace("\r\n", "\n");
        for (i, segment) in escaped.split('\n').enumerate() {
            let (lvl, t) = if i == 0 { (level, tag) } else { (level + 1, "CONT") };
            if self.v7 {
                self.raw(lvl, t, segment);
                continue;
            }
            for (j, chunk) in split_chunks(segment, CHUNK).into_iter().enumerate() {
                if j == 0 {
                    self.raw(lvl, t, chunk);
                } else {
                    self.raw(level + 1, "CONC", chunk);
                }
            }
        }
    }

    fn pointer(&mut self, level: usize, tag: &str, xref: &str) {
        self.raw(level, tag, &format!("@{xref}@"));
    }
}

/// Split at char boundaries, avoiding a space at either side of the cut
/// (readers differ in whether they keep it).
fn split_chunks(s: &str, max: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s;
    while rest.len() > max {
        let mut cut = max;
        while !rest.is_char_boundary(cut) {
            cut -= 1;
        }
        let mut c = cut;
        while c > 1 && (rest[..c].ends_with(' ') || rest[c..].starts_with(' ')) {
            c -= 1;
            while !rest.is_char_boundary(c) {
                c -= 1;
            }
        }
        if c <= 1 {
            c = cut;
        }
        out.push(&rest[..c]);
        rest = &rest[c..];
    }
    out.push(rest);
    out
}

pub(crate) fn export(pkg: &Package, v7: bool, media: &ExportMedia) -> Vec<u8> {
    let tree = Tree::from_doc(&pkg.doc);
    let fams = families::build(&tree);
    // source id -> number of its `@S<n>@` record
    let src: HashMap<&str, usize> = tree.sources.iter().enumerate().map(|(i, s)| (s.id.as_str(), i + 1)).collect();
    let indi_xref = |id: &str| tree.people.iter().position(|p| p.id == id).map(|i| format!("I{}", i + 1));

    let mut w = Writer { out: String::new(), v7 };
    w.raw(0, "HEAD", "");
    w.raw(1, "SOUR", "ROOTKIN");
    w.raw(2, "NAME", "Rootkin");
    w.raw(2, "VERS", env!("CARGO_PKG_VERSION"));
    w.raw(1, "GEDC", "");
    if v7 {
        w.raw(2, "VERS", "7.0");
    } else {
        w.raw(2, "VERS", "5.5.1");
        w.raw(2, "FORM", "LINEAGE-LINKED");
        w.raw(1, "CHAR", "UTF-8");
    }
    w.pointer(1, "SUBM", "U1");
    w.record("U1", "SUBM");
    w.raw(1, "NAME", "Rootkin");

    for (i, person) in tree.people.iter().enumerate() {
        w.record(&format!("I{}", i + 1), "INDI");
        let primary = person.primary_name();
        let names = primary.into_iter().chain(person.names.iter().filter(|n| Some(*n) != primary));
        let mut any_name = false;
        for name in names {
            any_name = true;
            write_name(&mut w, name, Some(name) != primary);
        }
        if !any_name {
            w.raw(1, "NAME", "//");
        }
        match person.gender {
            Gender::Male => w.raw(1, "SEX", "M"),
            Gender::Female => w.raw(1, "SEX", "F"),
            // GEDCOM 7's "X"; 5.5.1 readers commonly accept it
            Gender::Other => w.raw(1, "SEX", "X"),
            Gender::Unknown => {}
        }
        write_citations(&mut w, 1, &person.citations, &src);
        for fact in &person.facts {
            write_fact(&mut w, fact, false, &src);
        }
        // the primary photo first: readers commonly treat it as the portrait
        let photos = person
            .primary_image
            .iter()
            .chain(person.media.iter().filter(|m| Some(*m) != person.primary_image.as_ref()));
        for m in photos {
            if let Some(r) = media.refs.get(m) {
                w.pointer(1, "OBJE", &format!("O{}", r.number));
            }
        }
        for (f, fam) in fams.iter().enumerate() {
            if fam.husb == Some(&person.id) || fam.wife == Some(&person.id) {
                w.pointer(1, "FAMS", &format!("F{}", f + 1));
            }
        }
        for (f, fam) in fams.iter().enumerate() {
            if let Some(child) = fam.children.iter().find(|c| c.id == person.id) {
                w.pointer(1, "FAMC", &format!("F{}", f + 1));
                let pedi = match child.modifier() {
                    "Adopted" => "adopted",
                    "Foster" => "foster",
                    "Sealing" => "sealing",
                    _ => "",
                };
                if !pedi.is_empty() {
                    // 7.0 uses upper-case enumeration values
                    w.raw(2, "PEDI", &if v7 { pedi.to_ascii_uppercase() } else { pedi.to_string() });
                }
            }
        }
    }

    for (f, fam) in fams.iter().enumerate() {
        write_family(&mut w, f + 1, fam, &indi_xref, &src);
    }
    for id in &media.order {
        let r = &media.refs[id];
        w.record(&format!("O{}", r.number), "OBJE");
        w.line(1, "FILE", &r.path);
        let form = match (v7, r.ext.as_str()) {
            (true, "png") => "image/png".to_string(),
            (true, _) => "image/jpeg".to_string(),
            (false, ext) => ext.to_string(),
        };
        w.raw(2, "FORM", &form);
        if !r.title.is_empty() {
            // 5.5.1 puts the title under FILE; 7.0 too (FILE.TITL)
            w.line(2, "TITL", &r.title);
        }
    }
    for (i, s) in tree.sources.iter().enumerate() {
        w.record(&format!("S{}", i + 1), "SOUR");
        for (tag, value) in [("TITL", &s.title), ("ABBR", &s.abbreviation), ("AUTH", &s.author), ("PUBL", &s.publisher)]
        {
            if !value.is_empty() {
                w.line(1, tag, value);
            }
        }
        if let Some(r) = s.repository.as_deref().and_then(|r| tree.repositories.iter().position(|x| x.id == r)) {
            w.pointer(1, "REPO", &format!("R{}", r + 1));
            if !s.call_number.is_empty() {
                w.line(2, "CALN", &s.call_number);
            }
        }
        for note in &s.notes {
            w.line(1, "NOTE", &note.plain_text());
        }
    }
    for (i, r) in tree.repositories.iter().enumerate() {
        w.record(&format!("R{}", i + 1), "REPO");
        w.line(1, "NAME", &r.title);
    }
    w.raw(0, "TRLR", "");
    w.out.into_bytes()
}

fn write_family(
    w: &mut Writer,
    number: usize,
    fam: &Fam,
    indi_xref: &dyn Fn(&str) -> Option<String>,
    src: &HashMap<&str, usize>,
) {
    w.record(&format!("F{number}"), "FAM");
    for (tag, id) in [("HUSB", fam.husb), ("WIFE", fam.wife)] {
        if let Some(x) = id.and_then(indi_xref) {
            w.pointer(1, tag, &x);
        }
    }
    for child in &fam.children {
        if let Some(x) = indi_xref(child.id) {
            w.pointer(1, "CHIL", &x);
        }
    }
    for fact in fam.link.iter().flat_map(|l| &l.facts) {
        write_fact(w, fact, true, src);
    }
}

fn write_name(w: &mut Writer, name: &familyx::model::Name, alternate: bool) {
    let surname = [name.surname_prefix.as_str(), name.surname.as_str()]
        .iter()
        .filter(|s| !s.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let mut line = format!("{} /{surname}/", name.given).trim_start().to_string();
    if !name.suffix.is_empty() {
        line.push(' ');
        line.push_str(&name.suffix);
    }
    w.line(1, "NAME", &line);
    if alternate && !name.name_type.is_empty() {
        let ty = match name.name_type.as_str() {
            t @ ("Aka" | "Birth" | "Immigrant" | "Maiden" | "Married") => t.to_lowercase(),
            other => other.to_string(),
        };
        w.line(2, "TYPE", &ty);
    }
    for (tag, value) in [
        ("GIVN", &name.given),
        ("SPFX", &name.surname_prefix),
        ("SURN", &name.surname),
        ("NSFX", &name.suffix),
        ("NICK", &name.nickname),
    ] {
        if !value.is_empty() {
            w.line(2, tag, value);
        }
    }
}

/// `SOUR` pointers with `PAGE` at `level`; citations of unknown sources are skipped.
fn write_citations(w: &mut Writer, level: usize, citations: &[Citation], src: &HashMap<&str, usize>) {
    for c in citations {
        let Some(n) = src.get(c.source.as_str()) else { continue };
        w.pointer(level, "SOUR", &format!("S{n}"));
        if !c.page.is_empty() {
            w.line(level + 1, "PAGE", &c.page);
        }
    }
}

fn write_fact(w: &mut Writer, fact: &Fact, in_family: bool, src: &HashMap<&str, usize>) {
    let tag = fact.tag.as_str();
    let is_event = if in_family { FAM_EVENTS.contains(&tag) } else { INDI_EVENTS.contains(&tag) };
    let is_attribute = !in_family && INDI_ATTRIBUTES.contains(&tag);
    if is_attribute {
        w.line(1, tag, &fact.details);
    } else if is_event {
        w.raw(1, tag, "");
    } else {
        // tags GEDCOM does not know travel as a typed generic event
        w.raw(1, "EVEN", "");
        w.line(2, "TYPE", fact.label());
    }
    let date = dates::from_model(&fact.date).to_gedcom();
    if !date.is_empty() {
        w.line(2, "DATE", &date);
    }
    if !fact.place.is_empty() {
        w.line(2, "PLAC", &fact.place);
    }
    if !is_attribute && !fact.details.is_empty() {
        w.line(2, "NOTE", &fact.details);
    }
    write_citations(w, 2, &fact.citations, src);
}

// ---------------------------------------------------------------- import

#[derive(Debug)]
struct Node {
    tag: String,
    xref: Option<String>,
    value: String,
    children: Vec<Node>,
}

impl Node {
    fn child(&self, tag: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.tag == tag)
    }

    fn children_named<'a>(&'a self, tag: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.tag == tag)
    }

    fn child_value(&self, tag: &str) -> Option<&str> {
        self.child(tag).map(|c| c.value.trim()).filter(|v| !v.is_empty())
    }
}

/// UTF-8 (with or without BOM) or UTF-16; invalid UTF-8 is read as Latin-1.
fn decode(bytes: &[u8], warnings: &mut Vec<String>) -> String {
    let utf16 = |data: &[u8], le: bool| {
        let units =
            data.as_chunks::<2>().0.iter().map(|&c| if le { u16::from_le_bytes(c) } else { u16::from_be_bytes(c) });
        char::decode_utf16(units).map(|r| r.unwrap_or('\u{fffd}')).collect::<String>()
    };
    match bytes {
        [0xff, 0xfe, rest @ ..] => utf16(rest, true),
        [0xfe, 0xff, rest @ ..] => utf16(rest, false),
        [a, 0, ..] if *a != 0 => utf16(bytes, true),
        [0, b, ..] if *b != 0 => utf16(bytes, false),
        _ => {
            let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
            match std::str::from_utf8(bytes) {
                Ok(s) => s.to_string(),
                Err(_) => {
                    warnings.push("file is not valid UTF-8; read as Latin-1 (ANSEL is not supported)".into());
                    bytes.iter().map(|&b| b as char).collect()
                }
            }
        }
    }
}

fn parse_line(line: &str) -> Option<(u32, Option<String>, String, String)> {
    let (level, rest) = line.split_once(char::is_whitespace)?;
    let level: u32 = level.parse().ok()?;
    let mut rest = rest.trim_start();
    let mut xref = None;
    if rest.starts_with('@') {
        let (x, r) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        if x.len() > 2 && x.ends_with('@') {
            xref = Some(x.to_string());
            rest = r.trim_start();
        }
    }
    let (tag, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
    if tag.is_empty() {
        return None;
    }
    Some((level, xref, tag.to_ascii_uppercase(), unescape_at(value)))
}

/// `@@` stands for a literal `@`, except in pointers like `@I1@`.
fn unescape_at(value: &str) -> String {
    if is_pointer(value) {
        value.to_string()
    } else {
        value.replace("@@", "@")
    }
}

fn is_pointer(value: &str) -> bool {
    let v = value.trim();
    v.len() > 2 && v.starts_with('@') && v.ends_with('@') && !v[1..v.len() - 1].contains('@')
}

fn attach(stack: &mut [(u32, Node)], roots: &mut Vec<Node>, done: Node) {
    match stack.last_mut() {
        Some((_, parent)) => parent.children.push(done),
        None => roots.push(done),
    }
}

/// Build the record tree, merging CONC / CONT lines into their parent's value.
fn parse(text: &str, warnings: &mut Vec<String>) -> Vec<Node> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut roots = Vec::new();
    let mut stack: Vec<(u32, Node)> = Vec::new();
    let (mut bad, mut first_bad) = (0, 0);
    for (n, line) in text.lines().enumerate() {
        let line = line.trim_start_matches('\u{feff}').trim_start();
        if line.is_empty() {
            continue;
        }
        let Some((level, xref, tag, value)) = parse_line(line) else {
            bad += 1;
            if bad == 1 {
                first_bad = n + 1;
            }
            continue;
        };
        while stack.last().is_some_and(|(l, _)| *l >= level) {
            let (_, done) = stack.pop().expect("checked");
            attach(&mut stack, &mut roots, done);
        }
        if tag == "CONC" || tag == "CONT" {
            match stack.last_mut() {
                Some((_, parent)) => {
                    if tag == "CONT" {
                        parent.value.push('\n');
                    }
                    parent.value.push_str(&value);
                }
                None => bad += 1,
            }
            continue;
        }
        stack.push((level, Node { tag, xref, value, children: Vec::new() }));
    }
    while let Some((_, done)) = stack.pop() {
        attach(&mut stack, &mut roots, done);
    }
    if bad > 0 {
        warnings.push(format!("skipped {bad} malformed line(s), first at line {first_bad}"));
    }
    roots
}

/// `Given /Surname/ Suffix` -> (given, surname, suffix).
fn split_name(value: &str) -> (String, String, String) {
    match value.split_once('/') {
        Some((given, rest)) => {
            let (surname, suffix) = rest.split_once('/').unwrap_or((rest, ""));
            (given.trim().into(), surname.trim().into(), suffix.trim().into())
        }
        None => (value.trim().into(), String::new(), String::new()),
    }
}

/// Tags that carry no genealogical content for us; not worth a warning.
const SILENT: &[&str] = &["CHAN", "RIN", "RFN", "AFN", "REFN", "UID", "_UID", "FAMS", "RESN", "FAMC"];

struct Importer<'a> {
    pkg: Package,
    resolve: Resolve<'a>,
    /// OBJE xref -> file reference
    objects: HashMap<String, String>,
    dropped: Dropped,
    warnings: Vec<String>,
    notes: HashMap<String, String>,
    /// SOUR xref -> source id
    sources: HashMap<String, String>,
    /// text of inline (pointer-less) `SOUR` lines -> source id
    inline_sources: HashMap<String, String>,
    /// INDI xref -> person id
    people: HashMap<String, String>,
    /// (FAM xref, INDI xref) -> PEDI value
    pedi: HashMap<(String, String), String>,
    /// (FAM xref, INDI xref) pairs marked by `ADOP` / `FAMC`
    adopted: HashSet<(String, String)>,
    /// FAM xref -> children that name the family in a `FAMC`
    famc_children: HashMap<String, Vec<String>>,
}

pub fn import(bytes: &[u8], resolve: Resolve) -> Result<(Package, ImportReport)> {
    let mut warnings = Vec::new();
    let text = decode(bytes, &mut warnings);
    let roots = parse(&text, &mut warnings);
    if roots.first().map(|n| n.tag.as_str()) != Some("HEAD") {
        return Err(Error::Gedcom("not a GEDCOM file (no HEAD record)".into()));
    }
    let mut imp = Importer {
        pkg: Package::new(),
        resolve,
        objects: HashMap::new(),
        dropped: Dropped::default(),
        warnings,
        notes: HashMap::new(),
        sources: HashMap::new(),
        inline_sources: HashMap::new(),
        people: HashMap::new(),
        pedi: HashMap::new(),
        adopted: HashSet::new(),
        famc_children: HashMap::new(),
    };
    if let Some(ch) = roots[0].child_value("CHAR") {
        if !matches!(ch.to_ascii_uppercase().as_str(), "UTF-8" | "UNICODE" | "UTF8") {
            imp.warnings.push(format!("header declares character set {ch}; text was read as UTF-8 / UTF-16"));
        }
    }

    for n in &roots {
        match (&n.xref, n.tag.as_str()) {
            (Some(x), "NOTE") => {
                imp.notes.insert(x.clone(), n.value.clone());
            }
            (Some(x), "OBJE") => {
                if let Some(file) = n.child_value("FILE") {
                    imp.objects.insert(x.clone(), file.to_string());
                }
            }
            _ => {}
        }
    }
    imp.sources_and_repositories(&roots)?;
    let mut report = ImportReport::default();
    for n in roots.iter().filter(|n| n.tag == "INDI") {
        imp.indi(n)?;
        report.people += 1;
    }
    for n in roots.iter().filter(|n| n.tag == "FAM") {
        imp.fam(n);
        report.families += 1;
    }
    for n in &roots {
        match n.tag.as_str() {
            "HEAD" | "TRLR" | "SUBM" | "SUBN" | "INDI" | "FAM" | "NOTE" | "OBJE" | "SOUR" | "REPO" => {}
            other => imp.dropped.add(format!("{other} record")),
        }
    }

    if let Some(first) = imp.pkg.doc.root.child("PeopleCollection").and_then(|c| c.child("Person")) {
        let id = first.attr("Id").unwrap_or_default().into_owned();
        let _ = edit::set_home_person(&mut imp.pkg.doc, &id);
    }
    for problem in edit::check(&imp.pkg.doc) {
        imp.warnings.push(format!("inconsistency after import: {problem}"));
    }
    imp.dropped.into_warnings(&mut imp.warnings);
    report.warnings = imp.warnings;
    Ok((imp.pkg, report))
}

impl Importer<'_> {
    /// Text of a `NOTE` line: inline, or the record it points to.
    fn note_text(&self, n: &Node) -> String {
        if is_pointer(&n.value) {
            self.notes.get(n.value.trim()).cloned().unwrap_or_default()
        } else {
            n.value.clone()
        }
    }

    fn drop_children(&mut self, path: &str, n: &Node, silent: &[&str]) {
        for c in &n.children {
            if !SILENT.contains(&c.tag.as_str()) && !silent.contains(&c.tag.as_str()) {
                self.dropped.add(format!("{path}.{}", c.tag));
            }
        }
    }

    fn fact_input(&mut self, path: &str, n: &Node, attribute: bool) -> FactInput {
        let mut input = FactInput::default();
        let (mut ty, mut notes) = (String::new(), Vec::new());
        for c in &n.children {
            match c.tag.as_str() {
                "DATE" => input.date = dates::to_model(&DateValue::from_gedcom(&c.value)),
                "PLAC" => {
                    input.place = c.value.trim().to_string();
                    self.drop_children(&format!("{path}.PLAC"), c, &["FORM"]);
                }
                "TYPE" => ty = c.value.trim().to_string(),
                "NOTE" => notes.push(self.note_text(c)),
                // cited by the caller once the fact exists
                "SOUR" => {}
                t if SILENT.contains(&t) => {}
                t => self.dropped.add(format!("{path}.{t}")),
            }
        }
        let value = n.value.trim();
        let value = if value.is_empty() || (value == "Y" && !attribute) { None } else { Some(value.to_string()) };
        let parts: Vec<String> =
            [Some(ty), value].into_iter().flatten().chain(notes).filter(|s| !s.is_empty()).collect();
        input.details = parts.join("; ");
        input
    }

    /// Create the sources and repositories of `SOUR` / `REPO` records.
    fn sources_and_repositories(&mut self, roots: &[Node]) -> Result<()> {
        let mut repos: HashMap<&str, String> = HashMap::new();
        for n in roots.iter().filter(|n| n.tag == "REPO") {
            let Some(x) = &n.xref else { continue };
            let name = n.child_value("NAME").unwrap_or("(unnamed repository)");
            repos.insert(x, edit::add_repository(&mut self.pkg.doc, name)?);
            self.drop_children("REPO", n, &["NAME"]);
        }
        for n in roots.iter().filter(|n| n.tag == "SOUR") {
            let Some(x) = &n.xref else { continue };
            let mut input = edit::SourceInput::default();
            let mut notes = Vec::new();
            for c in &n.children {
                let v = c.value.trim().to_string();
                match c.tag.as_str() {
                    "TITL" => input.title = v,
                    "ABBR" => input.abbreviation = v,
                    "AUTH" => input.author = v,
                    "PUBL" => input.publisher = v,
                    "NOTE" => notes.push(self.note_text(c)),
                    "REPO" => match repos.get(v.as_str()) {
                        Some(id) => {
                            input.repository = Some(id.clone());
                            input.call_number = c.child_value("CALN").unwrap_or_default().to_string();
                            self.drop_children("SOUR.REPO", c, &["CALN"]);
                        }
                        None => self.dropped.add("SOUR.REPO that is not a repository record"),
                    },
                    t if SILENT.contains(&t) => {}
                    t => self.dropped.add(format!("SOUR.{t}")),
                }
            }
            let note = notes.into_iter().filter(|n| !n.is_empty()).collect::<Vec<_>>().join("\n\n");
            input.note = Some(note).filter(|n| !n.is_empty());
            self.sources.insert(x.clone(), edit::add_source(&mut self.pkg.doc, &input)?);
        }
        Ok(())
    }

    /// Cite the sources named by the `SOUR` children of `n` on `owner`.
    fn cite(&mut self, path: &str, owner: &str, n: &Node) -> Result<()> {
        for c in n.children_named("SOUR") {
            self.cite_line(path, owner, c)?;
        }
        Ok(())
    }

    /// One `SOUR` line: a pointer to a source record, or inline source text.
    fn cite_line(&mut self, path: &str, owner: &str, c: &Node) -> Result<()> {
        let path = format!("{path}.SOUR");
        let source = if is_pointer(&c.value) {
            let ptr = c.value.trim();
            if ptr.eq_ignore_ascii_case("@VOID@") {
                return Ok(());
            }
            match self.sources.get(ptr) {
                Some(id) => id.clone(),
                None => {
                    self.warnings.push(format!("{path} points to missing source {ptr}"));
                    return Ok(());
                }
            }
        } else {
            // a source described in place: one anonymous source per distinct text
            let text = c.value.trim().to_string();
            if text.is_empty() {
                self.dropped.add(format!("{path} without a source"));
                return Ok(());
            }
            match self.inline_sources.get(&text) {
                Some(id) => id.clone(),
                None => {
                    let input = edit::SourceInput { title: text.clone(), ..Default::default() };
                    let id = edit::add_source(&mut self.pkg.doc, &input)?;
                    self.inline_sources.insert(text, id.clone());
                    id
                }
            }
        };
        let page = c.child_value("PAGE").unwrap_or_default();
        edit::add_citation(&mut self.pkg.doc, owner, &source, page)?;
        self.drop_children(&path, c, &["PAGE"]);
        Ok(())
    }

    fn indi(&mut self, n: &Node) -> Result<()> {
        let names: Vec<&Node> = n.children_named("NAME").collect();
        let (given, surname) = names.first().map(|n| self.name_parts(n)).unwrap_or_default();
        let gender = match n.child_value("SEX").and_then(|s| s.chars().next()) {
            Some('M' | 'm') => Gender::Male,
            Some('F' | 'f') => Gender::Female,
            Some('X' | 'x') => Gender::Other,
            _ => Gender::Unknown,
        };
        let id = edit::add_person(&mut self.pkg.doc, &given, &surname, gender);
        if let Some(x) = &n.xref {
            self.people.insert(x.clone(), id.clone());
        }
        for alt in names.iter().skip(1) {
            let (g, s) = self.name_parts(alt);
            let ty = alt.child_value("TYPE").map(vocab::name_type).unwrap_or("");
            edit::add_alternate_name(&mut self.pkg.doc, &id, &g, &s, ty)?;
        }

        let (mut dead, mut birth_year) = (false, None);
        for c in &n.children {
            let tag = c.tag.as_str();
            let attribute = INDI_ATTRIBUTES.contains(&tag);
            if attribute || INDI_EVENTS.contains(&tag) {
                let input = self.fact_input(&format!("INDI.{tag}"), c, attribute);
                if tag == "BIRT" {
                    birth_year = birth_year.or(input.date.year);
                }
                dead |= matches!(tag, "DEAT" | "BURI" | "CREM");
                let fact = edit::add_fact(&mut self.pkg.doc, &id, tag, &input)?;
                self.cite(&format!("INDI.{tag}"), &fact, c)?;
                if let (Some(xref), "ADOP") = (&n.xref, tag) {
                    for famc in c.children_named("FAMC") {
                        self.adopted.insert((famc.value.trim().to_string(), xref.clone()));
                    }
                }
            } else if tag == "SOUR" {
                self.cite_line("INDI", &id, c)?;
            } else if tag == "OBJE" {
                // a pointer to an OBJE record, or an inline FILE
                let file = if is_pointer(&c.value) {
                    self.objects.get(c.value.trim()).cloned()
                } else {
                    c.child_value("FILE").map(str::to_string)
                };
                match file {
                    Some(file) => crate::import_photo(&mut self.pkg, &id, &file, self.resolve, &mut self.dropped),
                    None => self.dropped.add("INDI.OBJE without a file"),
                }
            } else if tag == "FAMC" {
                if let Some(xref) = &n.xref {
                    let fam = c.value.trim().to_string();
                    if let Some(p) = c.child_value("PEDI") {
                        self.pedi.insert((fam.clone(), xref.clone()), p.to_ascii_lowercase());
                    }
                    self.famc_children.entry(fam).or_default().push(xref.clone());
                }
            } else if !matches!(tag, "NAME" | "SEX") && !SILENT.contains(&tag) {
                self.dropped.add(format!("INDI.{tag}"));
            }
        }
        edit::set_living(&mut self.pkg.doc, &id, vocab::guess_living(dead, birth_year))?;
        Ok(())
    }

    fn name_parts(&mut self, n: &Node) -> (String, String) {
        let (mut given, mut surname, suffix) = split_name(&n.value);
        let mut spfx = String::new();
        for c in &n.children {
            let v = c.value.trim();
            match c.tag.as_str() {
                "GIVN" if !v.is_empty() => given = v.to_string(),
                "SURN" if !v.is_empty() => surname = v.to_string(),
                "SPFX" => spfx = v.to_string(),
                "TYPE" => {}
                t if SILENT.contains(&t) => {}
                t => self.dropped.add(format!("INDI.NAME.{t}")),
            }
        }
        if !spfx.is_empty() && !surname.starts_with(&spfx) {
            surname = format!("{spfx} {surname}");
        }
        if !suffix.is_empty() {
            self.dropped.add("INDI.NAME suffix");
        }
        (given, surname)
    }

    fn person(&mut self, fam: &str, tag: &str, n: &Node) -> Option<String> {
        let ptr = n.child(tag)?.value.trim();
        if ptr.eq_ignore_ascii_case("@VOID@") {
            return None;
        }
        let id = self.people.get(ptr).cloned();
        if id.is_none() {
            self.warnings.push(format!("FAM {fam}: {tag} points to missing individual {ptr}"));
        }
        id
    }

    fn fam(&mut self, n: &Node) {
        let label = n.xref.clone().unwrap_or_default();
        let (husb, wife) = (self.person(&label, "HUSB", n), self.person(&label, "WIFE", n));
        let parents: Vec<String> = husb.iter().chain(wife.iter()).cloned().collect();
        let doc = &mut self.pkg.doc;
        let link = match (&husb, &wife) {
            (Some(h), Some(w)) => match edit::link_spouse(doc, h, w) {
                Ok(id) => Some(id),
                Err(e) => {
                    self.warnings.push(format!("FAM {label}: cannot link spouses: {e}"));
                    None
                }
            },
            _ => None,
        };

        for c in &n.children {
            let tag = c.tag.as_str();
            if FAM_EVENTS.contains(&tag) {
                let input = self.fact_input(&format!("FAM.{tag}"), c, false);
                match &link {
                    Some(link) => match edit::add_couple_fact(&mut self.pkg.doc, link, tag, &input) {
                        Ok(fact) => {
                            if let Err(e) = self.cite(&format!("FAM.{tag}"), &fact, c) {
                                self.warnings.push(format!("FAM {label}: {tag}: {e}"));
                            }
                        }
                        Err(e) => self.warnings.push(format!("FAM {label}: {tag}: {e}")),
                    },
                    None => self.dropped.add(format!("FAM.{tag} without two spouses")),
                }
            } else if !matches!(tag, "HUSB" | "WIFE" | "CHIL") && !SILENT.contains(&tag) {
                self.dropped.add(format!("FAM.{tag}"));
            }
        }

        // children named by CHIL, plus those that name this family in FAMC
        let mut children: Vec<String> = n.children_named("CHIL").map(|c| c.value.trim().to_string()).collect();
        for extra in self.famc_children.get(&label).into_iter().flatten() {
            if !children.contains(extra) {
                children.push(extra.clone());
            }
        }
        for child_x in children {
            if child_x.eq_ignore_ascii_case("@VOID@") {
                continue;
            }
            let Some(child) = self.people.get(&child_x).cloned() else {
                self.warnings.push(format!("FAM {label}: CHIL points to missing individual {child_x}"));
                continue;
            };
            let key = (label.clone(), child_x.clone());
            let modifier = match self.pedi.get(&key).map(String::as_str) {
                Some("adopted") => "Adopted",
                Some("foster") => "Foster",
                Some("sealing") => "Sealing",
                Some("birth") | None => {
                    if self.adopted.contains(&key) {
                        "Adopted"
                    } else {
                        ""
                    }
                }
                Some(other) => {
                    self.warnings.push(format!("FAM {label}: unknown PEDI value {other}, treated as birth"));
                    ""
                }
            };
            for parent in &parents {
                if let Err(e) = edit::link_parent(&mut self.pkg.doc, &child, parent) {
                    self.warnings.push(format!("FAM {label}: cannot link child {child_x}: {e}"));
                } else if !modifier.is_empty() {
                    let _ = edit::set_parent_modifier(&mut self.pkg.doc, &child, parent, modifier);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_and_multiline_values_roundtrip_through_conc_cont() {
        let value = format!("{}\nsecond line with é and @ sign\n\nlast", "word ".repeat(120));
        let mut w = Writer { out: String::new(), v7: false };
        w.raw(0, "HEAD", "");
        w.line(1, "NOTE", &value);
        assert!(w.out.lines().all(|l| l.len() <= 255), "line too long");
        assert!(w.out.contains("CONC") && w.out.contains("CONT"));
        let mut warnings = Vec::new();
        let roots = parse(&w.out, &mut warnings);
        assert_eq!(roots[0].children[0].value, value);
    }

    #[test]
    fn decodes_utf16_and_bom() {
        let text = "0 HEAD\n1 CHAR UNICODE\n0 TRLR\n";
        let mut le = vec![0xff, 0xfe];
        le.extend(text.encode_utf16().flat_map(|u| u.to_le_bytes()));
        let mut warnings = Vec::new();
        assert_eq!(decode(&le, &mut warnings), text);
        let be: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_be_bytes()).collect();
        assert_eq!(decode(&be, &mut warnings), text);
        let mut bom = vec![0xef, 0xbb, 0xbf];
        bom.extend(text.as_bytes());
        assert_eq!(decode(&bom, &mut warnings), text);
    }

    #[test]
    fn splits_names() {
        assert_eq!(split_name("Karl /Lindqvist/ Jr"), ("Karl".into(), "Lindqvist".into(), "Jr".into()));
        assert_eq!(split_name("Cher"), ("Cher".into(), String::new(), String::new()));
    }
}

//! Tree image (SVG) and printable HTML reports.

use std::fmt::Write;

use familyx::doc::escape;
use familyx::model::{Gender, Person, Tree};

use crate::layout::{self, EdgeKind, Settings, BOX};

fn fill(g: Gender) -> &'static str {
    match g {
        Gender::Male => "#2f5d8a",
        Gender::Female => "#8a3f5d",
        Gender::Other => "#5d6e3f",
        Gender::Unknown => "#555555",
    }
}

/// The tree around `focus` as a standalone SVG document.
pub fn tree_svg(tree: &Tree, focus: &str, settings: Settings) -> String {
    let l = layout::layout(tree, focus, settings);
    let b = l.bounds().expand(20.0);
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="{} {} {} {}" width="{}" height="{}" font-family="sans-serif">"#,
        b.min.x,
        b.min.y,
        b.width(),
        b.height(),
        b.width(),
        b.height()
    );
    let _ = writeln!(
        s,
        r##"<rect x="{}" y="{}" width="{}" height="{}" fill="#ffffff"/>"##,
        b.min.x,
        b.min.y,
        b.width(),
        b.height()
    );
    for e in &l.edges {
        let (a, z) = (e.from, e.to);
        match e.kind {
            EdgeKind::Couple => {
                let _ = writeln!(
                    s,
                    r##"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="#666" stroke-width="2.5"/>"##,
                    a.x, a.y, z.x, z.y
                );
            }
            EdgeKind::Child { adopted } => {
                let mid = z.y - (z.y - a.y).abs().min(50.0) / 2.0;
                let dash = if adopted { r#" stroke-dasharray="6 4""# } else { "" };
                let _ = writeln!(
                    s,
                    r##"<polyline points="{},{} {},{} {},{} {},{}" fill="none" stroke="#666" stroke-width="1.5"{dash}/>"##,
                    a.x, a.y, a.x, mid, z.x, mid, z.x, z.y
                );
            }
        }
    }
    for n in &l.nodes {
        let Some(p) = tree.person(&n.id) else { continue };
        let (x, y) = (n.pos.x - BOX.x / 2.0, n.pos.y - BOX.y / 2.0);
        let stroke = if n.id == focus { r##" stroke="#f0c040" stroke-width="3""## } else { "" };
        let _ = writeln!(
            s,
            r#"<rect x="{x}" y="{y}" width="{}" height="{}" rx="7" fill="{}"{stroke}/>"#,
            BOX.x,
            BOX.y,
            fill(p.gender)
        );
        let _ = writeln!(
            s,
            r##"<text x="{}" y="{}" text-anchor="middle" font-size="15" fill="#fff">{}</text>"##,
            n.pos.x,
            n.pos.y - 4.0,
            escape(&p.display_name(), false)
        );
        let _ = writeln!(
            s,
            r##"<text x="{}" y="{}" text-anchor="middle" font-size="13" fill="#e0e0e0">{}</text>"##,
            n.pos.x,
            n.pos.y + 16.0,
            escape(&p.lifespan(), false)
        );
    }
    s.push_str("</svg>\n");
    s
}

const STYLE: &str = "body{font-family:sans-serif;max-width:860px;margin:2em auto;color:#222}\
h1{margin-bottom:.2em}h2{border-bottom:1px solid #ccc;padding-bottom:.2em;margin-top:1.6em}\
table{border-collapse:collapse;width:100%}td,th{text-align:left;padding:.3em .6em;border-bottom:1px solid #eee;vertical-align:top}\
.muted{color:#777}.note{white-space:pre-wrap}@media print{body{margin:0}}";

fn h(s: &str) -> String {
    escape(s, false)
}

fn events_table(out: &mut String, p: &Person) {
    if p.facts.is_empty() {
        return;
    }
    out.push_str("<table><tr><th>Event</th><th>Date</th><th>Place</th><th>Description</th></tr>");
    for f in &p.facts {
        let _ = write!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            h(f.label()),
            h(&f.date.display()),
            h(&f.place),
            h(&f.details)
        );
    }
    out.push_str("</table>");
}

fn person_line(tree: &Tree, id: &str) -> String {
    tree.person(id)
        .map(|p| format!("{} <span class=muted>{}</span>", h(&p.display_name()), h(&p.lifespan())))
        .unwrap_or_default()
}

fn person_section(out: &mut String, tree: &Tree, p: &Person, heading: &str) {
    let _ = write!(out, "<{heading}>{}</{heading}><p class=muted>{}</p>", h(&p.display_name()), h(&p.lifespan()));
    events_table(out, p);
    for note in &p.notes {
        let _ = write!(out, "<p class=note>{}</p>", h(&note.plain_text()));
    }
    let list = |out: &mut String, title: &str, ids: Vec<&str>| {
        if !ids.is_empty() {
            let _ = write!(
                out,
                "<p><b>{title}:</b> {}</p>",
                ids.iter().map(|id| person_line(tree, id)).collect::<Vec<_>>().join(", ")
            );
        }
    };
    list(out, "Parents", tree.parents(&p.id));
    list(out, "Spouses", tree.spouses(&p.id));
    list(out, "Children", tree.children(&p.id));
}

fn page(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=utf-8><title>{}</title><style>{STYLE}</style></head><body>{body}\
         <p class=muted>Created with Rootkin</p></body></html>",
        h(title)
    )
}

/// A family group sheet for one person: their details, partners and children.
pub fn person_report(tree: &Tree, id: &str) -> String {
    let Some(p) = tree.person(id) else { return String::new() };
    let mut body = String::new();
    person_section(&mut body, tree, p, "h1");
    for link in tree.spouse_links_of(id) {
        let Some(spouse) = tree.person(link.other(id)) else { continue };
        body.push_str("<h2>Partner</h2>");
        person_section(&mut body, tree, spouse, "h3");
        for f in &link.facts {
            let _ = write!(body, "<p>{}: {} {}</p>", h(f.label()), h(&f.date.display()), h(&f.place));
        }
    }
    let kids = tree.children(id);
    if !kids.is_empty() {
        body.push_str("<h2>Children</h2>");
        for k in kids {
            if let Some(child) = tree.person(k) {
                person_section(&mut body, tree, child, "h3");
            }
        }
    }
    page(&format!("Family of {}", p.display_name()), &body)
}

/// Everyone in the tree, sorted by surname.
pub fn full_report(tree: &Tree) -> String {
    let mut people: Vec<&Person> = tree.people.iter().collect();
    people.sort_by_cached_key(|p| crate::app::sort_key(p));
    let mut body = format!("<h1>Family tree</h1><p class=muted>{} people</p>", tree.people.len());
    for p in people {
        body.push_str("<hr>");
        person_section(&mut body, tree, p, "h2");
    }
    page("Family tree", &body)
}

//! The import report: what came in, what was left out (grouped, in plain
//! words) and what the user may want to check.

use std::collections::BTreeMap;

use familyx::model::Tree;
use familyx::Package;

use crate::ImportReport;

/// One kind of thing an import left out or could not read, with a plain
/// explanation and the raw technical lines behind it.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReportGroup {
    pub title: String,
    pub count: usize,
    pub explanation: String,
    /// The technical details, as found in the file.
    pub items: Vec<String>,
}

/// Which group a dropped item belongs to, in display order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    AncestryLinks,
    OnlinePhotos,
    Photos,
    ChangeDates,
    Other,
}

impl Kind {
    /// `key` is a path like `INDI.BIRT.SOUR._APID` or `online photo`.
    fn of(key: &str) -> Self {
        let last = key.rsplit('.').next().unwrap_or(key);
        if key == "online photo" {
            Kind::OnlinePhotos
        } else if key.starts_with("photo ") {
            Kind::Photos
        } else if last == "_APID" || last == "_OID" {
            Kind::AncestryLinks
        } else if matches!(last, "_UPD" | "_UPDATED" | "_LAST_UPDATE" | "_CHANGED" | "_UPDATE" | "UPDATED") {
            Kind::ChangeDates
        } else {
            Kind::Other
        }
    }

    fn title(self) -> &'static str {
        match self {
            Kind::AncestryLinks => "Ancestry record links",
            Kind::OnlinePhotos => "Online photos",
            Kind::Photos => "Photos that could not be added",
            Kind::ChangeDates => "Change dates from other programs",
            Kind::Other => "Other details Rootkin can't show yet",
        }
    }

    fn explanation(self) -> &'static str {
        match self {
            Kind::AncestryLinks => "These links only work on ancestry.com, so they are of no use here.",
            Kind::OnlinePhotos => {
                "Rootkin works offline, so these photos were not downloaded. The links are kept in the person's notes."
            }
            Kind::Photos => {
                "Rootkin can use PNG and JPEG photos that sit next to the file (or inside the zip). These were missing or in another format."
            }
            Kind::ChangeDates => "Other programs note when each record was last edited. Rootkin does not use this.",
            Kind::Other => "The file has these, but Rootkin has no place for them yet. Nothing else is affected.",
        }
    }
}

/// Counts dropped items by a path like `INDI.SOUR`, reported once at the end.
#[derive(Default)]
pub(crate) struct Dropped(BTreeMap<String, usize>);

impl Dropped {
    pub(crate) fn add(&mut self, what: impl Into<String>) {
        self.add_n(what, 1);
    }

    pub(crate) fn add_n(&mut self, what: impl Into<String>, n: usize) {
        *self.0.entry(what.into()).or_default() += n;
    }

    /// Group the items for the report and add their raw lines to `warnings`.
    pub(crate) fn into_groups(self, warnings: &mut Vec<String>) -> Vec<ReportGroup> {
        let mut groups: BTreeMap<Kind, ReportGroup> = BTreeMap::new();
        for (what, n) in self.0 {
            let line = format!("dropped {n}x {what}");
            warnings.push(line.clone());
            let kind = Kind::of(&what);
            let group = groups.entry(kind).or_insert_with(|| ReportGroup {
                title: kind.title().into(),
                explanation: kind.explanation().into(),
                ..Default::default()
            });
            group.count += n;
            group.items.push(line);
        }
        groups.into_values().collect()
    }
}

/// What the importer found out beyond the tree itself.
#[derive(Default)]
pub(crate) struct Findings {
    pub(crate) dropped: Dropped,
    /// Odd things in the file that were worked around.
    pub(crate) warnings: Vec<String>,
    /// Links to records that are not in the file.
    pub(crate) broken_links: Vec<String>,
    pub(crate) tree_name: Option<String>,
}

/// Fill the report from the finished package and the importer's findings.
/// Odd things that are neither dropped items nor broken links become the
/// group of problems with the file; `report.warnings` lists everything raw.
pub(crate) fn finish(pkg: &Package, mut report: ImportReport, findings: Findings) -> ImportReport {
    let Findings { dropped, warnings: problems, broken_links, tree_name } = findings;
    let mut raw = problems.clone();
    raw.extend(broken_links.iter().cloned());
    report.left_out = dropped.into_groups(&mut raw);
    if !problems.is_empty() {
        report.left_out.push(ReportGroup {
            title: "Things that looked wrong in the file".into(),
            count: problems.len(),
            explanation: "Rootkin worked around these. Open the technical details to see them.".into(),
            items: problems,
        });
    }
    report.warnings = raw;
    report.broken_links = broken_links;
    report.tree_name = tree_name;

    let tree = Tree::from_doc(&pkg.doc);
    report.sources = tree.sources.len();
    report.photos = tree.media.len();
    report.duplicates = possible_duplicates(&tree);
    report
}

/// Pairs that look like the same person (see `familyx::duplicates`), described
/// for the report.
fn possible_duplicates(tree: &Tree) -> Vec<String> {
    let describe = |id: &str| {
        let p = tree.person(id).expect("candidates come from the tree");
        match p.fact("BIRT").and_then(|f| f.date.year) {
            Some(y) => format!("{} (born {y})", p.display_name()),
            None => format!("{} (birth year unknown)", p.display_name()),
        }
    };
    let mut out: Vec<String> = familyx::duplicates::find_duplicates(tree)
        .iter()
        .map(|d| format!("{} and {}", describe(&d.a), describe(&d.b)))
        .collect();
    out.sort();
    out
}

/// 1234567 -> "1,234,567"
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn count_of(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", grouped(n), if n == 1 { one } else { many })
}

impl ImportReport {
    /// "1,512 people, 604 families, 230 sources, 0 photos"
    pub fn headline(&self) -> String {
        [
            count_of(self.people, "person", "people"),
            count_of(self.families, "family", "families"),
            count_of(self.sources, "source", "sources"),
            count_of(self.photos, "photo", "photos"),
        ]
        .join(", ")
    }

    /// The whole report as plain text, for copying or saving.
    pub fn to_text(&self, file: &str) -> String {
        let mut text = format!("Rootkin import report for {file}\n\nCame in: {}\n", self.headline());
        if !self.left_out.is_empty() {
            text.push_str("\nLeft out or changed:\n");
            for g in &self.left_out {
                text.push_str(&format!("- {} ({}): {}\n", g.title, g.count, g.explanation));
            }
        }
        if !self.duplicates.is_empty() || !self.broken_links.is_empty() {
            text.push_str("\nTo check:\n");
            if !self.duplicates.is_empty() {
                text.push_str(&format!("- Possible duplicate people ({})\n", self.duplicates.len()));
                for d in &self.duplicates {
                    text.push_str(&format!("    {d}\n"));
                }
            }
            if !self.broken_links.is_empty() {
                text.push_str(&format!("- Links to records that are not in the file ({})\n", self.broken_links.len()));
                for b in &self.broken_links {
                    text.push_str(&format!("    {b}\n"));
                }
            }
        }
        if !self.warnings.is_empty() {
            text.push_str("\nTechnical details:\n");
            for w in &self.warnings {
                text.push_str(&format!("  {w}\n"));
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_numbers() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1512), "1,512");
        assert_eq!(grouped(1234567), "1,234,567");
    }

    #[test]
    fn classifies_dropped_items() {
        assert!(Kind::of("INDI.BIRT.SOUR._APID") == Kind::AncestryLinks);
        assert!(Kind::of("SOUR._APID") == Kind::AncestryLinks);
        assert!(Kind::of("online photo") == Kind::OnlinePhotos);
        assert!(Kind::of("photo whose file was not found") == Kind::Photos);
        assert!(Kind::of("INDI._UPD") == Kind::ChangeDates);
        assert!(Kind::of("INDI._CUSTOM") == Kind::Other);
    }
}

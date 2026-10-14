//! The import report: grouped plain-language entries, duplicates and broken links.

use rootkin_formats::{import, Format, ImportReport};

fn run(text: &str) -> ImportReport {
    import(text.as_bytes(), Format::Gedcom).unwrap().1
}

fn group<'a>(report: &'a ImportReport, title: &str) -> &'a rootkin_formats::ReportGroup {
    report
        .left_out
        .iter()
        .find(|g| g.title == title)
        .unwrap_or_else(|| panic!("no group {title}: {:?}", report.left_out))
}

const ANCESTRY: &str = concat!(
    "0 HEAD\n1 _TREE Kask Family Tree\n2 RIN 1\n",
    "0 @S1@ SOUR\n1 TITL Parish register\n1 _APID 1,2::3\n",
    "0 @I1@ INDI\n1 NAME Ott /Ilves/\n1 _UPD 1 JAN 2020\n",
    "1 BIRT\n2 DATE 1850\n2 SOUR @S1@\n3 _APID 1,2::4\n",
    "1 OBJE\n2 FILE https://example.com/a.jpg\n",
    "1 OBJE\n2 FILE missing.jpg\n",
    "1 _CUSTOM something\n",
    "0 @I2@ INDI\n1 NAME Ott /Ilves/\n1 BIRT\n2 DATE 1851\n",
    "0 @I3@ INDI\n1 NAME Ott /Ilves/\n1 BIRT\n2 DATE 1870\n",
    "0 @I4@ INDI\n1 NAME Mari /Tamm/\n1 FAMS @F9@\n1 FAMC @F8@\n",
    "0 @F1@ FAM\n1 HUSB @I9@\n1 WIFE @I1@\n1 CHIL @I404@\n",
    "0 TRLR\n"
);

#[test]
fn dropped_items_are_grouped_in_plain_language() {
    let r = run(ANCESTRY);
    let links = group(&r, "Ancestry record links");
    assert_eq!(links.count, 2);
    assert!(links.explanation.contains("ancestry.com"));
    assert_eq!(links.items.len(), 2);
    assert!(links.items.iter().any(|i| i.contains("INDI.BIRT.SOUR._APID")));

    let online = group(&r, "Online photos");
    assert_eq!(online.count, 1);
    assert!(online.explanation.contains("offline"));
    assert_eq!(group(&r, "Photos that could not be added").count, 1);
    assert_eq!(group(&r, "Change dates from other programs").count, 1);
    assert_eq!(group(&r, "Other details Rootkin can't show yet").count, 1);
    // the raw list stays complete
    assert!(r.warnings.iter().any(|w| w == "dropped 1x INDI._UPD"), "{:?}", r.warnings);
}

#[test]
fn counts_and_tree_name() {
    let r = run(ANCESTRY);
    assert_eq!((r.people, r.families, r.sources, r.photos), (4, 1, 1, 0));
    assert_eq!(r.tree_name.as_deref(), Some("Kask Family Tree"));
    assert_eq!(r.headline(), "4 people, 1 family, 1 source, 0 photos");
}

#[test]
fn possible_duplicates_are_same_name_with_close_birth_years() {
    let r = run(ANCESTRY);
    assert_eq!(r.duplicates, vec!["Ott Ilves (born 1850) and Ott Ilves (born 1851)".to_string()]);
}

#[test]
fn every_broken_link_is_reported() {
    let r = run(ANCESTRY);
    assert_eq!(r.broken_links.len(), 4, "{:?}", r.broken_links);
    for needle in ["@I9@ as husband", "@I404@ as a child", "spouse in family @F9@", "child in family @F8@"] {
        assert!(r.broken_links.iter().any(|b| b.contains(needle)), "{needle}: {:?}", r.broken_links);
    }
    // and they are part of the technical details
    assert!(r.warnings.iter().any(|w| w.contains("@I404@")));
}

#[test]
fn text_report_has_every_section() {
    let text = run(ANCESTRY).to_text("ancestry.ged");
    for needle in [
        "ancestry.ged",
        "Came in: 4 people",
        "Ancestry record links (2)",
        "Possible duplicate people (1)",
        "Links to records that are not in the file (4)",
        "Technical details:",
    ] {
        assert!(text.contains(needle), "{needle}\n{text}");
    }
}

#[test]
fn a_clean_file_has_an_empty_report() {
    let r = run("0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n0 TRLR\n");
    assert!(r.left_out.is_empty() && r.duplicates.is_empty() && r.broken_links.is_empty() && r.warnings.is_empty());
    assert_eq!(r.tree_name, None);
}

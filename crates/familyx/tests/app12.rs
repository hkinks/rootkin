//! Files for and from My Family Tree 12.x. `data/old/app12_6.familyx` is the fictional example tree as
//! My Family Tree 12.6.10 saved it (after opening a copy in the format `Package::to_bytes_for_app12` writes): a
//! `family7` V2 part, `Version` 14.4, the 88-character `FamilyId` and `IsResolved` on every photo.

use std::io::{Cursor, Read};

use familyx::codec::{self, AesVersion, SourceFormat};
use familyx::doc::Document;
use familyx::model::Tree;
use familyx::{edit, migrate, protection, Package};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

fn entries(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    (0..zip.len())
        .map(|i| {
            let mut entry = zip.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            (entry.name().to_string(), data)
        })
        .collect()
}

fn entry<'a>(entries: &'a [(String, Vec<u8>)], name: &str) -> &'a [u8] {
    &entries.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no {name}")).1
}

/// The raw tree XML of a package and the format it was stored in.
fn raw_tree(bytes: &[u8]) -> (Document, SourceFormat) {
    let entries = entries(bytes);
    let (name, blob) = entries.iter().find(|(n, _)| n.starts_with("family")).unwrap();
    let (xml, format) = codec::decode_part(name, blob).unwrap();
    (Document::parse(&xml).unwrap(), format)
}

#[test]
fn file_saved_by_app_12_6_opens() {
    let pkg = Package::open(format!("{DATA}/old/app12_6.familyx")).unwrap();
    assert_eq!(pkg.source_format(), SourceFormat::Family7(AesVersion::V2));
    let report = pkg.migration().expect("a 14.4 tree is converted");
    assert_eq!(report.from_version, "14.4");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(pkg.doc.root.child_text("FileProperties/Version").as_deref(), Some("14.8"));
    assert!(!pkg.is_password_protected());
    assert!(edit::check(&pkg.doc).is_empty(), "{:?}", edit::check(&pkg.doc));

    let tree = Tree::from_doc(&pkg.doc);
    let example = Tree::from_doc(&Package::open(format!("{DATA}/example.familyx")).unwrap().doc);
    assert_eq!(tree.people.len(), example.people.len());
    assert_eq!(tree.media.len(), 3);
    assert!(!String::from_utf8(pkg.doc.to_bytes()).unwrap().contains("IsResolved"));
    for media in tree.media.values() {
        assert!(pkg.part(&media.part).is_some(), "photo {} missing", media.part);
    }
    // saving writes the current format again
    let (_, format) = raw_tree(&pkg.to_bytes().unwrap());
    assert_eq!(format, SourceFormat::CURRENT);
}

#[test]
fn copy_for_app_12_has_its_format() {
    let pkg = Package::open(format!("{DATA}/example.familyx")).unwrap();
    let before = pkg.doc.to_bytes();
    let (bytes, downgrade) = pkg.to_bytes_for_app12().unwrap();
    assert_eq!(pkg.doc.to_bytes(), before, "the open package is not changed");
    assert_eq!(downgrade, Default::default());

    let parts = entries(&bytes);
    assert!(parts.iter().all(|(n, _)| n != "family8"));
    let types = String::from_utf8_lossy(entry(&parts, "[Content_Types].xml")).into_owned();
    assert!(types.contains("PartName=\"/family7\"") && !types.contains("/family8"), "{types}");
    let rels = String::from_utf8_lossy(entry(&parts, "_rels/.rels")).into_owned();
    assert!(rels.contains("Target=\"/family7\"") && !rels.contains("/family8"), "{rels}");

    let (doc, format) = raw_tree(&bytes);
    assert_eq!(format, SourceFormat::Family7(AesVersion::V2));
    assert_eq!(doc.root.child_text("FileProperties/Version").as_deref(), Some("14.4"));
    let family_id = doc.root.attr("FamilyId").unwrap().into_owned();
    assert_eq!(family_id.len(), 88);
    assert!(!protection::is_protected(protection::upgrade_legacy_family_id(Some(&family_id)).as_deref()));
    let files: Vec<_> = doc.root.child("PackagedFileCollection").unwrap().elements().collect();
    assert!(!files.is_empty());
    for file in files {
        assert_eq!(file.child_text("IsResolved").as_deref(), Some("true"));
    }

    // and it reads back as the same tree
    let again = Package::from_bytes(&bytes).unwrap();
    assert!(again.migration().unwrap().warnings.is_empty());
    let (a, b) = (Tree::from_doc(&pkg.doc), Tree::from_doc(&again.doc));
    assert_eq!(a.people.len(), b.people.len());
    assert_eq!(a.spouse_links.len(), b.spouse_links.len());
    assert_eq!(a.parent_links.len(), b.parent_links.len());
    assert_eq!(a.media.len(), b.media.len());
}

#[test]
fn password_lock_is_dropped_with_a_report() {
    let mut pkg = Package::open(format!("{DATA}/example.familyx")).unwrap();
    pkg.doc.root.set_attr("FamilyId", &protection::hash_password("secret"));
    let (bytes, downgrade) = pkg.to_bytes_for_app12().unwrap();
    assert!(downgrade.password_removed);
    assert!(!Package::from_bytes(&bytes).unwrap().is_password_protected());
}

const NAMES_14_8: &str = "<Translations><NameTranslation Id=\"t\" Creation=\"c\"><Name>Ivan</Name>\
<Language>ru</Language><PrefixParts /><NameParts><NamePart Id=\"a\" Creation=\"c\"><Name>Иван</Name></NamePart>\
<NamePart Id=\"b\" Creation=\"c\"><Name>Петрович</Name></NamePart></NameParts><NicknameParts />\
<SurnamePrefixParts /><SurnameParts><NamePart Id=\"d\" Creation=\"c\"><Name>Сидоров</Name></NamePart>\
</SurnameParts><SuffixParts /></NameTranslation></Translations>";
const NAMES_14_4: &str = "<Translations><NameTranslation Id=\"t\" Creation=\"c\"><Name>Ivan</Name>\
<Language>ru</Language><Prefixes /><Names><string>Иван</string><string>Петрович</string></Names><Nicknames />\
<SurnamePrefixes /><Surnames><string>Сидоров</string></Surnames><Suffixes /></NameTranslation></Translations>";

fn tree_xml(version: &str, translations: &str, task: &str, tag: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?><Family xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\
<PeopleCollection><Person Id=\"p\" Creation=\"c\"><PersonNameCollection><Names><Name Id=\"n\" Creation=\"c\">\
<Names>Ivan</Names><IsPrimary>true</IsPrimary>{translations}</Name></Names></PersonNameCollection>\
<Facts><Fact Id=\"f\" Creation=\"c\"><Tag>{tag}</Tag></Fact></Facts></Person></PeopleCollection>\
<TaskCollection><Task Id=\"k\" Creation=\"c\"><Category>Research</Category>{task}<Title>Find</Title></Task>\
</TaskCollection><FileProperties Id=\"v\" Creation=\"c\"><Version>{version}</Version></FileProperties></Family>"
    )
    .into_bytes()
}

/// The XML with record ids and timestamps removed, for comparing generated records.
fn strip_ids(doc: &Document) -> String {
    let xml = String::from_utf8(doc.to_bytes()).unwrap();
    let (mut out, mut rest) = (String::new(), xml.as_str());
    while let Some((i, len)) =
        [" Id=\"", " Creation=\""].iter().filter_map(|a| rest.find(a).map(|i| (i, a.len()))).min()
    {
        out.push_str(&rest[..i]);
        let value = &rest[i + len..];
        rest = &value[value.find('"').unwrap() + 1..];
    }
    out + rest
}

#[test]
fn schema_14_steps_both_ways() {
    let task_new = "<PlaceDetails Id=\"x\" Creation=\"c\"><Description>Riga archive</Description></PlaceDetails>";
    let task_old = "<Location>Riga archive</Location>";
    let mut old = Document::parse(&tree_xml("14.4", NAMES_14_4, task_old, "EVEN_WAC")).unwrap();
    migrate::migrate(&mut old, &|_| None).unwrap();
    let new = Document::parse(&tree_xml("14.8", NAMES_14_8, task_new, "INIL")).unwrap();
    assert_eq!(strip_ids(&old), strip_ids(&new));

    let mut down = new.clone();
    let downgrade = migrate::schema14::downgrade(&mut down, &|_| false);
    assert_eq!(downgrade.task_places, 0);
    let mut expected = Document::parse(&tree_xml("14.4", NAMES_14_4, task_old, "INIL")).unwrap();
    let family_id = down.root.attr("FamilyId").unwrap().into_owned();
    expected.root.set_attr("FamilyId", &family_id);
    assert_eq!(strip_ids(&down), strip_ids(&expected));
}

#[test]
fn initiatory_tags_follow_their_gates() {
    for (version, tag, expected) in [
        ("13.0", "INIT", "INIL"),
        ("14.2", "INIT", "INIT"),
        ("14.2", "EVEN_WAC", "INIL"),
        ("14.5", "EVEN_WAC", "EVEN_WAC"),
    ] {
        let mut doc = Document::parse(&tree_xml(version, "", "", tag)).unwrap();
        migrate::migrate(&mut doc, &|_| None).unwrap();
        let fact = doc.root.path("PeopleCollection/Person/Facts/Fact").unwrap();
        assert_eq!(fact.child_text("Tag").as_deref(), Some(expected), "{tag} at {version}");
    }
}

#[test]
fn task_place_detail_beyond_the_description_is_reported() {
    let task = "<PlaceDetails Id=\"x\" Creation=\"c\"><Description>Riga</Description>\
<Latitude>56.9</Latitude></PlaceDetails>";
    let mut doc = Document::parse(&tree_xml("14.8", "", task, "BIRT")).unwrap();
    let downgrade = migrate::schema14::downgrade(&mut doc, &|_| false);
    assert_eq!(downgrade.task_places, 1);
    assert_eq!(doc.root.child_text("TaskCollection/Task/Location").as_deref(), Some("Riga"));
}

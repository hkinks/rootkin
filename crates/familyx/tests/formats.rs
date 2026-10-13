//! Reading older `.familyx` variants. The vectors in `tests/data/old` hold the XML of the
//! fictional example tree in each older encryption variant. They were produced by an
//! independent implementation of the documented algorithms and accepted by the Windows
//! app's own decryption code (see docs/design.md).

use familyx::codec::{self, AesVersion, SourceFormat};
use familyx::{model::Tree, protection, Package};
use std::io::{Cursor, Read, Write};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data");

fn read_zip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
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

fn write_zip(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in parts {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// The (fictional) XML the old-format vectors in `data/old/` were encrypted from.
fn fixture_xml() -> Vec<u8> {
    std::fs::read(format!("{DATA}/old/plaintext.xml")).unwrap()
}

/// A package in the old layout: content types and relationships name `/<part>`.
fn old_package(part: &str, blob: &[u8]) -> Vec<u8> {
    let types = format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"png\" ContentType=\"image/png\" />\
         <Override PartName=\"/{part}\" ContentType=\"application/vnd.chronoplexsoftware.familyx\" /></Types>"
    );
    let rels = format!(
        "\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Type=\"MyFamilyTree/Relationship\" Target=\"/{part}\" Id=\"Rabc\" /></Relationships>"
    );
    write_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        (part, blob),
        ("Multimedia/x.png", b"\x89PNG fake"),
    ])
}

const VECTORS: [(&str, &str, SourceFormat); 8] = [
    ("family8_v1", "family8", SourceFormat::Family8(AesVersion::V1)),
    ("family8_v2", "family8", SourceFormat::Family8(AesVersion::V2)),
    ("family7_v1", "family7", SourceFormat::Family7(AesVersion::V1)),
    ("family7_v2", "family7", SourceFormat::Family7(AesVersion::V2)),
    ("family7_v3", "family7", SourceFormat::Family7(AesVersion::V3)),
    ("family5", "family5", SourceFormat::Family5),
    ("family4", "family4", SourceFormat::Family4),
    ("family", "family", SourceFormat::Family),
];

#[test]
fn independent_vectors_decode_to_the_example_xml() {
    let xml = fixture_xml();
    for (file, part, format) in VECTORS {
        let blob = std::fs::read(format!("{DATA}/old/{file}")).unwrap();
        let (decoded, detected) = codec::decode_part(part, &blob).unwrap();
        assert!(decoded == xml, "{file}: XML differs");
        assert_eq!(detected, format, "{file}");
    }
}

#[test]
fn old_packages_open_and_save_as_family8_v3() {
    let xml = fixture_xml();
    let expected = Tree::from_doc(&familyx::doc::Document::parse(&xml).unwrap());
    for (file, part, format) in VECTORS {
        let blob = std::fs::read(format!("{DATA}/old/{file}")).unwrap();
        let pkg = Package::from_bytes(&old_package(part, &blob)).unwrap();
        assert_eq!(pkg.source_format(), format, "{file}");
        assert!(!pkg.is_password_protected(), "{file}");
        let tree = Tree::from_doc(&pkg.doc);
        assert_eq!(tree.people.len(), expected.people.len(), "{file}");
        assert_eq!(tree.parent_links.len(), expected.parent_links.len(), "{file}");
        assert_eq!(pkg.part("Multimedia/x.png"), Some(&b"\x89PNG fake"[..]));

        let saved = pkg.to_bytes().unwrap();
        let entries = read_zip(&saved);
        let names: Vec<&str> = entries.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"family8"), "{file}: {names:?}");
        assert!(
            !names.contains(&"family7") && !names.contains(&"family5"),
            "{file}: old part must not be written back"
        );
        let text = |name: &str| String::from_utf8(entries.iter().find(|(n, _)| n == name).unwrap().1.clone()).unwrap();
        if part != "family8" {
            assert!(text("[Content_Types].xml").contains("PartName=\"/family8\""), "{file}");
            assert!(!text("[Content_Types].xml").contains(&format!("PartName=\"/{part}\"")), "{file}");
            assert!(text("_rels/.rels").contains("Target=\"/family8\""), "{file}");
            assert!(!text("_rels/.rels").contains(&format!("Target=\"/{part}\"")), "{file}");
        }

        let reopened = Package::from_bytes(&saved).unwrap();
        assert_eq!(reopened.source_format(), SourceFormat::CURRENT);
        assert!(reopened.doc == pkg.doc, "{file}");
    }
}

#[test]
fn plain_content_xml_is_read() {
    let xml = fixture_xml();
    let pkg = Package::from_bytes(&old_package("content.xml", &xml)).unwrap();
    assert_eq!(pkg.source_format(), SourceFormat::ContentXml);
    assert_eq!(Tree::from_doc(&pkg.doc).people.len(), 17);
}

#[test]
fn newest_tree_part_wins_and_others_are_kept() {
    let xml = fixture_xml();
    let current = codec::encode(&xml);
    let old = codec::encode_part(b"\xef\xbb\xbf<Family />", SourceFormat::Family7(AesVersion::V2));
    let zip = write_zip(&[("family7", &old), ("family8", &current)]);
    let pkg = Package::from_bytes(&zip).unwrap();
    assert_eq!(pkg.source_format(), SourceFormat::CURRENT);
    assert_eq!(Tree::from_doc(&pkg.doc).people.len(), 17);
    assert!(pkg.part("family7") == Some(&old[..]));
}

#[test]
fn package_without_tree_part_is_an_error() {
    let zip = write_zip(&[("something", b"x")]);
    assert!(Package::from_bytes(&zip).is_err());
}

#[test]
fn legacy_family_id_is_upgraded_on_import() {
    // A user-protected tree from an old version: bare base64(salt)+base64(hash), SHA-256 / 10000.
    let xml = String::from_utf8(fixture_xml()).unwrap();
    let start = xml.find("FamilyId=\"").unwrap() + "FamilyId=\"".len();
    let end = start + xml[start..].find('"').unwrap();
    let legacy = "A".repeat(43) + "=" + &"B".repeat(43) + "=";
    assert_eq!(legacy.len(), 88);
    let old_xml = format!("{}{}{}", &xml[..start], legacy, &xml[end..]);
    let blob = codec::encode_part(old_xml.as_bytes(), SourceFormat::Family5);
    let pkg = Package::from_bytes(&old_package("family5", &blob)).unwrap();
    let id = pkg.doc.root.attr("FamilyId").unwrap().into_owned();
    assert_eq!(id.split('\0').count(), 4);
    assert!(id.ends_with(&legacy[44..]));
    assert!(pkg.is_password_protected(), "a legacy non-default hash stays protected");
}

#[test]
fn new_files_and_the_example_are_not_password_protected() {
    assert!(!Package::new().is_password_protected());
    let example = Package::open(format!("{DATA}/example.familyx")).unwrap();
    assert!(!example.is_password_protected());
    assert!(!protection::document_is_protected(&example.doc));
}

#[test]
fn legacy_schema_is_migrated_on_open() {
    let current = Package::open(format!("{DATA}/example.familyx")).unwrap();
    assert!(!current.has_legacy_schema());
    assert!(current.migration().is_none());
    // An old tree whose only trace of the old schema is an empty list is converted completely.
    let old_xml = "\u{feff}<Family FamilyId=\"x\"><PeopleCollection><Person Id=\"p\"><Relationships /></Person>\
                   </PeopleCollection></Family>";
    let blob = codec::encode_part(old_xml.as_bytes(), SourceFormat::Family4);
    let pkg = Package::from_bytes(&old_package("family4", &blob)).unwrap();
    assert!(pkg.migration().is_some());
    assert!(!pkg.has_legacy_schema());
    assert_eq!(pkg.doc.root.child_text("FileProperties/Version").as_deref(), Some("14.8"));
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert!(reopened.migration().is_none());
    assert!(reopened.doc == pkg.doc);
}

#[test]
fn unconvertible_legacy_elements_are_kept_and_flagged() {
    // A database-backed story cannot be read: it stays verbatim and keeps `has_legacy_schema` true.
    let old_xml =
        "\u{feff}<Family FamilyId=\"x\"><PeopleCollection><Person Id=\"p\"><Story><FileId>f1</FileId></Story></Person>\
                   </PeopleCollection></Family>";
    let blob = codec::encode_part(old_xml.as_bytes(), SourceFormat::Family4);
    let pkg = Package::from_bytes(&old_package("family4", &blob)).unwrap();
    assert!(pkg.has_legacy_schema());
    let reopened = Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    assert!(reopened.has_legacy_schema());
    assert!(reopened.doc == pkg.doc);
}

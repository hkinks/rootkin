//! Broken or hostile imports give an error or a bounded result, never a crash.

use std::io::{Cursor, Write};

use rootkin_formats::Format;

fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(256 << 10).spawn(f).unwrap().join().expect("no crash")
}

#[test]
fn deeply_nested_gedcom_is_refused() {
    let mut ged = String::from("0 HEAD\n0 @I1@ INDI\n1 NAME A /B/\n");
    for level in 1..100_000 {
        ged.push_str(&format!("{level} NOTE x\n"));
    }
    ged.push_str("0 TRLR\n");
    let err =
        on_small_stack(move || rootkin_formats::import(ged.as_bytes(), Format::Gedcom).err().unwrap().to_string());
    assert!(err.contains("nested more than 256 levels"), "{err}");
}

#[test]
fn deeply_nested_gramps_xml_is_refused() {
    let xml = format!("<database>{}{}</database>", "<a>".repeat(100_000), "</a>".repeat(100_000));
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gz.write_all(xml.as_bytes()).unwrap();
    let bytes = gz.finish().unwrap();
    let err = on_small_stack(move || rootkin_formats::import(&bytes, Format::Gramps).err().unwrap().to_string());
    assert!(err.contains("nested more than 256 levels"), "{err}");
}

#[test]
fn a_photo_referenced_many_times_in_a_gdz_is_stored_once() {
    let mut ged = String::from("0 HEAD\n");
    for i in 0..40 {
        ged.push_str(&format!("0 @I{i}@ INDI\n1 NAME A{i} /B/\n1 OBJE\n2 FILE a.jpg\n"));
    }
    ged.push_str("0 TRLR\n");
    let photo = vec![0xAB; 64 << 10];
    let mut out = Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut out);
    for (name, data) in [("gedcom.ged", ged.as_bytes()), ("a.jpg", &photo[..])] {
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
    let (pkg, report) = rootkin_formats::import_zip(&out.into_inner()).unwrap();
    assert_eq!(report.people, 40);
    let media: Vec<&str> = pkg.part_names().filter(|n| n.starts_with("Multimedia/")).collect();
    assert_eq!(media.len(), 1, "one file for all 40 references");
    let tree = familyx::model::Tree::from_doc(&pkg.doc);
    assert_eq!(tree.media.len(), 1);
    assert!(tree.people.iter().all(|p| p.primary_image.is_some()));
    assert!(familyx::edit::check(&pkg.doc).is_empty());
}

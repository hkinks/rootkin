//! Broken or hostile input gives an error, never a crash or unbounded memory use.

use std::io::{Cursor, Write};

use familyx::Package;

fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Cursor::new(Vec::new());
    let mut w = zip::ZipWriter::new(&mut out);
    for (name, data) in files {
        w.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap();
    out.into_inner()
}

fn nested(depth: usize) -> String {
    format!("<Family>{}{}</Family>", "<a>".repeat(depth), "</a>".repeat(depth))
}

/// On a thread with a small stack, as a background job would run.
fn on_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new().stack_size(256 << 10).spawn(f).unwrap().join().expect("no crash")
}

#[test]
fn deeply_nested_xml_is_refused() {
    let bytes = zip_of(&[("content.xml", nested(100_000).as_bytes())]);
    let err = on_small_stack(move || Package::from_bytes(&bytes).err().map(|e| e.to_string())).expect("an error");
    assert!(err.contains("nested more than 256 levels"), "{err}");
    // real trees nest far less; 200 levels are fine and round-trip
    let bytes = zip_of(&[("content.xml", nested(200).as_bytes())]);
    on_small_stack(move || {
        let pkg = Package::from_bytes(&bytes).unwrap();
        Package::from_bytes(&pkg.to_bytes().unwrap()).unwrap();
    });
}

#[test]
fn declared_zip_sizes_are_not_trusted() {
    let mut bytes = zip_of(&[("content.xml", b"<Family />")]);
    // claim 4 GB uncompressed in the central directory and local header
    let central = bytes.windows(4).position(|x| x == [0x50, 0x4b, 0x01, 0x02]).unwrap();
    bytes[central + 24..central + 28].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    bytes[22..26].copy_from_slice(&0xFFFF_FFFEu32.to_le_bytes());
    // whatever the zip reader makes of it, there is no huge allocation and no crash
    let _ = on_small_stack(move || Package::from_bytes(&bytes).map(|_| ()).map_err(|e| e.to_string()));
}

#[test]
fn reading_stops_at_the_limit_with_a_clear_error() {
    let mut gz = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&vec![b' '; 5 << 20]).unwrap();
    let bomb = gz.finish().unwrap();
    assert!(bomb.len() < 20_000);
    let reader = flate2::read::DeflateDecoder::new(&bomb[..]);
    let err = familyx::limits::read_limited(reader, 1 << 20, "photo.png").unwrap_err().to_string();
    assert!(err.contains("photo.png unpacks to more than 1 MB"), "{err}");
    let ok = familyx::limits::read_limited(flate2::read::DeflateDecoder::new(&bomb[..]), 5 << 20, "x").unwrap();
    assert_eq!(ok.len(), 5 << 20);
}

// ---- LiteDB (the old story database) whose primary-key index loops

const PS: usize = 4096;

fn page_header(p: &mut [u8], id: u32, ty: u8, items: u16) {
    p[0..4].copy_from_slice(&id.to_le_bytes());
    p[4] = ty;
    p[13..15].copy_from_slice(&items.to_le_bytes());
}

fn string(v: &mut Vec<u8>, t: &str) {
    v.extend((t.len() as i32).to_le_bytes());
    v.extend(t.as_bytes());
}

fn address(v: &mut Vec<u8>, page: u32, index: u16) {
    v.extend(page.to_le_bytes());
    v.extend(index.to_le_bytes());
}

/// `_files` whose index skip list runs head -> 1 -> 2 -> 1 -> ..., plus blank pages.
fn looping_litedb(pages: usize) -> Vec<u8> {
    let mut f = vec![0u8; PS * pages];
    {
        let p = &mut f[0..PS];
        page_header(p, 0, 1, 0);
        let mut v = Vec::new();
        v.extend(b"** This is a LiteDB file **");
        v.push(7);
        v.extend([0u8; 12]);
        v.extend([0u8; 36]);
        v.push(1);
        string(&mut v, "_files");
        v.extend(1u32.to_le_bytes());
        p[25..25 + v.len()].copy_from_slice(&v);
    }
    {
        let p = &mut f[PS..2 * PS];
        page_header(p, 1, 2, 0);
        let mut v = Vec::new();
        string(&mut v, "_files");
        v.extend(0i64.to_le_bytes());
        v.extend(u32::MAX.to_le_bytes());
        string(&mut v, "_id");
        v.push(1);
        address(&mut v, 2, 0);
        address(&mut v, 2, 3);
        p[25..25 + v.len()].copy_from_slice(&v);
    }
    {
        let p = &mut f[2 * PS..3 * PS];
        page_header(p, 2, 3, 4);
        let mut v = Vec::new();
        for (index, next) in [(0u16, 1u16), (1, 2), (2, 1), (3, 0xffff)] {
            v.extend(index.to_le_bytes());
            v.push(1);
            v.push(0);
            address(&mut v, u32::MAX, 0);
            address(&mut v, u32::MAX, 0);
            v.extend(0u16.to_le_bytes());
            v.push(0);
            address(&mut v, 3, 0);
            address(&mut v, u32::MAX, 0);
            address(&mut v, if next == 0xffff { u32::MAX } else { 2 }, next);
        }
        p[25..25 + v.len()].copy_from_slice(&v);
    }
    {
        let p = &mut f[3 * PS..4 * PS];
        page_header(p, 3, 4, 1);
        let body = "x".repeat(4000);
        let mut inner = vec![2];
        inner.extend(b"_id\0");
        inner.extend(((body.len() + 1) as i32).to_le_bytes());
        inner.extend(body.as_bytes());
        inner.extend([0, 0]);
        let mut doc = ((inner.len() + 4) as i32).to_le_bytes().to_vec();
        doc.extend(inner);
        let mut v = Vec::new();
        v.extend(0u16.to_le_bytes());
        v.extend(u32::MAX.to_le_bytes());
        v.extend((doc.len() as u16).to_le_bytes());
        v.extend(doc);
        p[25..25 + v.len()].copy_from_slice(&v);
    }
    f
}

#[test]
fn a_looping_story_database_index_is_an_error() {
    let started = std::time::Instant::now();
    let err = familyx::litedb::LiteDb::open(&looping_litedb(256), &[]).unwrap_err().to_string();
    assert!(err.contains("loop") || err.contains("share"), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn deeply_nested_bson_is_an_error() {
    // one document holding documents 400 levels deep, in the looping file's data block
    let mut f = looping_litedb(4);
    // make the index well-formed: head -> 1 -> tail
    let p = &mut f[2 * PS..3 * PS];
    let mut v = Vec::new();
    for (index, next) in [(0u16, 1u16), (1, 3), (2, 3), (3, 0xffff)] {
        v.extend(index.to_le_bytes());
        v.push(1);
        v.push(0);
        address(&mut v, u32::MAX, 0);
        address(&mut v, u32::MAX, 0);
        v.extend(0u16.to_le_bytes());
        v.push(0);
        address(&mut v, 3, 0);
        address(&mut v, u32::MAX, 0);
        address(&mut v, if next == 0xffff { u32::MAX } else { 2 }, next);
    }
    p[25..25 + v.len()].copy_from_slice(&v);
    // a document nested 400 levels: each level is `03 "d\0" <doc>`
    fn nest(depth: usize) -> Vec<u8> {
        let mut inner = Vec::new();
        if depth > 0 {
            inner.push(3);
            inner.extend(b"d\0");
            inner.extend(nest(depth - 1));
        }
        inner.push(0);
        let mut doc = ((inner.len() + 4) as i32).to_le_bytes().to_vec();
        doc.extend(inner);
        doc
    }
    let doc = nest(400);
    assert!(doc.len() < 4000);
    let p = &mut f[3 * PS..4 * PS];
    p[25..].fill(0);
    let mut v = Vec::new();
    v.extend(0u16.to_le_bytes());
    v.extend(u32::MAX.to_le_bytes());
    v.extend((doc.len() as u16).to_le_bytes());
    v.extend(doc);
    p[25..25 + v.len()].copy_from_slice(&v);
    let err = on_small_stack(move || familyx::litedb::LiteDb::open(&f, &[]).unwrap_err().to_string());
    assert!(err.contains("nested too deeply"), "{err}");
}

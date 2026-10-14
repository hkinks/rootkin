//! A minimal read-only reader for LiteDB v4 data files (file format version 7), enough to read the
//! `FileStorage` collections (`_files`, `_chunks`) of the story database that old versions of the app
//! kept next to the tree (`stories7`, see `crate::migrate`).
//!
//! Written from the public LiteDB 4.1.x format (MIT, <https://github.com/mbdavid/LiteDB>):
//!
//! * the file is a sequence of 4096-byte pages; page 0 is the header page and never encrypted;
//! * a password protects every other page with AES-256-CBC (no padding, the same key and IV for all
//!   pages) with `key | iv = PBKDF2-HMAC-SHA1(password as UTF-8, salt from the header, 1000 iterations)`;
//!   the header stores `SHA1(password)` for the check;
//! * every page starts with a 25-byte header (`id u32`, `type u8`, `prev u32`, `next u32`,
//!   `item count u16`, `free bytes u16`, 8 reserved);
//! * a collection page lists 16 indexes, the first one is the primary key; its skip list
//!   (`IndexNode`s on index pages) leads to the documents, which are BSON in data blocks, or in a chain of
//!   extend pages if they do not fit a page;
//! * `FileStorage` keeps file records in `_files` (`_id`, `filename`, `length`, `chunks`) and the content
//!   in `_chunks` (`_id` = `<file id>\<index:00000>`, `data` binary, 255 KiB each).
//!
//! Deleted or rewritten documents may leave unreferenced blocks in the file; only what the primary-key
//! index reaches is read.

use std::collections::HashMap;

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use sha1::{Digest, Sha1};

use crate::{Error, Result};

const PAGE_SIZE: usize = 4096;
const PAGE_HEADER: usize = 25;
const HEADER_INFO: &[u8] = b"** This is a LiteDB file **";
const FILE_VERSION: u8 = 7;
const NO_PAGE: u32 = u32::MAX;

fn bad(what: &str) -> Error {
    Error::Format(format!("litedb: {what}"))
}

/// A file stored in the database (`FileStorage`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredFile {
    pub id: String,
    pub filename: String,
    pub length: u64,
}

/// An opened (fully decrypted, in memory) LiteDB file.
#[derive(Debug)]
pub struct LiteDb {
    files: Vec<StoredFile>,
    chunks: HashMap<String, Vec<u8>>,
}

impl LiteDb {
    /// Open a data file. If it is encrypted, the first of `passwords` that matches the stored hash is used.
    pub fn open(bytes: &[u8], passwords: &[&str]) -> Result<Self> {
        let pages = Pages::new(bytes, passwords)?;
        let header = pages.page(0)?;
        let mut files = Vec::new();
        let mut chunks = HashMap::new();
        if let Some(root) = collection_page(&pages, header, "_files")? {
            for doc in documents(&pages, root)? {
                let (Some(id), Some(filename)) =
                    (doc.get("_id").and_then(Bson::as_str), doc.get("filename").and_then(Bson::as_str))
                else {
                    continue;
                };
                let length = doc.get("length").and_then(Bson::as_i64).unwrap_or(0).max(0) as u64;
                files.push(StoredFile { id: id.to_string(), filename: filename.to_string(), length });
            }
        }
        if let Some(root) = collection_page(&pages, header, "_chunks")? {
            for doc in documents(&pages, root)? {
                if let (Some(id), Some(Bson::Bin(data))) = (doc.get("_id").and_then(Bson::as_str), doc.get("data")) {
                    chunks.insert(id.to_string(), data.clone());
                }
            }
        }
        Ok(Self { files, chunks })
    }

    /// All stored files.
    pub fn files(&self) -> &[StoredFile] {
        &self.files
    }

    /// The content of the file with this id (chunks `0..` concatenated), `None` if there is no such file.
    pub fn read(&self, id: &str) -> Option<Vec<u8>> {
        self.files.iter().find(|f| f.id == id)?;
        let mut out = Vec::new();
        for index in 0.. {
            match self.chunks.get(&format!("{id}\\{index:05}")) {
                Some(data) => out.extend_from_slice(data),
                None => break,
            }
        }
        Some(out)
    }
}

// ------------------------------------------------------------------ pages

struct Pages {
    /// Decrypted pages (page 0 as stored).
    data: Vec<u8>,
}

impl Pages {
    fn new(bytes: &[u8], passwords: &[&str]) -> Result<Self> {
        if bytes.len() < PAGE_SIZE {
            return Err(bad("file shorter than one page"));
        }
        let header = &bytes[..PAGE_SIZE];
        let info = &header[PAGE_HEADER..PAGE_HEADER + HEADER_INFO.len()];
        if header[4] != 1 || info != HEADER_INFO {
            return Err(bad("not a LiteDB file"));
        }
        let version = header[PAGE_HEADER + HEADER_INFO.len()];
        if version != FILE_VERSION {
            return Err(bad(&format!("unsupported data file version {version}")));
        }
        let at = PAGE_HEADER + HEADER_INFO.len() + 1 + 2 + 4 + 4 + 2; // after UserVersion
        let stored_hash = &header[at..at + 20];
        let salt = &header[at + 20..at + 36];
        let mut data = bytes[..bytes.len() / PAGE_SIZE * PAGE_SIZE].to_vec();
        if stored_hash.iter().any(|&b| b != 0) {
            let password = passwords
                .iter()
                .find(|p| Sha1::digest(p.as_bytes()).as_slice() == stored_hash)
                .ok_or_else(|| bad("the database is password protected and none of the known passwords fits"))?;
            let mut key_iv = [0u8; 48];
            pbkdf2::pbkdf2_hmac::<Sha1>(password.as_bytes(), salt, 1000, &mut key_iv);
            for page in data.as_chunks_mut::<PAGE_SIZE>().0.iter_mut().skip(1) {
                cbc::Decryptor::<aes::Aes256>::new(key_iv[..32].into(), key_iv[32..].into())
                    .decrypt_padded_mut::<NoPadding>(page.as_mut_slice())
                    .map_err(|_| bad("page decryption failed"))?;
            }
        }
        Ok(Self { data })
    }

    fn count(&self) -> usize {
        self.data.len() / PAGE_SIZE
    }

    fn page(&self, id: u32) -> Result<&[u8]> {
        let id = id as usize;
        if id >= self.count() {
            return Err(bad(&format!("page {id} is beyond the end of the file")));
        }
        Ok(&self.data[id * PAGE_SIZE..(id + 1) * PAGE_SIZE])
    }

    /// A page of this type, whose own id field matches.
    fn typed(&self, id: u32, page_type: u8) -> Result<&[u8]> {
        let page = self.page(id)?;
        if page[4] != page_type || u32::from_le_bytes(page[..4].try_into().unwrap()) != id {
            return Err(bad(&format!("page {id} is not a page of type {page_type} (wrong password or corrupt file?)")));
        }
        Ok(page)
    }
}

/// A bounds-checked little-endian reader.
struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn at(buf: &'a [u8], pos: usize) -> Self {
        Self { buf, pos }
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).filter(|&e| e <= self.buf.len()).ok_or_else(|| bad("truncated data"))?;
        let out = &self.buf[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }

    /// A page address: page id and index within the page.
    fn address(&mut self) -> Result<(u32, u16)> {
        Ok((self.u32()?, self.u16()?))
    }

    /// A string with an `i32` byte length prefix.
    fn string(&mut self) -> Result<String> {
        let n = usize::try_from(self.i32()?).map_err(|_| bad("negative string length"))?;
        Ok(String::from_utf8_lossy(self.bytes(n)?).into_owned())
    }

    fn cstring(&mut self) -> Result<String> {
        let rest = &self.buf[self.pos.min(self.buf.len())..];
        let n = rest.iter().position(|&b| b == 0).ok_or_else(|| bad("unterminated name"))?;
        self.pos += n + 1;
        Ok(String::from_utf8_lossy(&rest[..n]).into_owned())
    }
}

/// The page id of the named collection's collection page (names are case-insensitive).
fn collection_page<'a>(pages: &'a Pages, header: &[u8], name: &str) -> Result<Option<&'a [u8]>> {
    let mut r = Reader::at(header, PAGE_HEADER + HEADER_INFO.len() + 1 + 2 + 4 + 4 + 2 + 20 + 16);
    let count = r.u8()?;
    for _ in 0..count {
        let (collection, page_id) = (r.string()?, r.u32()?);
        if collection.eq_ignore_ascii_case(name) {
            return pages.typed(page_id, 2).map(Some);
        }
    }
    Ok(None)
}

/// All documents of a collection, in primary-key order.
fn documents(pages: &Pages, collection: &[u8]) -> Result<Vec<Bson>> {
    let mut r = Reader::at(collection, PAGE_HEADER);
    r.string()?; // collection name
    r.i64()?; // document count
    r.u32()?; // free data page
    let _field = r.string()?; // primary key index: "_id"
    r.u8()?; // unique
    let head = r.address()?;
    let tail = r.address()?;

    let mut index_pages: HashMap<u32, HashMap<u16, IndexNode>> = HashMap::new();
    let mut out = Vec::new();
    let mut node = head;
    let mut guard = 0usize;
    loop {
        if let std::collections::hash_map::Entry::Vacant(entry) = index_pages.entry(node.0) {
            entry.insert(index_nodes(pages.typed(node.0, 3)?)?);
        }
        let current = index_pages[&node.0].get(&node.1).ok_or_else(|| bad("dangling index node"))?;
        if node != head && node != tail {
            out.push(read_document(pages, current.block)?);
        }
        let next = current.next;
        if next.0 == NO_PAGE || next == tail {
            break;
        }
        node = next;
        guard += 1;
        if guard > pages.count() * PAGE_SIZE {
            return Err(bad("index loop"));
        }
    }
    Ok(out)
}

struct IndexNode {
    block: (u32, u16),
    /// Next node on level 0 of the skip list.
    next: (u32, u16),
}

fn index_nodes(page: &[u8]) -> Result<HashMap<u16, IndexNode>> {
    let count = u16::from_le_bytes(page[13..15].try_into().unwrap());
    let mut r = Reader::at(page, PAGE_HEADER);
    let mut nodes = HashMap::new();
    for _ in 0..count {
        let index = r.u16()?;
        let levels = r.u8()?;
        r.u8()?; // slot
        r.address()?; // prev node (same key)
        r.address()?; // next node (same key)
        let key_length = usize::from(r.u16()?);
        r.bytes(1 + key_length)?; // BSON type + key value
        let block = r.address()?;
        let mut next = (NO_PAGE, u16::MAX);
        for level in 0..levels {
            r.address()?; // prev
            let n = r.address()?;
            if level == 0 {
                next = n;
            }
        }
        nodes.insert(index, IndexNode { block, next });
    }
    Ok(nodes)
}

/// The BSON document in a data block, following the extend pages of a large one.
fn read_document(pages: &Pages, block: (u32, u16)) -> Result<Bson> {
    let page = pages.typed(block.0, 4)?;
    let count = u16::from_le_bytes(page[13..15].try_into().unwrap());
    let mut r = Reader::at(page, PAGE_HEADER);
    for _ in 0..count {
        let index = r.u16()?;
        let extend = r.u32()?;
        let size = usize::from(r.u16()?);
        let data = r.bytes(size)?;
        if index != block.1 {
            continue;
        }
        if extend == NO_PAGE {
            return Bson::parse_document(data);
        }
        let mut all = Vec::new();
        let mut next = extend;
        let mut hops = 0usize;
        while next != NO_PAGE {
            let ext = pages.typed(next, 5)?;
            let used = usize::from(u16::from_le_bytes(ext[13..15].try_into().unwrap()));
            all.extend_from_slice(ext.get(PAGE_HEADER..PAGE_HEADER + used).ok_or_else(|| bad("bad extend page"))?);
            next = u32::from_le_bytes(ext[9..13].try_into().unwrap());
            hops += 1;
            if hops > pages.count() {
                return Err(bad("extend page loop"));
            }
        }
        return Bson::parse_document(&all);
    }
    Err(bad("dangling data block"))
}

// ------------------------------------------------------------------ BSON

/// The BSON values LiteDB writes, as far as the story database needs them.
#[derive(Debug, Clone, PartialEq)]
enum Bson {
    Int(i64),
    Double(f64),
    Str(String),
    Bin(Vec<u8>),
    Doc(HashMap<String, Bson>),
    /// Arrays, booleans, null, ObjectId, Guid, DateTime, Decimal, min/max keys: not needed, skipped.
    Other,
}

impl Bson {
    fn get(&self, key: &str) -> Option<&Bson> {
        match self {
            Bson::Doc(map) => map.get(key),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Bson::Str(s) => Some(s),
            _ => None,
        }
    }

    fn as_i64(&self) -> Option<i64> {
        match self {
            Bson::Int(v) => Some(*v),
            Bson::Double(v) => Some(*v as i64),
            _ => None,
        }
    }

    fn parse_document(data: &[u8]) -> Result<Bson> {
        Self::document(&mut Reader::at(data, 0), false)
    }

    fn document(r: &mut Reader, array: bool) -> Result<Bson> {
        let length = usize::try_from(r.i32()?).map_err(|_| bad("negative document length"))?;
        let end = (r.pos + length).checked_sub(5).ok_or_else(|| bad("short document"))?;
        let mut map = HashMap::new();
        while r.pos < end {
            let ty = r.u8()?;
            let name = r.cstring()?;
            let value = match ty {
                0x01 => Bson::Double(r.f64()?),
                0x02 => {
                    let n = usize::try_from(r.i32()?).map_err(|_| bad("negative string length"))?;
                    let raw = r.bytes(n)?;
                    Bson::Str(String::from_utf8_lossy(raw.strip_suffix(&[0]).unwrap_or(raw)).into_owned())
                }
                0x03 => Self::document(r, false)?,
                0x04 => {
                    Self::document(r, true)?;
                    Bson::Other
                }
                0x05 => {
                    let n = usize::try_from(r.i32()?).map_err(|_| bad("negative binary length"))?;
                    let subtype = r.u8()?;
                    let raw = r.bytes(n)?;
                    if subtype == 0 {
                        Bson::Bin(raw.to_vec())
                    } else {
                        Bson::Other
                    }
                }
                0x07 => {
                    r.bytes(12)?;
                    Bson::Other
                }
                0x08 => {
                    r.u8()?;
                    Bson::Other
                }
                0x09 => {
                    r.i64()?;
                    Bson::Other
                }
                0x0A | 0x7F | 0xFF => Bson::Other,
                0x10 => Bson::Int(i64::from(r.i32()?)),
                0x12 => Bson::Int(r.i64()?),
                0x13 => {
                    r.bytes(16)?;
                    Bson::Other
                }
                other => return Err(bad(&format!("unsupported BSON type {other:#x}"))),
            };
            if !array {
                map.insert(name, value);
            }
        }
        r.u8()?; // terminator
        Ok(Bson::Doc(map))
    }
}

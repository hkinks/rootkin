//! Limits for reading untrusted files, so that a broken or hostile file gives a clear error
//! instead of exhausting memory or the stack. They are far above what real family trees need
//! (see "Limits" in docs/design.md).

use std::io::Read;

use crate::{Error, Result};

/// Deepest element nesting accepted in XML (the app's trees nest about 10 levels) and GEDCOM.
pub const MAX_DEPTH: usize = 256;

/// Largest single file unpacked from a zip, gzip or deflate stream (a photo, video or tree).
pub const MAX_ENTRY: u64 = 1 << 30;

/// Largest total size of everything unpacked from one zip file.
pub const MAX_TOTAL: u64 = 2 << 30;

/// Read at most `limit` bytes; more is an error naming `what`. No size hint from the file is
/// trusted for allocation.
pub fn read_limited(reader: impl Read, limit: u64, what: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    reader.take(limit.saturating_add(1)).read_to_end(&mut out)?;
    if out.len() as u64 > limit {
        return Err(too_big(what, limit));
    }
    Ok(out)
}

/// The error for a file part that unpacks to more than `limit` bytes.
pub fn too_big(what: &str, limit: u64) -> Error {
    Error::Format(too_big_message(what, limit))
}

/// The text of [`too_big`].
pub fn too_big_message(what: &str, limit: u64) -> String {
    format!("{what} unpacks to more than {} MB; the file is damaged or not a family tree", limit >> 20)
}

/// The error for XML or GEDCOM nested deeper than [`MAX_DEPTH`].
pub fn too_deep(what: &str) -> String {
    format!("{what} is nested more than {MAX_DEPTH} levels deep; the file is damaged or not a family tree")
}

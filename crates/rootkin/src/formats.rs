//! GEDCOM / Gramps import and export (wraps the `rootkin-formats` crate).

use std::path::Path;

use familyx::Package;

/// Import a genealogy file into a new tree; the `String` summarizes the result.
pub fn import(path: &Path) -> Result<(Package, String), String> {
    let _ = path;
    Err("GEDCOM and Gramps import is coming in the next version".into())
}

pub fn export(pkg: &Package, path: &Path) -> Result<(), String> {
    let _ = (pkg, path);
    Err("GEDCOM and Gramps export is coming in the next version".into())
}

//! GEDCOM / Gramps import and export (wraps the `rootkin-formats` crate).

use std::path::Path;

use familyx::Package;

/// Import a genealogy file into a new tree; the `String` summarizes the result.
pub fn import(path: &Path) -> Result<(Package, String), String> {
    let (pkg, report) = rootkin_formats::import_path(path).map_err(|e| e.to_string())?;
    let mut summary = format!("{} people and {} families", report.people, report.families);
    if !report.warnings.is_empty() {
        summary.push_str(&format!(" ({} notes: {})", report.warnings.len(), report.warnings.join("; ")));
    }
    Ok((pkg, summary))
}

pub fn export(pkg: &Package, path: &Path) -> Result<(), String> {
    rootkin_formats::export_path(pkg, path).map_err(|e| e.to_string())
}

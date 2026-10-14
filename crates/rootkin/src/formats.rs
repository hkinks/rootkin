//! GEDCOM / Gramps import and export (wraps the `rootkin-formats` crate).

use std::path::Path;

use familyx::Package;

/// Import a genealogy file into a new tree.
pub fn import(path: &Path) -> Result<(Package, ImportReport), String> {
    rootkin_formats::import_path(path).map_err(|e| e.to_string())
}

/// The file name offered by "Save as" after an import: the tree's own name
/// when the file gives one, otherwise the imported file's name.
pub fn suggested_save_name(report: &ImportReport, imported: &Path) -> String {
    let stem = imported.file_stem().map(|s| s.to_string_lossy().into_owned());
    let name = report.tree_name.clone().or(stem).unwrap_or_default();
    let clean: String =
        name.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let clean = clean.trim_matches('.');
    format!("{}.familyx", if clean.is_empty() { "family" } else { clean })
}

pub use rootkin_formats::Format;
pub use rootkin_formats::ImportReport;

/// Export with photos copied into `<name>_media/` next to the file.
pub fn export(pkg: &Package, path: &Path, format: Format) -> Result<(), String> {
    rootkin_formats::export_path_as(pkg, path, format).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(tree: Option<&str>, file: &str) -> String {
        let report = ImportReport { tree_name: tree.map(str::to_string), ..Default::default() };
        suggested_save_name(&report, Path::new(file))
    }

    #[test]
    fn save_name_comes_from_the_tree_name_or_the_file() {
        assert_eq!(name(Some("Kask Family Tree"), "/x/ancestry.ged"), "Kask Family Tree.familyx");
        assert_eq!(name(None, "/x/My Tree.ged"), "My Tree.familyx");
        assert_eq!(name(None, "/x/download.gdz"), "download.familyx");
        assert_eq!(name(Some("a/b: c?"), "x.ged"), "a b c.familyx");
        assert_eq!(name(Some("  "), ""), "family.familyx");
    }
}

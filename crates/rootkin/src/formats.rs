//! GEDCOM / Gramps import and export (wraps the `rootkin-formats` crate).

use std::path::Path;

use familyx::Package;

/// What an import brought in, and the notes about what it left out.
pub struct Summary {
    pub people: usize,
    pub families: usize,
    pub notes: Vec<String>,
}

impl Summary {
    /// "1 person and 2 families"
    pub fn headline(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        format!("{} and {}", plural(self.people, "person", "people"), plural(self.families, "family", "families"))
    }

    /// The whole report as plain text, for copying.
    pub fn details(&self, file: &str) -> String {
        let mut text = format!("Imported {file}: {}\n", self.headline());
        for note in &self.notes {
            text.push_str(&format!("- {note}\n"));
        }
        text
    }
}

/// Import a genealogy file into a new tree.
pub fn import(path: &Path) -> Result<(Package, Summary), String> {
    let (pkg, report) = rootkin_formats::import_path(path).map_err(|e| e.to_string())?;
    Ok((pkg, Summary { people: report.people, families: report.families, notes: report.warnings }))
}

pub use rootkin_formats::Format;

/// Export with photos copied into `<name>_media/` next to the file.
pub fn export(pkg: &Package, path: &Path, format: Format) -> Result<(), String> {
    rootkin_formats::export_path_as(pkg, path, format).map_err(|e| e.to_string())
}

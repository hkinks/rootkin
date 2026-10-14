//! Error dialogs in plain language: what the file is and what to do about it.

use std::path::{Path, PathBuf};

use crate::app::file_name;

/// A failure shown to the user. Raw error text stays in `details`.
#[derive(Clone, Debug)]
pub struct Problem {
    pub heading: String,
    pub message: String,
    pub path: PathBuf,
    /// Raw error text, only shown behind "Details".
    pub details: String,
    /// Offer an "Import" button for `path`.
    pub offer_import: bool,
}

fn extension(path: &Path) -> String {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Why `path` can't be opened as a family tree, and what to do instead.
pub fn cannot_open(path: &Path, raw: &str) -> Problem {
    let name = file_name(path);
    let (message, offer_import) = match extension(path).as_str() {
        "ged" | "gedcom" => ("This is a GEDCOM file. Use Import to bring it in.", true),
        "gramps" => ("This is a Gramps file. Use Import to bring it in.", true),
        "gdz" => ("This is a GEDCOM 7 archive. Use Import to bring it in.", true),
        "zip" => (
            "This is a zip file, not a family tree. If it holds a GEDCOM or Gramps file, \
             Import can read it.",
            true,
        ),
        "ftm" | "ftmb" => (
            "This is a Family Tree Maker file. Rootkin can't read it directly: export a GEDCOM \
             file from Family Tree Maker first, then use Import.",
            false,
        ),
        "rmtree" | "rmgc" => (
            "This is a RootsMagic file. Rootkin can't read it directly: export a GEDCOM file \
             from RootsMagic first, then use Import.",
            false,
        ),
        "paf" => (
            "This is a Personal Ancestral File (PAF). Rootkin can't read it directly: open it \
             in PAF or another genealogy program, export a GEDCOM file, then use Import.",
            false,
        ),
        "csv" | "xls" | "xlsx" | "txt" => (
            "This is a spreadsheet or text file, not a family tree. Rootkin opens .familyx files \
             from My Family Tree. To bring in a tree from another program, use Import (GEDCOM or Gramps).",
            false,
        ),
        "familyx" => (
            "This family tree file could not be read. It may be damaged, or it may not be a file \
             saved by My Family Tree. If you have a backup, it is next to the file as .bak.",
            false,
        ),
        _ => (
            "This isn't a family tree file. Rootkin opens .familyx files from My Family Tree. \
             To bring in a tree from another program, use Import (GEDCOM or Gramps).",
            false,
        ),
    };
    Problem {
        heading: format!("Rootkin can't open {name}"),
        message: message.into(),
        path: path.to_path_buf(),
        details: raw.into(),
        offer_import,
    }
}

/// Why `path` could not be imported.
pub fn cannot_import(path: &Path, raw: &str) -> Problem {
    let name = file_name(path);
    let message = match extension(path).as_str() {
        "ftm" | "ftmb" => "This is a Family Tree Maker file. Export a GEDCOM file from Family Tree Maker first, then import that.",
        "rmtree" | "rmgc" => "This is a RootsMagic file. Export a GEDCOM file from RootsMagic first, then import that.",
        "paf" => "This is a Personal Ancestral File (PAF). Export a GEDCOM file from PAF or another genealogy program first, then import that.",
        "csv" | "xls" | "xlsx" => "This is a spreadsheet, not a genealogy file. Rootkin imports GEDCOM (.ged, .gdz) and Gramps files.",
        "familyx" => "This is already a Rootkin family tree. Use Open instead of Import.",
        _ => "Rootkin could not read this file as GEDCOM or Gramps. It may be damaged or a different kind of file.",
    };
    Problem {
        heading: format!("Rootkin can't import {name}"),
        message: message.into(),
        path: path.to_path_buf(),
        details: raw.into(),
        offer_import: false,
    }
}

/// A failed save: the changes are still open.
pub fn cannot_save(path: &Path, raw: &str) -> Problem {
    Problem {
        heading: format!("Rootkin can't save {}", file_name(path)),
        message: "Your changes are still open here. Check that the folder exists, has free space and \
                  that you are allowed to write to it, or save under another name."
            .into(),
        path: path.to_path_buf(),
        details: raw.into(),
        offer_import: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_files_are_named_with_a_next_step() {
        let ged = cannot_open(Path::new("/tmp/x/tree.GED"), "zip: invalid Zip archive");
        assert_eq!(ged.message, "This is a GEDCOM file. Use Import to bring it in.");
        assert!(ged.offer_import);
        assert_eq!(ged.heading, "Rootkin can't open tree.GED");
        assert!(!ged.message.contains("zip"));

        let ftm = cannot_open(Path::new("a/b.ftm"), "x");
        assert!(ftm.message.contains("export a GEDCOM file from Family Tree Maker first"));
        assert!(!ftm.offer_import);
        assert!(cannot_open(Path::new("b.ftmb"), "x").message.contains("Family Tree Maker"));
        assert!(cannot_open(Path::new("b.rmtree"), "x").message.contains("RootsMagic"));
        assert!(cannot_open(Path::new("b.paf"), "x").message.contains("PAF"));
        assert!(cannot_open(Path::new("b.csv"), "x").message.contains("not a family tree"));
        assert!(cannot_open(Path::new("b.zip"), "x").offer_import);
        assert!(cannot_open(Path::new("b.bin"), "x").message.starts_with("This isn't a family tree file"));
        assert_eq!(ged.details, "zip: invalid Zip archive");
    }
}

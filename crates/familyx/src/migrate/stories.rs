//! Database-backed stories (`Story/FileId`, `Story/AnnotationsFileId`) of app versions 7.0 to 14.1.
//!
//! Those versions kept the text of the stories in a LiteDB `FileStorage` database in a part next to the
//! tree, `stories7` (the file name is a literal of the app's conversion code). The database is encrypted
//! with a password that is a constant of the app and depends on the file version. `FileId` is the `_id` of
//! the story's file record; the file is the story text (UTF-8, treated by the app as `text/richtext`);
//! `AnnotationsFileId` names a second file with the WPF annotations (sticky notes) of the story.
//! See docs/design.md, "Schema migration".

use crate::litedb::LiteDb;

/// Part name of the story database (case-insensitive).
pub(super) const PART: &str = "stories7";

/// Database passwords of the app: constants of its conversion code.
const PASSWORD_7_0_TO_7_6: &str = "5EuBD8Bfmx53Re9oyiEYvJO7AQF9dLLMbON2ZBMICzQeaRKAbUNE1JrUi22X518";
const PASSWORD_FROM_7_7: &str = "gkBrhnC6uSzoU2KMOL4UeNfNpeIqSSRkEn0PliJJ5NJP3IJViZE9o1eAQw6dbQI";

/// The story database of a package, opened on first use.
pub(super) struct StoryDb<'a> {
    parts: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    version: Option<f64>,
    db: Option<Result<LiteDb, String>>,
    /// Some story could not be resolved: the database must stay in the package.
    pub unresolved: bool,
    /// The database was opened and read.
    pub used: bool,
}

/// What the database says about one story.
pub(super) enum Lookup {
    Found(Vec<u8>),
    Missing,
    /// The database itself is unusable (absent, wrong password, corrupt).
    Unavailable(String),
}

impl<'a> StoryDb<'a> {
    pub fn new(parts: &'a dyn Fn(&str) -> Option<Vec<u8>>, version: Option<f64>) -> Self {
        Self { parts, version, db: None, unresolved: false, used: false }
    }

    fn open(&self) -> Result<LiteDb, String> {
        let bytes = (self.parts)(PART).ok_or_else(|| format!("the story database {PART} is not in the package"))?;
        // The app picks the password by file version; the stored hash identifies the right one anyway.
        let (first, second) = match self.version {
            Some(v) if v <= 7.6 => (PASSWORD_7_0_TO_7_6, PASSWORD_FROM_7_7),
            _ => (PASSWORD_FROM_7_7, PASSWORD_7_0_TO_7_6),
        };
        LiteDb::open(&bytes, &[first, second]).map_err(|e| format!("the story database {PART} cannot be read ({e})"))
    }

    pub fn read(&mut self, file_id: &str) -> Lookup {
        if self.db.is_none() {
            self.db = Some(self.open());
        }
        match self.db.as_ref() {
            Some(Ok(db)) => {
                self.used = true;
                match db.read(file_id) {
                    Some(bytes) => Lookup::Found(bytes),
                    None => Lookup::Missing,
                }
            }
            Some(Err(e)) => Lookup::Unavailable(e.clone()),
            None => unreachable!(),
        }
    }
}

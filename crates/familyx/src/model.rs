//! Typed, read-only snapshot of a family [`Document`], plus relationship index.
//!
//! The snapshot is rebuilt from the document after every edit; it is cheap
//! (a few hundred people) and keeps the document the single source of truth.

use std::collections::HashMap;

pub use crate::date::Date;
use crate::doc::{Document, Element};

pub type Id = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gender {
    Male,
    Female,
    Other,
    #[default]
    Unknown,
}

impl Gender {
    pub fn parse(s: &str) -> Self {
        match s {
            "Male" => Gender::Male,
            "Female" => Gender::Female,
            "Other" => Gender::Other,
            _ => Gender::Unknown,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Name {
    pub id: Id,
    pub given: String,
    pub surname_prefix: String,
    pub surname: String,
    pub suffix: String,
    pub nickname: String,
    pub is_primary: bool,
    pub name_type: String,
}

impl Name {
    pub fn display(&self) -> String {
        [self.given.as_str(), self.surname_prefix.as_str(), self.surname.as_str(), self.suffix.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fact {
    pub id: Id,
    /// GEDCOM-like tag: `BIRT`, `DEAT`, `MARR`, `OCCU`, ...
    pub tag: String,
    pub is_primary: bool,
    pub date: Date,
    pub place: String,
    pub details: String,
    /// User-defined event name for `EVEN` facts.
    pub custom_tag: String,
    pub notes: Vec<Note>,
    pub citations: Vec<Citation>,
}

impl Fact {
    pub fn label(&self) -> &str {
        if self.custom_tag.is_empty() {
            fact_label(&self.tag)
        } else {
            &self.custom_tag
        }
    }
}

/// A note as stored in `Notes/Notes/Note`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Note {
    pub id: Id,
    /// `text/plain`, `text/html` or `text/richtext` (RTF).
    pub format: String,
    pub text: String,
}

impl Note {
    pub fn is_rich(&self) -> bool {
        crate::richtext::is_rich(&self.format)
    }

    pub fn plain_text(&self) -> String {
        crate::richtext::to_plain(&self.format, &self.text)
    }
}

/// A citation of a [`Source`] on a person or fact (`Citation` in a
/// `*CitationCollection`). `source` may dangle; see `edit::check`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Citation {
    pub id: Id,
    pub source: Id,
    /// Page, entry or other location within the source.
    pub page: String,
    /// The app's `Quality` enum (`Weak`, `Satisfactory`, `Good`, `Excellent`); empty = blank.
    pub quality: String,
}

/// A source record (`SourceCollection/Source`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    pub id: Id,
    pub title: String,
    pub abbreviation: String,
    pub author: String,
    pub publisher: String,
    /// The first `Repositories/SourceRepositoryCitation`: repository id and call number.
    pub repository: Option<Id>,
    pub call_number: String,
    pub notes: Vec<Note>,
}

impl Source {
    /// Title, or abbreviation / `(untitled source)` when there is none.
    pub fn display_title(&self) -> String {
        [&self.title, &self.abbreviation]
            .into_iter()
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| "(untitled source)".into())
    }
}

/// A repository record (`RepositoryCollection/Repository`), e.g. an archive.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Repository {
    pub id: Id,
    pub title: String,
}

/// Face rectangle within the primary photo, in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Face {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn fact_label(tag: &str) -> &str {
    match tag {
        "BIRT" => "Birth",
        "DEAT" => "Death",
        "MARR" => "Marriage",
        // the app's relationship types for a couple
        "MARR_Civil" => "Civil marriage",
        "MARR_Religious" => "Religious marriage",
        "MARR_CommonLaw" => "Common-law marriage",
        "MARR_OtherMarriage" => "Other marriage",
        "MARR_Partnership" => "Partnership",
        "MARR_RegisteredPartnership" => "Registered partnership",
        "MARR_LivingTogether" => "Living together",
        "MARR_LivingTogetherApart" => "Living apart together",
        "MARR_NotMarried" => "Not married",
        "MARR_Other" => "Other relationship",
        "MARR_Unknown" => "Relationship (type unknown)",
        "DIV" => "Divorce",
        "OCCU" => "Occupation",
        "BURI" => "Burial",
        "CHR" => "Christening",
        "BAPM" => "Baptism",
        "RESI" => "Residence",
        "ADOP" => "Adoption",
        "EDUC" => "Education",
        "EVEN" => "Event",
        "FACT_ContactAddress" => "Contact address",
        "CREM" => "Cremation",
        "EMIG" => "Emigration",
        "IMMI" => "Immigration",
        "NATU" => "Naturalization",
        "RELI" => "Religion",
        "GRAD" => "Graduation",
        "RETI" => "Retirement",
        "CENS" => "Census",
        "ENGA" => "Engagement",
        "MARB" => "Marriage banns",
        "ANUL" => "Annulment",
        "TITL" => "Title",
        "DSCR" => "Physical description",
        other => other,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Person {
    pub id: Id,
    pub names: Vec<Name>,
    pub gender: Gender,
    pub is_living: bool,
    pub facts: Vec<Fact>,
    pub primary_image: Option<Id>,
    pub media: Vec<Id>,
    pub face: Option<Face>,
    pub notes: Vec<Note>,
    pub citations: Vec<Citation>,
}

impl Person {
    pub fn primary_name(&self) -> Option<&Name> {
        self.names.iter().find(|n| n.is_primary).or_else(|| self.names.first())
    }

    pub fn display_name(&self) -> String {
        self.primary_name().map(Name::display).filter(|s| !s.is_empty()).unwrap_or_else(|| "(unnamed)".into())
    }

    pub fn fact(&self, tag: &str) -> Option<&Fact> {
        let mut matching = self.facts.iter().filter(|f| f.tag == tag);
        let first = matching.clone().next();
        matching.find(|f| f.is_primary).or(first)
    }

    /// e.g. `1901–1980`, `b. 1990`.
    pub fn lifespan(&self) -> String {
        let year = |tag| self.fact(tag).and_then(|f| f.date.year);
        match (year("BIRT"), year("DEAT")) {
            (Some(b), Some(d)) => format!("{b}–{d}"),
            (Some(b), None) => format!("b. {b}"),
            (None, Some(d)) => format!("d. {d}"),
            (None, None) => String::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpouseLink {
    pub id: Id,
    pub person1: Id,
    pub person2: Id,
    pub facts: Vec<Fact>,
    pub status: String,
}

impl SpouseLink {
    pub fn other(&self, id: &str) -> &str {
        if self.person1 == id {
            &self.person2
        } else {
            &self.person1
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParentChildLink {
    pub id: Id,
    pub parent: Id,
    pub child: Id,
    pub modifier: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Media {
    pub id: Id,
    /// Zip part name, e.g. `Multimedia/abc.png`.
    pub part: String,
    pub file_name: String,
    pub title: String,
}

/// Everything the UI needs, derived from the document.
#[derive(Debug, Default)]
pub struct Tree {
    pub people: Vec<Person>,
    pub spouse_links: Vec<SpouseLink>,
    pub parent_links: Vec<ParentChildLink>,
    pub media: HashMap<Id, Media>,
    pub sources: Vec<Source>,
    pub repositories: Vec<Repository>,
    pub home_person: Option<Id>,
    by_id: HashMap<Id, usize>,
}

impl Tree {
    pub fn from_doc(doc: &Document) -> Self {
        let root = &doc.root;
        let people: Vec<Person> = root
            .child("PeopleCollection")
            .map(|c| c.children_named("Person").map(read_person).collect())
            .unwrap_or_default();

        let mut spouse_links = Vec::new();
        let mut parent_links = Vec::new();
        for link in root.child("RelationshipCollection").into_iter().flat_map(|c| c.children_named("RelationshipLink"))
        {
            let id = attr(link, "Id");
            let p1 = attr(link, "Person1Id");
            let p2 = attr(link, "Person2Id");
            match link.attr("xsi:type").as_deref() {
                Some("SpouseLink") => spouse_links.push(SpouseLink {
                    id,
                    person1: p1,
                    person2: p2,
                    facts: read_facts(link),
                    status: link.child_text("SpouseRelationshipStatus").unwrap_or_default(),
                }),
                Some("ParentChildLink") => parent_links.push(ParentChildLink {
                    id,
                    parent: p1,
                    child: p2,
                    modifier: link.child_text("ParentChildModifier").unwrap_or_default(),
                }),
                _ => {}
            }
        }

        let media = root
            .child("PackagedFileCollection")
            .into_iter()
            .flat_map(|c| c.children_named("PackagedFile"))
            .map(|f| {
                let m = Media {
                    id: attr(f, "Id"),
                    part: f.child_text("RelativePath").unwrap_or_default().replace('\\', "/"),
                    file_name: f.child_text("FileName").unwrap_or_default(),
                    title: f.child_text("Properties/Title").unwrap_or_default(),
                };
                (m.id.clone(), m)
            })
            .collect();

        let sources = root
            .child("SourceCollection")
            .into_iter()
            .flat_map(|c| c.children_named("Source"))
            .map(read_source)
            .collect();
        let repositories = root
            .child("RepositoryCollection")
            .into_iter()
            .flat_map(|c| c.children_named("Repository"))
            .map(|r| Repository { id: attr(r, "Id"), title: r.child_text("Title").unwrap_or_default() })
            .collect();

        let home_person = root.child_text("FamilyProperties/HomePersonId").filter(|s| !s.is_empty());
        let by_id = people.iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect();
        Tree { people, spouse_links, parent_links, media, sources, repositories, home_person, by_id }
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.by_id.get(id).map(|&i| &self.people[i])
    }

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.sources.iter().find(|s| s.id == id)
    }

    /// How many citations (on people, events and couples) refer to this source.
    pub fn citation_count(&self, source: &str) -> usize {
        let count = |c: &[Citation]| c.iter().filter(|c| c.source == source).count();
        let couple_facts = self.spouse_links.iter().flat_map(|l| &l.facts);
        self.people
            .iter()
            .map(|p| count(&p.citations) + p.facts.iter().map(|f| count(&f.citations)).sum::<usize>())
            .sum::<usize>()
            + couple_facts.map(|f| count(&f.citations)).sum::<usize>()
    }

    pub fn repository(&self, id: &str) -> Option<&Repository> {
        self.repositories.iter().find(|r| r.id == id)
    }

    pub fn parents(&self, id: &str) -> Vec<&str> {
        self.parent_links.iter().filter(|l| l.child == id).map(|l| l.parent.as_str()).collect()
    }

    pub fn children(&self, id: &str) -> Vec<&str> {
        // file order, which the user can rearrange (see edit::move_child)
        self.parent_links.iter().filter(|l| l.parent == id).map(|l| l.child.as_str()).collect()
    }

    pub fn spouse_links_of(&self, id: &str) -> Vec<&SpouseLink> {
        self.spouse_links.iter().filter(|l| l.person1 == id || l.person2 == id).collect()
    }

    pub fn spouses(&self, id: &str) -> Vec<&str> {
        self.spouse_links_of(id).into_iter().map(|l| l.other(id)).collect()
    }

    pub fn primary_photo(&self, person: &Person) -> Option<&Media> {
        person.primary_image.as_ref().and_then(|m| self.media.get(m))
    }
}

fn attr(el: &Element, name: &str) -> String {
    el.attr(name).map(|v| v.into_owned()).unwrap_or_default()
}

fn text_opt<T: std::str::FromStr>(el: &Element, path: &str) -> Option<T> {
    el.child_text(path).and_then(|t| t.trim().parse().ok())
}

fn read_notes(owner: &Element) -> Vec<Note> {
    owner
        .path("Notes/Notes")
        .into_iter()
        .flat_map(|n| n.children_named("Note"))
        .map(|n| Note {
            id: attr(n, "Id"),
            format: n.child_text("Format").unwrap_or_else(|| "text/plain".into()),
            text: n.child_text("Text").unwrap_or_default(),
        })
        .collect()
}

fn read_date(d: Option<&Element>) -> Date {
    let Some(d) = d else { return Date::default() };
    let descriptor = d.child_text("DateDescriptor").unwrap_or_default();
    Date {
        // `Exact` is the serialized default; normalize to empty
        descriptor: if descriptor == "Exact" { String::new() } else { descriptor },
        year: text_opt(d, "StartYear"),
        // StartMonth is a non-nullable int in the app; 0 means unknown
        month: text_opt(d, "StartMonth").filter(|m| *m != 0),
        day: text_opt(d, "StartDay"),
        end_year: text_opt(d, "EndYear"),
        end_month: text_opt(d, "EndMonth").filter(|m| *m != 0),
        end_day: text_opt(d, "EndDay"),
        text: d.child_text("Text").unwrap_or_default(),
    }
}

fn read_person(p: &Element) -> Person {
    let names = p
        .path("PersonNameCollection/Names")
        .into_iter()
        .flat_map(|n| n.children_named("Name"))
        // the app keeps an empty trailing <Name/> placeholder (no child elements); skip it.
        // A name added for editing has its (still empty) fields and is kept.
        .filter(|n| n.elements().next().is_some())
        .map(|n| Name {
            id: attr(n, "Id"),
            given: n.child_text("Names").unwrap_or_default(),
            surname_prefix: n.child_text("SurnamePrefix").unwrap_or_default(),
            surname: n.child_text("Surname").unwrap_or_default(),
            suffix: n.child_text("Suffix").unwrap_or_default(),
            nickname: n.child_text("Nickname").unwrap_or_default(),
            is_primary: n.child_text("IsPrimary").as_deref() == Some("true"),
            name_type: n.child_text("NameType").unwrap_or_default(),
        })
        .collect();
    Person {
        id: attr(p, "Id"),
        names,
        gender: Gender::parse(&p.child_text("Biology/Gender").unwrap_or_default()),
        is_living: p.child_text("IsLiving").as_deref() == Some("true"),
        facts: read_facts(p),
        primary_image: p.child_text("PrimaryImageId").filter(|s| !s.is_empty()),
        face: p.child("Face").and_then(|f| {
            let face = Face {
                x: text_opt(f, "X")?,
                y: text_opt(f, "Y")?,
                width: text_opt(f, "Width")?,
                height: text_opt(f, "Height")?,
            };
            (face.width > 0.0 && face.height > 0.0).then_some(face)
        }),
        notes: read_notes(p),
        citations: read_citations(p, "PersonCitationCollection"),
        media: p
            .child("Multimedia")
            .map(|m| m.children_named("string").map(Element::text).collect())
            .unwrap_or_default(),
    }
}

fn read_facts(owner: &Element) -> Vec<Fact> {
    owner
        .child("Facts")
        .into_iter()
        .flat_map(|f| f.children_named("Fact"))
        .map(|f| Fact {
            id: attr(f, "Id"),
            tag: f.child_text("Tag").unwrap_or_default(),
            is_primary: f.child_text("IsPrimary").as_deref() == Some("true"),
            date: read_date(f.child("FactDate")),
            place: f.child_text("PlaceDetails/Description").unwrap_or_default(),
            details: f.child_text("Details").unwrap_or_default(),
            custom_tag: f.child_text("CustomFactTag").unwrap_or_default(),
            notes: read_notes(f),
            citations: read_citations(f, "FactCitationCollection"),
        })
        .collect()
}

fn read_citations(owner: &Element, collection: &str) -> Vec<Citation> {
    owner
        .path(&format!("{collection}/Citations"))
        .into_iter()
        .flat_map(|c| c.children_named("Citation"))
        .map(|c| Citation {
            id: attr(c, "Id"),
            source: c.child_text("Source").unwrap_or_default(),
            page: c.child_text("Page").unwrap_or_default(),
            quality: c.child_text("Quality").filter(|q| q != "Blank").unwrap_or_default(),
        })
        .collect()
}

fn read_source(s: &Element) -> Source {
    let first = s.path("Repositories").and_then(|r| r.child("SourceRepositoryCitation"));
    Source {
        id: attr(s, "Id"),
        title: s.child_text("Title").unwrap_or_default(),
        abbreviation: s.child_text("Abbreviation").unwrap_or_default(),
        author: s.child_text("Author").unwrap_or_default(),
        publisher: s.child_text("Publisher").unwrap_or_default(),
        repository: first.and_then(|r| r.child_text("Repository")).filter(|r| !r.is_empty()),
        call_number: first.and_then(|r| r.child_text("CallNumber")).unwrap_or_default(),
        notes: read_notes(s),
    }
}

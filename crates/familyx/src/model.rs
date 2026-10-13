//! Typed, read-only snapshot of a family [`Document`], plus relationship index.
//!
//! The snapshot is rebuilt from the document after every edit; it is cheap
//! (a few hundred people) and keeps the document the single source of truth.

use std::collections::HashMap;

use crate::doc::{Document, Element};

pub type Id = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Gender {
    Male,
    Female,
    #[default]
    Unknown,
}

impl Gender {
    pub fn parse(s: &str) -> Self {
        match s {
            "Male" => Gender::Male,
            "Female" => Gender::Female,
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
pub struct Date {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    /// `FactDatePeriod/DateDescriptor`, e.g. `Span` (exact) or `About`.
    pub descriptor: String,
}

impl Date {
    pub fn is_empty(&self) -> bool {
        self.year.is_none() && self.month.is_none() && self.day.is_none()
    }

    pub fn display(&self) -> String {
        let core = match (self.year, self.month, self.day) {
            (Some(y), Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
            (Some(y), Some(m), None) => format!("{y:04}-{m:02}"),
            (Some(y), None, _) => format!("{y}"),
            (None, Some(m), Some(d)) => format!("??-{m:02}-{d:02}"),
            _ => String::new(),
        };
        if self.descriptor == "About" && !core.is_empty() {
            format!("abt. {core}")
        } else {
            core
        }
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
}

impl Fact {
    pub fn label(&self) -> &str {
        fact_label(&self.tag)
    }
}

pub fn fact_label(tag: &str) -> &str {
    match tag {
        "BIRT" => "Birth",
        "DEAT" => "Death",
        "MARR" => "Marriage",
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
        for link in root.child("RelationshipCollection").into_iter().flat_map(|c| c.children_named("RelationshipLink")) {
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

        let home_person = root.child_text("FamilyProperties/HomePersonId").filter(|s| !s.is_empty());
        let by_id = people.iter().enumerate().map(|(i, p)| (p.id.clone(), i)).collect();
        Tree { people, spouse_links, parent_links, media, home_person, by_id }
    }

    pub fn person(&self, id: &str) -> Option<&Person> {
        self.by_id.get(id).map(|&i| &self.people[i])
    }

    pub fn parents(&self, id: &str) -> Vec<&str> {
        self.parent_links.iter().filter(|l| l.child == id).map(|l| l.parent.as_str()).collect()
    }

    pub fn children(&self, id: &str) -> Vec<&str> {
        let mut kids: Vec<&str> =
            self.parent_links.iter().filter(|l| l.parent == id).map(|l| l.child.as_str()).collect();
        kids.sort_by_key(|k| self.person(k).and_then(|p| p.fact("BIRT")).and_then(|f| f.date.year));
        kids
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

fn read_person(p: &Element) -> Person {
    let names = p
        .path("PersonNameCollection/Names")
        .into_iter()
        .flat_map(|n| n.children_named("Name"))
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
        // the app keeps an empty trailing <Name/> placeholder; skip it
        .filter(|n| !(n.given.is_empty() && n.surname.is_empty() && n.nickname.is_empty()))
        .collect();
    Person {
        id: attr(p, "Id"),
        names,
        gender: Gender::parse(&p.child_text("Biology/Gender").unwrap_or_default()),
        is_living: p.child_text("IsLiving").as_deref() == Some("true"),
        facts: read_facts(p),
        primary_image: p.child_text("PrimaryImageId").filter(|s| !s.is_empty()),
        media: p.child("Multimedia").map(|m| m.children_named("string").map(Element::text).collect()).unwrap_or_default(),
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
            date: Date {
                year: text_opt(f, "FactDate/StartYear"),
                month: text_opt(f, "FactDate/StartMonth"),
                day: text_opt(f, "FactDate/StartDay"),
                descriptor: f.child_text("FactDatePeriod/DateDescriptor").unwrap_or_default(),
            },
            place: f.child_text("PlaceDetails/Description").unwrap_or_default(),
            details: f.child_text("Details").unwrap_or_default(),
        })
        .collect()
}

//! Generates a fictional family tree used as the test fixture.

use std::io::Cursor;

use anyhow::Result;
use familyx::doc::Document;
use familyx::edit::{self, FactInput, SourceInput};
use familyx::model::{Date, Gender, Id};
use familyx::Package;

struct Builder<'a> {
    doc: &'a mut Document,
}

impl Builder<'_> {
    fn person(&mut self, given: &str, surname: &str, gender: Gender, facts: &[(&str, Date, &str, &str)]) -> Result<Id> {
        let id = edit::add_person(self.doc, given, surname, gender);
        for (tag, date, place, details) in facts {
            let input = FactInput { date: date.clone(), place: place.to_string(), details: details.to_string() };
            edit::add_fact(self.doc, &id, tag, &input)?;
        }
        Ok(id)
    }

    fn couple(&mut self, a: &Id, b: &Id, married: Option<(Date, &str)>) -> Result<Id> {
        let link = edit::link_spouse(self.doc, a, b)?;
        if let Some((date, place)) = married {
            let input = FactInput { date, place: place.into(), details: String::new() };
            edit::add_couple_fact(self.doc, &link, "MARR", &input)?;
        }
        Ok(link)
    }

    fn child(&mut self, child: &Id, parents: &[&Id]) -> Result<()> {
        for p in parents {
            edit::link_parent(self.doc, child, p)?;
        }
        Ok(())
    }
}

fn ymd(year: i32, month: u32, day: u32) -> Date {
    Date::ymd(year, month, day)
}

fn year(year: i32) -> Date {
    Date::year(year)
}

fn about(year: i32) -> Date {
    Date::year(year).with_descriptor("About")
}

/// A small placeholder portrait: a two-colour gradient.
fn portrait(rgb: [u8; 3]) -> Result<Vec<u8>> {
    let img = image::RgbImage::from_fn(96, 120, |x, y| {
        let shade = |c: u8| (c as u32 * (60 + y) / 180 + x / 8) as u8;
        image::Rgb([shade(rgb[0]), shade(rgb[1]), shade(rgb[2])])
    });
    let mut out = Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

/// Four generations of an invented family, exercising the format's features:
/// couples with and without marriage facts, a single parent, an adoption,
/// a married name, approximate dates, living people, photos and places with
/// characters that need XML escaping.
pub fn build() -> Result<Package> {
    use Gender::*;
    let mut pkg = Package::new();
    let mut b = Builder { doc: &mut pkg.doc };

    // generation 1
    let arthur = b.person(
        "Arthur",
        "Lindqvist",
        Male,
        &[
            ("BIRT", ymd(1890, 3, 14), "Uppsala, Sweden", ""),
            ("DEAT", ymd(1961, 11, 2), "Tallinn, Estonia", ""),
            ("OCCU", year(1920).with_descriptor("After"), "", "Clockmaker"),
        ],
    )?;
    let elsa = b.person(
        "Elsa",
        "Berg",
        Female,
        &[("BIRT", ymd(1893, 7, 1), "Stockholm, Sweden", ""), ("DEAT", year(1975), "Tallinn, Estonia", "")],
    )?;
    let johan = b.person(
        "Johan",
        "Mets",
        Male,
        &[
            ("BIRT", about(1888), "Tartu, Estonia", ""),
            ("DEAT", ymd(1944, 9, 20), "", ""),
            ("OCCU", Date::default(), "", "Farmer & beekeeper"),
        ],
    )?;
    let marta = b.person(
        "Marta",
        "Kask",
        Female,
        &[
            ("BIRT", ymd(1895, 1, 30), "Võru, Estonia", ""),
            ("DEAT", ymd(1980, 5, 5), "Tartu, Estonia", ""),
            ("BURI", ymd(1980, 5, 12), "Raadi cemetery, Tartu", ""),
        ],
    )?;
    b.couple(&arthur, &elsa, Some((ymd(1915, 6, 21), "Uppsala Cathedral")))?;
    b.couple(&johan, &marta, Some((year(1919), "Tartu")))?;

    // generation 2
    let karl = b.person(
        "Karl",
        "Lindqvist",
        Male,
        &[
            ("BIRT", ymd(1920, 2, 2), "Tallinn, Estonia", ""),
            ("DEAT", ymd(1999, 12, 24), "Tallinn, Estonia", ""),
            ("OCCU", Date::default(), "", "Engineer at the \"Volta\" factory"),
        ],
    )?;
    let ingrid = b.person("Ingrid", "Lindqvist", Female, &[("BIRT", about(1923), "Tallinn, Estonia", "")])?;
    let peeter =
        b.person("Peeter", "Mets", Male, &[("BIRT", year(1921), "Tartu, Estonia", ""), ("DEAT", year(1941), "", "")])?;
    let liis = b.person(
        "Liis",
        "Mets",
        Female,
        &[("BIRT", ymd(1924, 8, 9), "Tartu, Estonia", ""), ("DEAT", ymd(2015, 3, 3), "Tallinn, Estonia", "")],
    )?;
    edit::add_alternate_name(b.doc, &liis, "Liis", "Lindqvist", "Married")?;
    b.child(&karl, &[&arthur, &elsa])?;
    b.child(&ingrid, &[&arthur, &elsa])?;
    b.child(&peeter, &[&johan, &marta])?;
    b.child(&liis, &[&johan, &marta])?;
    b.couple(&karl, &liis, Some((ymd(1948, 9, 18), "Tallinn <Old Town>")))?;

    // generation 3
    let erik = b.person(
        "Erik",
        "Lindqvist",
        Male,
        &[("BIRT", ymd(1950, 4, 1), "Tallinn, Estonia", ""), ("RESI", year(1985), "Pärnu, Estonia", "")],
    )?;
    let anna = b.person("Anna", "Lindqvist", Female, &[("BIRT", ymd(1953, 10, 10), "Tallinn, Estonia", "")])?;
    let olaf = b.person("Olaf", "Berg-Lindqvist", Male, &[("BIRT", year(1950), "Helsinki, Finland", "")])?;
    let sofia = b.person("Sofia", "Rahe", Female, &[("BIRT", ymd(1955, 12, 31), "Pärnu, Estonia", "")])?;
    let tom = b.person("Tom", "O'Brien", Male, &[("BIRT", year(1951), "Cork, Ireland", "")])?;
    b.child(&erik, &[&karl, &liis])?;
    b.child(&anna, &[&karl, &liis])?;
    b.child(&olaf, &[&ingrid])?; // single parent
    b.couple(&erik, &sofia, Some((ymd(1977, 7, 7), "Smith & Sons farm, Pärnu")))?;
    b.couple(&anna, &tom, None)?;

    // generation 4
    let leo = b.person("Leo", "Lindqvist", Male, &[("BIRT", ymd(1979, 5, 20), "Pärnu, Estonia", "")])?;
    let nora = b.person(
        "Nora",
        "Lindqvist",
        Female,
        &[("BIRT", year(1983), "Riga, Latvia", ""), ("ADOP", year(1985), "Pärnu, Estonia", "")],
    )?;
    let mia = b.person("Mia", "O'Brien", Female, &[("BIRT", ymd(1980, 1, 15), "Dublin, Ireland", "")])?;
    b.child(&leo, &[&erik, &sofia])?;
    b.child(&nora, &[&erik, &sofia])?;
    edit::set_parent_modifier(b.doc, &nora, &erik, "Adopted")?;
    edit::set_parent_modifier(b.doc, &nora, &sofia, "Adopted")?;
    b.child(&mia, &[&anna, &tom])?;

    // generation 5: a child with one recorded parent
    let ada = b.person("Ada", "Lindqvist", Female, &[("BIRT", ymd(2010, 6, 6), "Tallinn, Estonia", "")])?;
    b.child(&ada, &[&leo])?;

    for id in [&erik, &anna, &olaf, &sofia, &tom, &leo, &nora, &mia, &ada] {
        edit::set_living(b.doc, id, true)?;
    }
    edit::set_home_person(b.doc, &erik)?;

    // sources: one archive, three sources, citations on a fact, a couple fact and a person
    let archive = edit::add_repository(b.doc, "Tallinn City Archives")?;
    let register = edit::add_source(
        b.doc,
        &SourceInput {
            title: "Uppsala cathedral parish register 1885-1895".into(),
            author: "Parish clerks of Uppsala".into(),
            publisher: "Uppsala Cathedral parish (manuscript)".into(),
            repository: Some(archive),
            call_number: "EAA.1234.1.56".into(),
            ..Default::default()
        },
    )?;
    let bible = edit::add_source(
        b.doc,
        &SourceInput {
            title: "Lindqvist family Bible".into(),
            note: Some("Births and deaths entered by hand on the flyleaf.".into()),
            ..Default::default()
        },
    )?;
    let census = edit::add_source(
        b.doc,
        &SourceInput {
            title: "Estonian census 1934".into(),
            abbreviation: "Census 1934".into(),
            publisher: "Central Bureau of Statistics".into(),
            ..Default::default()
        },
    )?;
    let fact_of = |doc: &Document, person: &Id, tag: &str| -> Result<Id> {
        let tree = familyx::model::Tree::from_doc(doc);
        let fact = tree.person(person).and_then(|p| p.fact(tag)).ok_or_else(|| anyhow::anyhow!("no {tag} fact"))?;
        Ok(fact.id.clone())
    };
    let birth = fact_of(b.doc, &arthur, "BIRT")?;
    edit::add_citation(b.doc, &birth, &register, "p. 214, no. 31")?;
    let wedding = familyx::model::Tree::from_doc(b.doc).spouse_links_of(&arthur)[0].facts[0].id.clone();
    edit::add_citation(b.doc, &wedding, &register, "p. 88")?;
    edit::add_citation(b.doc, &arthur, &bible, "")?;
    let birth = fact_of(b.doc, &karl, "BIRT")?;
    edit::add_citation(b.doc, &birth, &census, "household 17")?;
    edit::add_citation(b.doc, &birth, &bible, "flyleaf")?;

    pkg.add_photo(&erik, portrait([70, 110, 170])?, "png", "erik.png")?;
    pkg.add_photo(&liis, portrait([170, 80, 110])?, "png", "liis wedding.png")?;
    pkg.add_photo(&arthur, portrait([120, 120, 120])?, "png", "arthur.png")?;
    Ok(pkg)
}

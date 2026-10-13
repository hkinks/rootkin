//! A hand-written, fictional uncompressed Gramps XML file.

use familyx::model::{Gender, Tree};
use rootkin_formats::{import, Format};

const XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE database PUBLIC "-//Gramps//DTD Gramps XML 1.7.1//EN" "http://gramps-project.org/xml/1.7.1/grampsxml.dtd">
<database xmlns="http://gramps-project.org/xml/1.7.1/">
  <header><created date="2024-01-01" version="5.1.0"/></header>
  <events>
    <event handle="_e1" id="E0001"><type>Birth</type><dateval val="1901-05-00" type="about"/><place hlink="_p2"/></event>
    <event handle="_e2" id="E0002"><type>Residence</type><datespan start="1920-00-00" stop="1930-00-00"/><description>Farm &amp; mill</description></event>
    <event handle="_e3" id="E0003"><type>Marriage</type><dateval val="1925-06-07"/></event>
    <event handle="_e4" id="E0004"><type>Pilgrimage</type><datestr val="around the war"/></event>
  </events>
  <people>
    <person handle="_a" id="I1"><gender>M</gender>
      <name type="Birth Name"><first>Peeter</first><surname prefix="von">Sepp</surname><nick>Pete</nick></name>
      <name alt="1" type="Married Name"><first>Peeter</first><surname>Kuusk</surname></name>
      <eventref hlink="_e1" role="Primary"/><eventref hlink="_e2" role="Primary"/><eventref hlink="_e4" role="Primary"/>
      <parentin hlink="_f1"/></person>
    <person handle="_b" id="I2"><gender>F</gender><name><first>Anu</first><surname>Mägi</surname></name><parentin hlink="_f1"/></person>
    <person handle="_c" id="I3"><gender>U</gender><name><first>Kai</first></name><childof hlink="_f1"/></person>
    <person handle="_d" id="I4"><gender>F</gender><name><first>Tiiu</first></name></person>
  </people>
  <families>
    <family handle="_f1" id="F1"><rel type="Married"/><father hlink="_a"/><mother hlink="_b"/>
      <eventref hlink="_e3" role="Family"/><childref hlink="_c" frel="Adopted" mrel="Birth"/></family>
    <family handle="_f2" id="F2"><mother hlink="_d"/><childref hlink="_c"/></family>
  </families>
  <places>
    <placeobj handle="_p1" id="P1" type="Country"><pname value="Estonia"/></placeobj>
    <placeobj handle="_p2" id="P2" type="City"><pname value="Tartu"/><placeref hlink="_p1"/></placeobj>
  </places>
  <notes><note handle="_n1" id="N1" type="General"><text>hello</text></note></notes>
</database>"#;

#[test]
fn imports_handwritten_gramps_xml() {
    let (pkg, report) = import(XML.as_bytes(), Format::Gramps).unwrap();
    let tree = Tree::from_doc(&pkg.doc);
    assert_eq!((report.people, report.families), (4, 2));
    let peeter = &tree.people[0];
    assert_eq!(peeter.display_name(), "Peeter von Sepp");
    assert_eq!(peeter.gender, Gender::Male);
    assert_eq!(peeter.names[1].name_type, "Married");
    let birth = peeter.fact("BIRT").unwrap();
    assert_eq!((birth.date.descriptor.as_str(), birth.date.year, birth.date.month), ("About", Some(1901), Some(5)));
    assert_eq!(birth.place, "Tartu, Estonia");
    let resi = peeter.fact("RESI").unwrap();
    assert_eq!(
        (resi.date.descriptor.as_str(), resi.date.end_year, resi.details.as_str()),
        ("Span", Some(1930), "Farm & mill")
    );
    let custom = peeter.fact("EVEN").unwrap();
    assert_eq!((custom.date.descriptor.as_str(), custom.date.text.as_str()), ("TextOnly", "around the war"));
    assert_eq!(custom.details, "Pilgrimage");

    let kai = &tree.people[2];
    assert_eq!(tree.parents(&kai.id).len(), 3);
    let by_parent = |name: &str| {
        let parent = tree.people.iter().find(|p| p.display_name() == name).unwrap();
        tree.parent_links.iter().find(|l| l.child == kai.id && l.parent == parent.id).unwrap().modifier.clone()
    };
    assert_eq!(by_parent("Peeter von Sepp"), "Adopted");
    assert_eq!(by_parent("Anu Mägi"), "");
    assert_eq!(tree.spouse_links.len(), 1);
    assert_eq!(tree.spouse_links[0].facts[0].tag, "MARR");

    assert!(report.warnings.iter().any(|w| w.contains("person.name.nick")), "{:?}", report.warnings);
    assert!(report.warnings.iter().any(|w| w.contains("notes")));
}

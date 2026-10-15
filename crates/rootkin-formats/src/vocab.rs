//! Vocabulary shared by the formats.

/// The app's `NameType` values.
const NAME_TYPES: [&str; 10] =
    ["Adopted", "Aka", "Birth", "FormerSurname", "Immigrant", "Maiden", "Married", "Nick", "Professional", "Religious"];

/// Map a foreign name type (`married`, `Birth Name`, `MAIDEN`, ...) onto the
/// app's; anything unknown becomes `Aka`.
pub fn name_type(raw: &str) -> &'static str {
    let t = raw.trim();
    let t = t.strip_suffix(" Name").or_else(|| t.strip_suffix(" name")).unwrap_or(t);
    let t: String = t.chars().filter(|c| c.is_alphanumeric()).collect();
    NAME_TYPES.iter().find(|n| n.eq_ignore_ascii_case(&t)).copied().unwrap_or("Aka")
}

/// Neither format has a "living" flag: guess from missing death data and a
/// plausible age.
pub fn guess_living(dead: bool, birth_year: Option<i32>) -> bool {
    use chrono::Datelike;
    !dead && birth_year.is_some_and(|y| y > chrono::Utc::now().year() - 110)
}

/// The app's relationship types of a couple: the tag of the couple's marriage event.
const RELATIONSHIP_TAGS: [&str; 11] = [
    "MARR_Civil",
    "MARR_Religious",
    "MARR_CommonLaw",
    "MARR_OtherMarriage",
    "MARR_Partnership",
    "MARR_RegisteredPartnership",
    "MARR_LivingTogether",
    "MARR_LivingTogetherApart",
    "MARR_NotMarried",
    "MARR_Other",
    "MARR_Unknown",
];

/// The relationship tag whose readable name (`Civil marriage`) is `label`.
pub fn relationship_tag(label: &str) -> Option<&'static str> {
    let label = label.trim();
    RELATIONSHIP_TAGS.iter().find(|t| familyx::model::fact_label(t).eq_ignore_ascii_case(label)).copied()
}

/// Relationship types that are marriages (written as `MARR` in GEDCOM).
pub fn is_marriage_like(tag: &str) -> bool {
    matches!(tag, "MARR" | "MARR_Civil" | "MARR_Religious" | "MARR_CommonLaw" | "MARR_OtherMarriage")
}

/// Gramps' family relationship type for a couple's marriage-event tag.
pub fn gramps_relationship(tag: &str) -> &'static str {
    match tag {
        "MARR" | "MARR_Religious" | "MARR_CommonLaw" | "MARR_OtherMarriage" => "Married",
        "MARR_Civil" | "MARR_Partnership" | "MARR_RegisteredPartnership" => "Civil Union",
        "MARR_LivingTogether" | "MARR_LivingTogetherApart" | "MARR_NotMarried" => "Unmarried",
        _ => "Unknown",
    }
}

/// The marriage-event tag that stands for a Gramps family relationship type.
pub fn relationship_from_gramps(rel: &str) -> Option<&'static str> {
    match rel.trim().to_ascii_lowercase().as_str() {
        "married" => Some("MARR"),
        "civil union" => Some("MARR_RegisteredPartnership"),
        "unmarried" => Some("MARR_NotMarried"),
        _ => None,
    }
}

/// Individual events (GEDCOM 5.5.1).
pub const INDI_EVENTS: &[&str] = &[
    "BIRT", "CHR", "DEAT", "BURI", "CREM", "ADOP", "BAPM", "BARM", "BASM", "BLES", "CHRA", "CONF", "FCOM", "ORDN",
    "NATU", "EMIG", "IMMI", "CENS", "PROB", "WILL", "GRAD", "RETI", "EVEN",
];

/// Individual attributes: the value on the line is the fact's details.
pub const INDI_ATTRIBUTES: &[&str] =
    &["CAST", "DSCR", "EDUC", "IDNO", "NATI", "NCHI", "NMR", "OCCU", "PROP", "RELI", "RESI", "SSN", "TITL", "FACT"];

/// Family events.
pub const FAM_EVENTS: &[&str] =
    &["ANUL", "CENS", "DIV", "DIVF", "ENGA", "MARB", "MARC", "MARR", "MARL", "MARS", "RESI", "EVEN"];

/// Facts with a Gramps event type of their own: `(fact tag, Gramps type)`.
pub const GRAMPS_TYPES: &[(&str, &str)] = &[
    ("BIRT", "Birth"),
    ("DEAT", "Death"),
    ("MARR", "Marriage"),
    ("DIV", "Divorce"),
    ("OCCU", "Occupation"),
    ("BURI", "Burial"),
    ("CHR", "Christening"),
    ("BAPM", "Baptism"),
    ("RESI", "Residence"),
    ("ADOP", "Adopted"),
    ("EDUC", "Education"),
    ("CREM", "Cremation"),
    ("ENGA", "Engagement"),
    ("ANUL", "Annulment"),
    ("CENS", "Census"),
    ("EMIG", "Emigration"),
    ("IMMI", "Immigration"),
    ("GRAD", "Graduation"),
    ("RETI", "Retirement"),
    ("NATU", "Naturalization"),
    ("PROB", "Probate"),
    ("WILL", "Will"),
    ("CONF", "Confirmation"),
    ("FCOM", "First Communion"),
    ("ORDN", "Ordination"),
    ("NATI", "Nationality"),
    ("RELI", "Religion"),
    ("PROP", "Property"),
    ("TITL", "Nobility Title"),
    ("BARM", "Bar Mitzvah"),
    ("BASM", "Bas Mitzvah"),
    ("BLES", "Blessing"),
    ("CAST", "Caste"),
    ("DSCR", "Physical Description"),
    ("IDNO", "National Id"),
    ("SSN", "Social Security Number"),
    ("NCHI", "Number of Children"),
    ("NMR", "Number of Marriages"),
];

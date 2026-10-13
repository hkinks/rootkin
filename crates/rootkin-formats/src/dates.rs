//! Dates in GEDCOM and Gramps notation, via a neutral [`DateValue`].
//!
//! All conversion to and from `familyx::model::Date` lives in
//! [`to_model`] / [`from_model`] so the model's date shape is touched here only.

use familyx::model::Date;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    #[default]
    Exact,
    About,
    Estimated,
    Calculated,
    Before,
    After,
    /// `BET a AND b`
    Range,
    /// `FROM a TO b`
    Span,
    FromOnly,
    ToOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ymd {
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
}

impl Ymd {
    pub fn is_empty(&self) -> bool {
        self.year.is_none() && self.month.is_none()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DateValue {
    pub kind: Kind,
    pub start: Ymd,
    pub end: Ymd,
    /// Free text that could not be parsed; when set, the rest is empty.
    pub text: String,
}

const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const MONTH_NAMES: [&str; 12] = [
    "JANUARY",
    "FEBRUARY",
    "MARCH",
    "APRIL",
    "MAY",
    "JUNE",
    "JULY",
    "AUGUST",
    "SEPTEMBER",
    "OCTOBER",
    "NOVEMBER",
    "DECEMBER",
];

impl DateValue {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.start.is_empty() && self.end.is_empty()
    }

    // ---- GEDCOM ----

    pub fn to_gedcom(&self) -> String {
        if !self.text.is_empty() {
            return format!("({})", self.text);
        }
        let (a, b) = (gedcom_ymd(&self.start), gedcom_ymd(&self.end));
        match self.kind {
            Kind::Exact => a,
            Kind::About => format!("ABT {a}"),
            Kind::Estimated => format!("EST {a}"),
            Kind::Calculated => format!("CAL {a}"),
            Kind::Before => format!("BEF {a}"),
            Kind::After => format!("AFT {a}"),
            Kind::Range => format!("BET {a} AND {b}"),
            Kind::Span => format!("FROM {a} TO {b}"),
            Kind::FromOnly => format!("FROM {a}"),
            Kind::ToOnly => format!("TO {a}"),
        }
    }

    /// Never fails: what cannot be understood becomes [`DateValue::text`].
    pub fn from_gedcom(raw: &str) -> Self {
        let raw = raw.trim();
        let as_text = |s: &str| DateValue { text: s.trim_matches(['(', ')']).trim().to_string(), ..Default::default() };
        if raw.is_empty() {
            return Self::default();
        }
        if raw.starts_with('(') {
            return as_text(raw);
        }
        let mut s = raw;
        if s.starts_with("@#") {
            // calendar escape: only Gregorian maps onto our dates
            match s.split_once("@ ") {
                Some((esc, rest)) if esc.eq_ignore_ascii_case("@#DGREGORIAN") => s = rest.trim_start(),
                _ => return as_text(raw),
            }
        }
        // the remainder after a leading keyword such as `BEF `
        let after = |kw: &str| {
            let (head, rest) = s.split_once(' ')?;
            head.eq_ignore_ascii_case(kw).then_some(rest.trim())
        };
        let single = |kind, rest: &str| parse_ymd(rest).map(|start| DateValue { kind, start, ..Default::default() });
        let pair = |kind, a: &str, b: &str| {
            Some(DateValue { kind, start: parse_ymd(a)?, end: parse_ymd(b)?, ..Default::default() })
        };
        let result = if let Some(r) = after("BET") {
            split_ci(r, " AND ").and_then(|(a, b)| pair(Kind::Range, a, b))
        } else if let Some(r) = after("FROM") {
            match split_ci(r, " TO ") {
                Some((a, b)) => pair(Kind::Span, a, b),
                None => single(Kind::FromOnly, r),
            }
        } else if let Some(r) = after("TO") {
            single(Kind::ToOnly, r)
        } else if let Some(r) = after("BEF") {
            single(Kind::Before, r)
        } else if let Some(r) = after("AFT") {
            single(Kind::After, r)
        } else if let Some(r) = after("ABT").or_else(|| after("ABOUT")).or_else(|| after("INT")) {
            // INT carries a trailing "(phrase)"
            single(Kind::About, r.split('(').next().unwrap_or(r).trim())
        } else if let Some(r) = after("EST") {
            single(Kind::Estimated, r)
        } else if let Some(r) = after("CAL") {
            single(Kind::Calculated, r)
        } else {
            single(Kind::Exact, s)
        };
        result.unwrap_or_else(|| DateValue { text: raw.to_string(), ..Default::default() })
    }

    // ---- Gramps ----

    /// A `<dateval>`, `<daterange>`, `<datespan>` or `<datestr>` element, or
    /// an empty string for no date.
    pub fn to_gramps_xml(&self) -> String {
        let esc = |s: &str| familyx::doc::escape(s, true);
        if self.is_empty() {
            return String::new();
        }
        let dateval = |kind: &str| {
            let ty = if kind.is_empty() { String::new() } else { format!(" type=\"{kind}\"") };
            format!("<dateval val=\"{}\"{ty}/>", gramps_ymd(&self.start))
        };
        match self.kind {
            _ if !self.text.is_empty() => format!("<datestr val=\"{}\"/>", esc(&self.text)),
            Kind::Exact => dateval(""),
            Kind::About => dateval("about"),
            Kind::Estimated => dateval("estimated"),
            Kind::Calculated => dateval("calculated"),
            Kind::Before => dateval("before"),
            Kind::After => dateval("after"),
            Kind::Range => {
                format!("<daterange start=\"{}\" stop=\"{}\"/>", gramps_ymd(&self.start), gramps_ymd(&self.end))
            }
            Kind::Span => {
                format!("<datespan start=\"{}\" stop=\"{}\"/>", gramps_ymd(&self.start), gramps_ymd(&self.end))
            }
            // Gramps has no open-ended spans; keep them as text
            Kind::FromOnly | Kind::ToOnly => format!("<datestr val=\"{}\"/>", esc(&self.to_gedcom())),
        }
    }
}

fn gedcom_ymd(y: &Ymd) -> String {
    let month = y.month.filter(|m| (1..=12).contains(m)).map(|m| MONTHS[m as usize - 1]);
    match (y.year, month, y.day) {
        (Some(y), Some(m), Some(d)) => format!("{d} {m} {y}"),
        (Some(y), Some(m), None) => format!("{m} {y}"),
        (Some(y), None, _) => y.to_string(),
        (None, Some(m), Some(d)) => format!("{d} {m}"),
        (None, Some(m), None) => m.to_string(),
        (None, None, _) => String::new(),
    }
}

fn gramps_ymd(y: &Ymd) -> String {
    match (y.year, y.month, y.day) {
        (Some(y), Some(m), Some(d)) => format!("{y:04}-{m:02}-{d:02}"),
        (Some(y), Some(m), None) => format!("{y:04}-{m:02}"),
        (Some(y), None, _) => format!("{y:04}"),
        (None, m, d) => format!("0000-{:02}-{:02}", m.unwrap_or(0), d.unwrap_or(0)),
    }
}

/// Split at the first case-insensitive occurrence of an ASCII separator.
fn split_ci<'a>(s: &'a str, sep: &str) -> Option<(&'a str, &'a str)> {
    let at = s.to_ascii_uppercase().find(sep)?;
    Some((s[..at].trim(), s[at + sep.len()..].trim()))
}

fn parse_month(token: &str) -> Option<u32> {
    let t = token.trim_end_matches('.').to_uppercase();
    if t.len() < 3 || !t.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    MONTHS.iter().position(|m| *m == t).or_else(|| MONTH_NAMES.iter().position(|m| *m == t)).map(|i| i as u32 + 1)
}

fn parse_year(token: &str) -> Option<i32> {
    // dual dating "1750/51": keep the first year
    let head = token.split('/').next()?;
    if head.is_empty() || head.len() > 4 || !head.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    head.parse().ok()
}

fn parse_day(token: &str) -> Option<u32> {
    token.parse().ok().filter(|d| (1..=31).contains(d) && token.len() <= 2)
}

/// `14 MAR 1890`, `MAR 1890`, `1890`, `14 MAR`, `MAR`, or ISO `1890-03-14`.
pub fn parse_ymd(s: &str) -> Option<Ymd> {
    let tokens: Vec<&str> = s.split_whitespace().collect();
    match tokens.as_slice() {
        [one] if one.contains('-') => {
            let mut it = one.split('-');
            let year = parse_year(it.next()?)?;
            let month = it.next().and_then(|m| m.parse().ok()).filter(|m| (1..=12).contains(m));
            let day = it.next().and_then(parse_day);
            Some(Ymd { year: Some(year), month, day })
        }
        [t] => parse_year(t)
            .map(|year| Ymd { year: Some(year), ..Default::default() })
            .or_else(|| parse_month(t).map(|m| Ymd { month: Some(m), ..Default::default() })),
        [m, y] => match (parse_month(m), parse_year(y)) {
            (Some(month), Some(year)) => Some(Ymd { year: Some(year), month: Some(month), day: None }),
            _ => match (parse_day(m), parse_month(y)) {
                (Some(day), Some(month)) => Some(Ymd { year: None, month: Some(month), day: Some(day) }),
                _ => None,
            },
        },
        [d, m, y] => Some(Ymd { year: Some(parse_year(y)?), month: Some(parse_month(m)?), day: Some(parse_day(d)?) }),
        _ => None,
    }
}

/// Gramps `val`/`start`/`stop`: `YYYY-MM-DD` where unknown parts are `00`.
pub fn parse_gramps_ymd(s: &str) -> Ymd {
    let mut it = s.trim().splitn(3, '-');
    let num = |p: Option<&str>| p.and_then(|p| p.trim().parse::<u32>().ok()).filter(|n| *n > 0);
    let year = it.next().and_then(|y| y.trim().parse::<i32>().ok()).filter(|y| *y > 0);
    Ymd { year, month: num(it.next()).filter(|m| *m <= 12), day: num(it.next()).filter(|d| *d <= 31) }
}

// ---- familyx::model::Date <-> DateValue (the only place that knows the model's shape) ----

/// Returns the model date and, when the model cannot hold the value exactly,
/// the original date text to keep in the fact's details.
pub fn to_model(v: &DateValue) -> (Date, Option<String>) {
    if !v.text.is_empty() {
        let year = year_in(&v.text);
        let descriptor = if year.is_some() { "About" } else { "" };
        return (Date { year, descriptor: descriptor.into(), ..Default::default() }, Some(v.text.clone()));
    }
    let (descriptor, note) = match v.kind {
        Kind::Exact => ("Span", None),
        Kind::About | Kind::Estimated | Kind::Calculated => ("About", None),
        _ => ("About", Some(v.to_gedcom())),
    };
    (Date { year: v.start.year, month: v.start.month, day: v.start.day, descriptor: descriptor.into() }, note)
}

pub fn from_model(d: &Date) -> DateValue {
    let kind = if d.descriptor == "About" { Kind::About } else { Kind::Exact };
    DateValue { kind, start: Ymd { year: d.year, month: d.month, day: d.day }, ..Default::default() }
}

/// First 3-4 digit number in free text.
fn year_in(text: &str) -> Option<i32> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .find(|t| (3..=4).contains(&t.len()) && t.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|t| t.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ymd(y: i32, m: u32, d: u32) -> Ymd {
        Ymd { year: Some(y), month: Some(m), day: Some(d) }
    }

    #[test]
    fn gedcom_roundtrip() {
        for s in ["14 MAR 1890", "MAR 1890", "1950", "ABT 1950", "BEF 1900", "BET 1900 AND 1910", "FROM 1900 TO 1910"] {
            assert_eq!(DateValue::from_gedcom(s).to_gedcom(), s);
        }
    }

    #[test]
    fn parses_variants() {
        let v = DateValue::from_gedcom("  est 3 march 1901 ");
        assert_eq!((v.kind, v.start), (Kind::Estimated, ymd(1901, 3, 3)));
        assert_eq!(DateValue::from_gedcom("1890-03-14").start, ymd(1890, 3, 14));
        assert_eq!(DateValue::from_gedcom("@#DGREGORIAN@ 5 MAY 1900").start, ymd(1900, 5, 5));
        assert_eq!(DateValue::from_gedcom("1750/51").start.year, Some(1750));
        assert_eq!(DateValue::from_gedcom("(sometime in spring)").text, "sometime in spring");
        assert_eq!(DateValue::from_gedcom("@#DHEBREW@ 5 TSH 5700").text, "@#DHEBREW@ 5 TSH 5700");
        assert_eq!(DateValue::from_gedcom("garbled 1850s").text, "garbled 1850s");
    }

    #[test]
    fn gramps_values() {
        assert_eq!(parse_gramps_ymd("1890-03-00"), Ymd { year: Some(1890), month: Some(3), day: None });
        assert_eq!(parse_gramps_ymd("0000-00-00"), Ymd::default());
        assert_eq!(gramps_ymd(&ymd(1890, 3, 4)), "1890-03-04");
    }

    #[test]
    fn model_conversion_keeps_year_and_note() {
        let (d, note) = to_model(&DateValue::from_gedcom("BEF 1900"));
        assert_eq!((d.year, d.descriptor.as_str(), note.as_deref()), (Some(1900), "About", Some("BEF 1900")));
        let (d, note) = to_model(&DateValue::from_gedcom("14 MAR 1890"));
        assert_eq!((d.day, note), (Some(14), None));
    }
}

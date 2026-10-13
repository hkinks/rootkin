//! Genealogical dates: the app's `CustomDate` (start, optional end, qualifier,
//! free text), plus lenient parsing from and formatting to human text.

/// A date as stored in `Fact/FactDate`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Date {
    /// The app's `DateDescriptor`; empty means `Exact` (the serialized default).
    /// Others: `About`, `Estimated`, `Calculated`, `Before`, `After`,
    /// `Range` (between start and end), `Span` (from start to end),
    /// `FromOnly`, `ToOnly`, `EitherOr`, `Interpreted`, `TextOnly`.
    pub descriptor: String,
    pub year: Option<i32>,
    pub month: Option<u32>,
    pub day: Option<u32>,
    pub end_year: Option<i32>,
    pub end_month: Option<u32>,
    pub end_day: Option<u32>,
    /// Free text, used with `TextOnly` (and kept for `Interpreted`).
    pub text: String,
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Qualifier keywords accepted when parsing, longest first within a group.
const PREFIXES: &[(&[&str], &str)] = &[
    (&["approximately", "about", "approx", "abt", "circa", "ca", "c", "~"], "About"),
    (&["estimated", "est"], "Estimated"),
    (&["calculated", "calc", "cal"], "Calculated"),
    (&["before", "bef", "<"], "Before"),
    (&["after", "aft", ">"], "After"),
];

impl Date {
    pub fn year(year: i32) -> Self {
        Date { year: Some(year), ..Default::default() }
    }

    pub fn ymd(year: i32, month: u32, day: u32) -> Self {
        Date { year: Some(year), month: Some(month), day: Some(day), ..Default::default() }
    }

    pub fn with_descriptor(mut self, descriptor: &str) -> Self {
        self.descriptor = descriptor.to_string();
        self
    }

    pub fn is_empty(&self) -> bool {
        self.year.is_none() && self.month.is_none() && self.day.is_none() && self.text.trim().is_empty()
    }

    /// Human-readable form, e.g. `14 Mar 1890`, `abt. 1950`, `between 1900 and 1910`.
    /// [`Date::parse`] reads it back to the same value.
    pub fn display(&self) -> String {
        if self.descriptor == "TextOnly" {
            return format!("({})", self.text);
        }
        let start = format_part(self.year, self.month, self.day);
        let end = format_part(self.end_year, self.end_month, self.end_day);
        if start.is_empty() && end.is_empty() {
            return if self.text.is_empty() { String::new() } else { format!("({})", self.text) };
        }
        match self.descriptor.as_str() {
            "About" => format!("abt. {start}"),
            "Estimated" => format!("est. {start}"),
            "Calculated" => format!("calc. {start}"),
            "Before" => format!("before {start}"),
            "After" => format!("after {start}"),
            "Range" => format!("between {start} and {end}"),
            "Span" => format!("from {start} to {end}"),
            "FromOnly" => format!("from {start}"),
            "ToOnly" => format!("to {start}"),
            "EitherOr" => format!("{start} or {end}"),
            _ => start,
        }
    }

    /// Lenient parse of a typed date. Empty input gives an empty date.
    /// Accepts e.g. `1950`, `Mar 1950`, `12 Mar 1950`, `1950-03-12`, `12.03.1950`,
    /// `abt 1950`, `c. 1950`, `before 1900`, `between 1900 and 1910`,
    /// `1900-1910`, `from 1900 to 1910`, `1900 or 1901`, and `(free text)`.
    pub fn parse(input: &str) -> Result<Date, String> {
        let s = input.trim();
        if s.is_empty() {
            return Ok(Date::default());
        }
        if let Some(text) = s.strip_prefix('(').and_then(|t| t.strip_suffix(')')) {
            return Ok(Date { descriptor: "TextOnly".into(), text: text.trim().into(), ..Default::default() });
        }
        let lower = s.to_lowercase();

        let two = |descriptor: &str, a: &str, b: &str| -> Result<Date, String> {
            let (y, m, d) = parse_part(a)?;
            let (ey, em, ed) = parse_part(b)?;
            Ok(Date {
                descriptor: descriptor.into(),
                year: y,
                month: m,
                day: d,
                end_year: ey,
                end_month: em,
                end_day: ed,
                text: String::new(),
            })
        };
        let one = |descriptor: &str, a: &str| -> Result<Date, String> {
            let (y, m, d) = parse_part(a)?;
            Ok(Date { descriptor: descriptor.into(), year: y, month: m, day: d, ..Default::default() })
        };

        for kw in ["between", "bet"] {
            if let Some(rest) = strip_word(&lower, kw) {
                let (a, b) = rest.split_once(" and ").ok_or("expected \"between … and …\"")?;
                return two("Range", a, b);
            }
        }
        if let Some(rest) = strip_word(&lower, "from") {
            return match rest.split_once(" to ") {
                Some((a, b)) => two("Span", a, b),
                None => one("FromOnly", rest),
            };
        }
        if let Some(rest) = strip_word(&lower, "to") {
            return one("ToOnly", rest);
        }
        if let Some((a, b)) = lower.split_once(" or ") {
            return two("EitherOr", a, b);
        }
        // "1900-1910" / "1900–1910" as a range of years
        if let Some((a, b)) = lower
            .split_once(['–', '—'])
            .or_else(|| lower.split_once('-').filter(|(a, b)| is_year(a.trim()) && is_year(b.trim())))
        {
            return two("Range", a, b);
        }
        for (words, descriptor) in PREFIXES {
            for w in *words {
                if let Some(rest) = strip_word(&lower, w) {
                    return one(descriptor, rest);
                }
            }
        }
        one("", &lower)
    }
}

fn is_year(s: &str) -> bool {
    (1..=4).contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
}

/// `rest` if `s` starts with the keyword `word` (optionally followed by `.`).
fn strip_word<'a>(s: &'a str, word: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(word)?;
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    let symbolic = !word.chars().next().is_some_and(char::is_alphanumeric);
    (symbolic || rest.starts_with(' ') || rest.is_empty()).then(|| rest.trim())
}

fn format_part(year: Option<i32>, month: Option<u32>, day: Option<u32>) -> String {
    let month_name = |m: u32| MONTHS.get(m as usize - 1).copied().unwrap_or("?");
    match (day, month, year) {
        (Some(d), Some(m), Some(y)) => format!("{d} {} {y}", month_name(m)),
        (None, Some(m), Some(y)) => format!("{} {y}", month_name(m)),
        (_, None, Some(y)) => y.to_string(),
        (Some(d), Some(m), None) => format!("{d} {}", month_name(m)),
        (None, Some(m), None) => month_name(m).to_string(),
        _ => String::new(),
    }
}

fn month_from_name(s: &str) -> Option<u32> {
    let s = s.trim_end_matches('.');
    if s.len() < 3 {
        return None;
    }
    const FULL: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    FULL.iter().position(|m| m.starts_with(s)).map(|i| i as u32 + 1)
}

pub fn days_in_month(year: Option<i32>, month: u32) -> u32 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 => match year {
            Some(y) if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
            Some(_) => 28,
            None => 29,
        },
        _ => 31,
    }
}

type Part = (Option<i32>, Option<u32>, Option<u32>);

/// One calendar date, possibly partial: `1950`, `mar 1950`, `12 mar 1950`,
/// `1950-03-12`, `1950-03`, `12.03.1950`, `12/03/1950`, `03.1950`.
fn parse_part(s: &str) -> Result<Part, String> {
    let s = s.trim().trim_end_matches(['.', ',']);
    if s.is_empty() {
        return Err("missing date".into());
    }
    let tokens: Vec<&str> = s.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).collect();
    let num = |t: &str| t.parse::<u32>().ok();
    let (year, month, day): (Option<i32>, Option<u32>, Option<u32>) = if tokens.len() == 1 {
        let t = tokens[0];
        if let Ok(y) = t.parse::<i32>() {
            (Some(y), None, None)
        } else if let Some(m) = month_from_name(t) {
            (None, Some(m), None)
        } else if t.contains('-') && t.split('-').next().is_some_and(|y| y.len() == 4) {
            let p: Vec<&str> = t.split('-').collect();
            let year = p[0].parse().ok();
            (year, p.get(1).and_then(|m| num(m)), p.get(2).and_then(|d| num(d)))
        } else if t.contains(['.', '/']) {
            let p: Vec<&str> = t.split(['.', '/']).collect();
            match p.as_slice() {
                [d, m, y] => (y.parse().ok(), num(m), num(d)),
                [m, y] => (y.parse().ok(), num(m), None),
                _ => return Err(format!("can't read date \"{s}\"")),
            }
        } else {
            return Err(format!("can't read date \"{s}\""));
        }
    } else {
        // word forms: "12 mar 1950", "mar 12 1950", "mar 1950", "12 mar"
        let mut year = None;
        let mut month = None;
        let mut day = None;
        for t in &tokens {
            if let Some(m) = month_from_name(t) {
                month = Some(m);
            } else if let Some(n) = num(t.trim_end_matches(['.', 'h', 't', 's', 'n', 'd', 'r'])) {
                if t.len() >= 3 || day.is_some() || n > 31 {
                    year = Some(n as i32);
                } else {
                    day = Some(n);
                }
            } else {
                return Err(format!("can't read \"{t}\" in date"));
            }
        }
        (year, month, day)
    };
    if year.is_none() && month.is_none() {
        return Err(format!("can't read date \"{s}\""));
    }
    if let Some(m) = month {
        if !(1..=12).contains(&m) {
            return Err(format!("month {m} doesn't exist"));
        }
    }
    if let Some(d) = day {
        let max = month.map_or(31, |m| days_in_month(year, m));
        if d == 0 || d > max {
            return Err(format!("day {d} doesn't exist in that month"));
        }
    }
    Ok((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_forms() {
        assert_eq!(Date::parse("1950").unwrap(), Date::year(1950));
        assert_eq!(Date::parse("12 Mar 1950").unwrap(), Date::ymd(1950, 3, 12));
        assert_eq!(Date::parse("March 12, 1950").unwrap(), Date::ymd(1950, 3, 12));
        assert_eq!(Date::parse("1950-03-12").unwrap(), Date::ymd(1950, 3, 12));
        assert_eq!(Date::parse("12.03.1950").unwrap(), Date::ymd(1950, 3, 12));
        assert_eq!(Date::parse("mar 1950").unwrap(), Date { year: Some(1950), month: Some(3), ..Default::default() });
        assert_eq!(Date::parse("abt 1950").unwrap(), Date::year(1950).with_descriptor("About"));
        assert_eq!(Date::parse("c. 1950").unwrap(), Date::year(1950).with_descriptor("About"));
        assert_eq!(Date::parse("~1950").unwrap(), Date::year(1950).with_descriptor("About"));
        assert_eq!(Date::parse("before 1900").unwrap(), Date::year(1900).with_descriptor("Before"));
        let range = Date::parse("between 1900 and 1910").unwrap();
        assert_eq!((range.descriptor.as_str(), range.year, range.end_year), ("Range", Some(1900), Some(1910)));
        assert_eq!(Date::parse("1900-1910").unwrap(), range);
        let span = Date::parse("from 1 Jan 1900 to 1910").unwrap();
        assert_eq!((span.descriptor.as_str(), span.day, span.end_year), ("Span", Some(1), Some(1910)));
        assert_eq!(Date::parse("(during the war)").unwrap().text, "during the war");
    }

    #[test]
    fn rejects_impossible_dates() {
        assert!(Date::parse("31 Feb 1950").is_err());
        assert!(Date::parse("29 Feb 1900").is_err());
        assert!(Date::parse("29 Feb 2000").is_ok());
        assert!(Date::parse("1950-13-01").is_err());
        assert!(Date::parse("sometime").is_err());
    }

    #[test]
    fn display_parses_back() {
        let dates = [
            Date::ymd(1890, 3, 14),
            Date::year(1950).with_descriptor("About"),
            Date::year(1950).with_descriptor("Estimated"),
            Date::year(1950).with_descriptor("Calculated"),
            Date::year(1900).with_descriptor("Before"),
            Date::year(1900).with_descriptor("After"),
            Date::year(1900).with_descriptor("FromOnly"),
            Date::year(1900).with_descriptor("ToOnly"),
            Date { end_year: Some(1910), ..Date::year(1900).with_descriptor("Range") },
            Date { end_year: Some(1910), end_month: Some(5), ..Date::ymd(1900, 1, 2).with_descriptor("Span") },
            Date { end_year: Some(1901), ..Date::year(1900).with_descriptor("EitherOr") },
            Date { descriptor: "TextOnly".into(), text: "during the war".into(), ..Default::default() },
            Date::default(),
        ];
        for d in dates {
            let shown = d.display();
            assert_eq!(Date::parse(&shown).unwrap(), d, "{shown}");
        }
    }
}

//! Plain-text rendering of the app's formatted notes (`text/html`,
//! `text/richtext` = RTF). Good enough to read and to convert a note to plain
//! text for editing; formatting is dropped.

/// Plain text for a note body in the given MIME-like format.
pub fn to_plain(format: &str, text: &str) -> String {
    match format {
        "text/html" => html_to_plain(text),
        "text/richtext" => rtf_to_plain(text),
        _ => text.to_string(),
    }
}

pub fn is_rich(format: &str) -> bool {
    matches!(format, "text/html" | "text/richtext")
}

fn html_to_plain(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else {
            rest = &rest[lt..];
            break;
        };
        let tag = rest[lt + 1..lt + gt].trim_start_matches('/').to_ascii_lowercase();
        let name = tag.split(|c: char| !c.is_ascii_alphanumeric()).next().unwrap_or("");
        if matches!(name, "br" | "p" | "div" | "li" | "tr") && !out.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        rest = &rest[lt + gt + 1..];
    }
    out.push_str(rest);
    crate::doc::unescape(out.trim()).replace('\u{a0}', " ")
}

fn rtf_to_plain(rtf: &str) -> String {
    let bytes = rtf.as_bytes();
    let mut out = String::new();
    // depth at which a skipped destination group started
    let mut skip_from: Option<usize> = None;
    let mut depth = 0usize;
    // characters to skip after \uN (the ANSI fallback), per \ucN
    let mut uc = 1usize;
    let mut pending_skip = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        match c {
            b'{' => {
                depth += 1;
                i += 1;
                // destinations that never contain visible text
                let ahead = &rtf[i..];
                if skip_from.is_none()
                    && ["\\*", "\\fonttbl", "\\colortbl", "\\stylesheet", "\\info", "\\pict", "\\listtable"]
                        .iter()
                        .any(|d| ahead.starts_with(d))
                {
                    skip_from = Some(depth);
                }
            }
            b'}' => {
                if skip_from == Some(depth) {
                    skip_from = None;
                }
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b'\\' => {
                i += 1;
                let Some(&next) = bytes.get(i) else { break };
                if next == b'\'' {
                    // \'hh: a byte in the ANSI code page (cp1252 ≈ Latin-1)
                    // up to two hex digits (ASCII, so `i` stays on a character boundary)
                    let digits = bytes[i + 1..].iter().take(2).take_while(|b| b.is_ascii_hexdigit()).count();
                    let hex = &rtf[i + 1..i + 1 + digits];
                    i += 1 + digits;
                    if pending_skip > 0 {
                        pending_skip -= 1;
                    } else if skip_from.is_none() {
                        if let Ok(b) = u8::from_str_radix(hex, 16) {
                            out.push(b as char);
                        }
                    }
                    continue;
                }
                if !next.is_ascii_alphabetic() {
                    // escaped literal: \\ \{ \} or a control symbol
                    if skip_from.is_none() && matches!(next, b'\\' | b'{' | b'}') {
                        out.push(next as char);
                    }
                    // a non-ASCII symbol is skipped whole
                    i += rtf[i..].chars().next().map_or(1, char::len_utf8);
                    continue;
                }
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                let word = &rtf[start..i];
                let num_start = i;
                if i < bytes.len() && (bytes[i] == b'-' || bytes[i].is_ascii_digit()) {
                    i += 1;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let param: Option<i32> = rtf[num_start..i].parse().ok();
                if i < bytes.len() && bytes[i] == b' ' {
                    i += 1;
                }
                if skip_from.is_some() {
                    continue;
                }
                match word {
                    "par" | "line" => out.push('\n'),
                    "tab" => out.push('\t'),
                    "uc" => uc = param.unwrap_or(1).max(0) as usize,
                    "u" => {
                        let code = param.unwrap_or(0);
                        let code = if code < 0 { code + 65536 } else { code };
                        if let Some(ch) = char::from_u32(code as u32) {
                            out.push(ch);
                        }
                        pending_skip = uc;
                    }
                    _ => {}
                }
            }
            b'\r' | b'\n' => i += 1,
            _ => {
                let ch_len = rtf[i..].chars().next().map_or(1, char::len_utf8);
                if pending_skip > 0 {
                    pending_skip -= 1;
                } else if skip_from.is_none() {
                    out.push_str(&rtf[i..i + ch_len]);
                }
                i += ch_len;
            }
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html() {
        assert_eq!(
            to_plain("text/html", "<p>Hello <b>world</b> &amp; co</p><p>Line 2</p>"),
            "Hello world & co\nLine 2"
        );
    }

    #[test]
    fn rtf() {
        let rtf = r"{\rtf1\ansi\ansicpg1252\uc1\deff2{\fonttbl{\f0\fcharset0 Times New Roman;}}{\colortbl\red0\green0\blue0;}{\*\generator x;}\pard\f2 Born in P\'e4rnu\par Second \u8364?\par}";
        assert_eq!(to_plain("text/richtext", rtf), "Born in Pärnu\nSecond €");
    }

    #[test]
    fn rtf_with_non_ascii_after_an_escape_does_not_panic() {
        assert_eq!(to_plain("text/richtext", "{\\rtf1 x\\'aé}"), "x\né");
        assert_eq!(to_plain("text/richtext", "{\\rtf1 \\é x}"), "x");
        assert_eq!(to_plain("text/richtext", "{\\rtf1 \\'é}"), "é");
        assert_eq!(to_plain("text/richtext", "{\\rtf1 \\'"), "");
        // every cut of a mixed text is handled
        let text = "{\\rtf1 P\\'e4rnu \\u8364? Šö\\'\\é}";
        for end in (0..=text.len()).filter(|&n| text.is_char_boundary(n)) {
            let _ = to_plain("text/richtext", &text[..end]);
        }
    }

    #[test]
    fn plain_passes_through() {
        assert_eq!(to_plain("text/plain", "a <b>"), "a <b>");
    }
}

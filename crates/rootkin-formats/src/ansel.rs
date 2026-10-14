//! ANSEL (ANSI Z39.47) decoding, as written by PAF, older Legacy and RootsMagic
//! (`CHAR ANSEL`). Diacritics are separate bytes that come *before* the letter
//! they belong to; the result is composed to NFC (`M\xe8ae` -> "Mäe").

use unicode_normalization::UnicodeNormalization;

/// A byte in 0xE0..=0xFE that is a combining mark, as a Unicode combining character.
fn combining(b: u8) -> Option<char> {
    Some(match b {
        0xE0 => '\u{0309}', // hook above
        0xE1 => '\u{0300}', // grave
        0xE2 => '\u{0301}', // acute
        0xE3 => '\u{0302}', // circumflex
        0xE4 => '\u{0303}', // tilde
        0xE5 => '\u{0304}', // macron
        0xE6 => '\u{0306}', // breve
        0xE7 => '\u{0307}', // dot above
        0xE8 => '\u{0308}', // diaeresis
        0xE9 => '\u{030C}', // caron
        0xEA => '\u{030A}', // ring above
        0xEB => '\u{FE20}', // ligature, left half
        0xEC => '\u{FE21}', // ligature, right half
        0xED => '\u{0315}', // comma above right
        0xEE => '\u{030B}', // double acute
        0xEF => '\u{0310}', // candrabindu
        0xF0 => '\u{0327}', // cedilla
        0xF1 => '\u{0328}', // ogonek
        0xF2 => '\u{0323}', // dot below
        0xF3 => '\u{0324}', // double dot below
        0xF4 => '\u{0325}', // ring below
        0xF5 => '\u{0333}', // double underscore
        0xF6 => '\u{0332}', // underscore
        0xF7 => '\u{0326}', // comma below
        0xF8 => '\u{031C}', // left half ring below
        0xF9 => '\u{032E}', // breve below
        0xFA => '\u{FE22}', // double tilde, left half
        0xFB => '\u{FE23}', // double tilde, right half
        0xFE => '\u{0313}', // comma above
        _ => return None,
    })
}

/// A byte in 0x80..=0xFF that stands for a letter or symbol on its own.
fn spacing(b: u8) -> Option<char> {
    Some(match b {
        0x8D => '\u{200D}', // zero width joiner
        0x8E => '\u{200C}', // zero width non-joiner
        0xA1 => 'Ł',
        0xA2 => 'Ø',
        0xA3 => 'Đ',
        0xA4 => 'Þ',
        0xA5 => 'Æ',
        0xA6 => 'Œ',
        0xA7 => '\u{02B9}', // soft sign / prime
        0xA8 => '·',
        0xA9 => '♭',
        0xAA => '®',
        0xAB => '±',
        0xAC => 'Ơ',
        0xAD => 'Ư',
        0xAE => '\u{02BC}', // alif / apostrophe
        0xB0 => '\u{02BB}', // ayn
        0xB1 => 'ł',
        0xB2 => 'ø',
        0xB3 => 'đ',
        0xB4 => 'þ',
        0xB5 => 'æ',
        0xB6 => 'œ',
        0xB7 => '\u{02BA}', // hard sign / double prime
        0xB8 => 'ı',
        0xB9 => '£',
        0xBA => 'ð',
        0xBC => 'ơ',
        0xBD => 'ư',
        0xC0 => '°',
        0xC1 => 'ℓ',
        0xC2 => '℗',
        0xC3 => '©',
        0xC4 => '♯',
        0xC5 => '¿',
        0xC6 => '¡',
        0xC7 | 0xCF => 'ß', // eszett (0xCF in PAF and Legacy exports)
        0xC8 => '€',
        _ => return None,
    })
}

/// Decode ANSEL bytes. Unassigned bytes become U+FFFD.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    // marks seen since the last letter; they go after the next letter
    let mut marks = String::new();
    for &b in bytes {
        if let Some(mark) = combining(b) {
            marks.push(mark);
            continue;
        }
        let ch = match b {
            0x00..=0x7F => b as char,
            _ => spacing(b).unwrap_or('\u{FFFD}'),
        };
        out.push(ch);
        out.push_str(&std::mem::take(&mut marks));
    }
    out.push_str(&marks);
    out.nfc().collect()
}

/// Does the GEDCOM header (first lines) declare `CHAR ANSEL`?
pub fn declared(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(4096)];
    head.windows(10).any(|w| w.eq_ignore_ascii_case(b"CHAR ANSEL"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diacritics_precede_the_letter_and_compose() {
        assert_eq!(decode(b"M\xe8ae"), "Mäe");
        assert_eq!(decode(b"P\xe4oder"), "Põder");
        assert_eq!(decode(b"Ren\xe2e"), "René");
        assert_eq!(decode(b"\xe9Svejk Fran\xf0cois"), "Švejk François");
    }

    #[test]
    fn letters_and_symbols() {
        assert_eq!(decode(b"\xa1\xe2od\xbc \xb2 \xcf"), "Łódơ ø ß");
    }

    #[test]
    fn declared_in_header() {
        assert!(declared(b"0 HEAD\r\n1 CHAR ANSEL\r\n"));
        assert!(!declared(b"0 HEAD\r\n1 CHAR UTF-8\r\n"));
    }
}

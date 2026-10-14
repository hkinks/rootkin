//! Encryption/compression of the tree part (`family8` and older variants).
//!
//! # family8 / family7 (AES containers)
//!
//! Layout: `HMAC-SHA512 (64) | version tag (32) | salt (32) | IV (16) | ciphertext`.
//! The ciphertext is AES-256-CBC/PKCS7 over raw DEFLATE of the UTF-8 XML.
//! Keys derive from a constant application password (UTF-16LE) via PBKDF2 and
//! an SP800-108 counter-mode KDF (HMAC-SHA512); the MAC covers everything
//! after itself. The first byte of the tag selects the parameter set:
//!
//! | tag byte | version | PBKDF2 hash | iterations | master key |
//! |----------|---------|-------------|------------|------------|
//! | `0x3A`   | V1      | SHA-256     | 10 000     | 256 bytes  |
//! | `0x5A`   | V2      | SHA-256     | 10 000     | 32 bytes   |
//! | `0x7C`   | V3      | SHA-512     | 210 000    | 32 bytes   |
//!
//! `family8` uses one password and `family7` another; both accept all three
//! versions. Only `family8` V3 is ever written.
//!
//! # family5 / family4 / family (legacy)
//!
//! `[32 bytes, ignored] | salt (32) | ciphertext` (`family` has no header; its
//! salt is a constant). Key and IV = PBKDF2-HMAC-SHA1(password UTF-8, salt, n,
//! 48 bytes); AES-256-CBC/PKCS7 over raw DEFLATE of the XML. There is no MAC.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Sha256, Sha512};
use std::io::{Read, Write};

use crate::{Error, Result};

type HmacSha512 = Hmac<Sha512>;

/// `family8` password (UTF-16LE for key derivation).
const PASSWORD_F8: &str = "1@42%e23/;.j)Y3#{#.s5INU2V#:_Dx)w-R~_7812c448,,T%92L}Yt?{B*(=$1";
/// `family7` password (UTF-16LE).
const PASSWORD_F7: &str = "rzy2z3AAOw4Hv2l1jZLq6BYcGVEWy3fS52rTebxVMPgE6SQDg1ONfHZnA6UyQvt";
/// `family5` password (UTF-8), 10 622 iterations.
const PASSWORD_F5: &str = "IxXqhSlqHxKvCniULelX9sDnXhqzdakEoFEcmJf6OYmnDfGZrIkb4RIA0wnu71P";
/// `family4` password (UTF-8), 10 564 iterations.
const PASSWORD_F4: &str = "A9426036-1CCF-427C-896C-33E05CAF829F";
/// `family` password and constant salt (UTF-8), 1 258 iterations.
const PASSWORD_F1: &str = "0B0995E2-5285-4B85-86E2-E90EAEA7397C";

/// Bytes 1.. of the version tag, shared by all versions.
const TAG_TAIL: [u8; 31] = [
    0x42, 0x9A, 0x26, 0x62, 0x18, 0x14, 0x53, 0x38, 0x6A, 0x06, 0x7D, 0x5B, 0xB8, 0xC0, 0x2A, 0x52, 0x92, 0xCB, 0xDB,
    0x15, 0xEE, 0x91, 0x58, 0x92, 0xCE, 0x38, 0xBE, 0xFF, 0x2D, 0xF5, 0x66,
];
const HEADER_LEN: usize = 64 + 32 + 32 + 16;

/// Parameter set of the AES container (`family8`, `family7`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AesVersion {
    V1,
    V2,
    V3,
}

impl AesVersion {
    fn tag(self) -> [u8; 32] {
        let mut tag = [0u8; 32];
        tag[0] = match self {
            AesVersion::V1 => 0x3A,
            AesVersion::V2 => 0x5A,
            AesVersion::V3 => 0x7C,
        };
        tag[1..].copy_from_slice(&TAG_TAIL);
        tag
    }

    fn from_tag(tag: &[u8]) -> Option<Self> {
        [AesVersion::V1, AesVersion::V2, AesVersion::V3].into_iter().find(|v| v.tag() == tag)
    }

    /// PBKDF2 output (the master key) as the app computes it.
    fn master_key(self, password: &[u8], salt: &[u8]) -> Vec<u8> {
        match self {
            AesVersion::V1 => {
                let mut out = vec![0u8; 256];
                pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, 10_000, &mut out);
                out
            }
            AesVersion::V2 => {
                let mut out = vec![0u8; 32];
                pbkdf2::pbkdf2_hmac::<Sha256>(password, salt, 10_000, &mut out);
                out
            }
            AesVersion::V3 => {
                let mut out = vec![0u8; 32];
                pbkdf2::pbkdf2_hmac::<Sha512>(password, salt, 210_000, &mut out);
                out
            }
        }
    }
}

/// Which tree part, and which variant of its encryption, a package was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceFormat {
    /// Current format (app 16.x): `family8`.
    Family8(AesVersion),
    /// `family7`: same container as `family8` with a different password.
    Family7(AesVersion),
    /// `family5`: PBKDF2-SHA1 + AES-CBC, no MAC.
    Family5,
    /// `family4`: PBKDF2-SHA1 + AES-CBC, no MAC.
    Family4,
    /// `family`: oldest encrypted format, constant salt, no header.
    Family,
    /// `content.xml`: the tree as plain, unencrypted XML.
    ContentXml,
}

impl SourceFormat {
    /// What [`encode`] writes and what saving a package always produces.
    pub const CURRENT: SourceFormat = SourceFormat::Family8(AesVersion::V3);

    /// Name of the zip entry that holds the tree in this format.
    pub fn part_name(self) -> &'static str {
        match self {
            SourceFormat::Family8(_) => "family8",
            SourceFormat::Family7(_) => "family7",
            SourceFormat::Family5 => "family5",
            SourceFormat::Family4 => "family4",
            SourceFormat::Family => "family",
            SourceFormat::ContentXml => "content.xml",
        }
    }

    pub fn is_current(self) -> bool {
        self == Self::CURRENT
    }
}

impl std::fmt::Display for SourceFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceFormat::Family8(v) | SourceFormat::Family7(v) => write!(f, "{} ({v:?})", self.part_name()),
            _ => f.write_str(self.part_name()),
        }
    }
}

/// Tree part names, newest first; the first one present in a package is read.
pub const PART_NAMES: [&str; 6] = ["family8", "family7", "family5", "family4", "family", "content.xml"];

struct Keys {
    mac: [u8; 32],
    enc: [u8; 32],
}

fn derive_keys(password: &str, version: AesVersion, salt: &[u8]) -> Keys {
    let password: Vec<u8> = password.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let master = version.master_key(&password, salt);
    let kdf = |suffix: &[u8]| {
        let mut mac = HmacSha512::new_from_slice(&master).expect("any key length");
        mac.update(&1u32.to_be_bytes());
        mac.update(b"ChronoplexSoftware\0xml");
        mac.update(suffix);
        mac.update(&256u32.to_be_bytes());
        let mut key = [0u8; 32];
        key.copy_from_slice(&mac.finalize().into_bytes()[..32]);
        key
    };
    Keys { mac: kdf(b"|mac_key"), enc: kdf(b"|encryption_key") }
}

fn inflate(compressed: &[u8]) -> Result<Vec<u8>> {
    let mut xml = Vec::with_capacity(compressed.len() * 5);
    flate2::read::DeflateDecoder::new(compressed)
        .read_to_end(&mut xml)
        .map_err(|e| Error::Format(format!("cannot inflate tree data (wrong format or corrupt): {e}")))?;
    Ok(xml)
}

fn deflate(xml: &[u8]) -> Vec<u8> {
    let mut deflater = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
    deflater.write_all(xml).expect("writing to a Vec cannot fail");
    deflater.finish().expect("writing to a Vec cannot fail")
}

fn decode_aes(blob: &[u8], password: &str) -> Result<(Vec<u8>, AesVersion)> {
    if blob.len() < HEADER_LEN || !(blob.len() - HEADER_LEN).is_multiple_of(16) {
        return Err(Error::Format("tree part has an invalid length".into()));
    }
    let version = AesVersion::from_tag(&blob[64..96])
        .ok_or_else(|| Error::Format("unknown tree part version tag (expected V1, V2 or V3)".into()))?;
    let salt = &blob[96..128];
    let iv = &blob[128..144];
    let keys = derive_keys(password, version, salt);

    let mut mac = HmacSha512::new_from_slice(&keys.mac).expect("any key length");
    mac.update(&blob[64..]);
    mac.verify_slice(&blob[..64]).map_err(|_| Error::Mac)?;

    let compressed = cbc::Decryptor::<aes::Aes256>::new(&keys.enc.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&blob[HEADER_LEN..])
        .map_err(|_| Error::Format("bad padding".into()))?;
    Ok((inflate(&compressed)?, version))
}

fn encode_aes(xml: &[u8], password: &str, version: AesVersion, salt: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let keys = derive_keys(password, version, salt);
    let ciphertext =
        cbc::Encryptor::<aes::Aes256>::new(&keys.enc.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(&deflate(xml));

    let mut body = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    body.extend_from_slice(&version.tag());
    body.extend_from_slice(salt);
    body.extend_from_slice(iv);
    body.extend_from_slice(&ciphertext);
    let mut mac = HmacSha512::new_from_slice(&keys.mac).expect("any key length");
    mac.update(&body);

    let mut out = mac.finalize().into_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

fn legacy_key_iv(password: &str, salt: &[u8], iterations: u32) -> [u8; 48] {
    let mut key_iv = [0u8; 48];
    pbkdf2::pbkdf2_hmac::<sha1::Sha1>(password.as_bytes(), salt, iterations, &mut key_iv);
    key_iv
}

fn decode_legacy(blob: &[u8], password: &str, iterations: u32, constant_salt: bool) -> Result<Vec<u8>> {
    let (salt, ciphertext) = if constant_salt {
        (password.as_bytes(), blob)
    } else {
        if blob.len() < 64 {
            return Err(Error::Format("tree part is too short".into()));
        }
        (&blob[32..64], &blob[64..])
    };
    if ciphertext.is_empty() || !ciphertext.len().is_multiple_of(16) {
        return Err(Error::Format("tree part has an invalid length".into()));
    }
    let key_iv = legacy_key_iv(password, salt, iterations);
    let compressed = cbc::Decryptor::<aes::Aes256>::new(key_iv[..32].into(), key_iv[32..].into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| Error::Format("bad padding (corrupt legacy tree part)".into()))?;
    inflate(&compressed)
}

fn encode_legacy(xml: &[u8], password: &str, iterations: u32, constant_salt: bool) -> Vec<u8> {
    let mut header = [0u8; 64];
    rand::thread_rng().fill_bytes(&mut header);
    let salt: &[u8] = if constant_salt { password.as_bytes() } else { &header[32..] };
    let key_iv = legacy_key_iv(password, salt, iterations);
    let ciphertext = cbc::Encryptor::<aes::Aes256>::new(key_iv[..32].into(), key_iv[32..].into())
        .encrypt_padded_vec_mut::<Pkcs7>(&deflate(xml));
    if constant_salt {
        ciphertext
    } else {
        [&header[..], &ciphertext].concat()
    }
}

/// Decode the tree stored in zip entry `part_name` into XML bytes and report
/// the exact variant it was stored in. `part_name` is one of [`PART_NAMES`].
pub fn decode_part(part_name: &str, blob: &[u8]) -> Result<(Vec<u8>, SourceFormat)> {
    match part_name {
        "family8" => decode_aes(blob, PASSWORD_F8).map(|(xml, v)| (xml, SourceFormat::Family8(v))),
        "family7" => decode_aes(blob, PASSWORD_F7).map(|(xml, v)| (xml, SourceFormat::Family7(v))),
        "family5" => Ok((decode_legacy(blob, PASSWORD_F5, 10_622, false)?, SourceFormat::Family5)),
        "family4" => Ok((decode_legacy(blob, PASSWORD_F4, 10_564, false)?, SourceFormat::Family4)),
        "family" => Ok((decode_legacy(blob, PASSWORD_F1, 1_258, true)?, SourceFormat::Family)),
        "content.xml" => Ok((blob.to_vec(), SourceFormat::ContentXml)),
        other => Err(Error::Format(format!("{other} is not a known tree part"))),
    }
}

/// Encode XML in any known variant. The editor writes [`SourceFormat::CURRENT`], and
/// `family7` V2 for copies for My Family Tree 12.x; the others exist for test vectors and tooling.
pub fn encode_part(xml: &[u8], format: SourceFormat) -> Vec<u8> {
    let mut salt = [0u8; 32];
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut iv);
    match format {
        SourceFormat::Family8(v) => encode_aes(xml, PASSWORD_F8, v, &salt, &iv),
        SourceFormat::Family7(v) => encode_aes(xml, PASSWORD_F7, v, &salt, &iv),
        SourceFormat::Family5 => encode_legacy(xml, PASSWORD_F5, 10_622, false),
        SourceFormat::Family4 => encode_legacy(xml, PASSWORD_F4, 10_564, false),
        SourceFormat::Family => encode_legacy(xml, PASSWORD_F1, 1_258, true),
        SourceFormat::ContentXml => xml.to_vec(),
    }
}

/// Decrypt and decompress a `family8` blob (any of V1, V2, V3) into XML bytes (with BOM).
pub fn decode(blob: &[u8]) -> Result<Vec<u8>> {
    decode_aes(blob, PASSWORD_F8).map(|(xml, _)| xml)
}

/// Compress and encrypt XML bytes into a `family8` V3 blob, with fresh salt and IV.
pub fn encode(xml: &[u8]) -> Vec<u8> {
    encode_part(xml, SourceFormat::CURRENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &[u8] = "\u{feff}<?xml version=\"1.0\"?><Family />".as_bytes();

    #[test]
    fn roundtrip_small_xml() {
        let blob = encode(XML);
        assert_eq!(&blob[64..96], &AesVersion::V3.tag());
        assert_eq!(blob[64], 0x7C);
        assert_eq!(decode(&blob).unwrap(), XML);
    }

    #[test]
    fn tampering_is_detected() {
        let mut blob = encode(b"<Family />");
        let last = blob.len() - 1;
        blob[last] ^= 1;
        assert!(matches!(decode(&blob), Err(Error::Mac)));
    }

    #[test]
    fn every_variant_roundtrips_and_is_detected() {
        let variants = [
            SourceFormat::Family8(AesVersion::V1),
            SourceFormat::Family8(AesVersion::V2),
            SourceFormat::Family7(AesVersion::V1),
            SourceFormat::Family7(AesVersion::V2),
            SourceFormat::Family7(AesVersion::V3),
            SourceFormat::Family5,
            SourceFormat::Family4,
            SourceFormat::Family,
            SourceFormat::ContentXml,
        ];
        for format in variants {
            let blob = encode_part(XML, format);
            let (xml, detected) = decode_part(format.part_name(), &blob).unwrap();
            assert_eq!(xml, XML, "{format}");
            assert_eq!(detected, format);
        }
    }

    #[test]
    fn tags_differ_in_first_byte_only() {
        assert_eq!(AesVersion::V1.tag()[0], 0x3A);
        assert_eq!(AesVersion::V2.tag()[0], 0x5A);
        assert_eq!(AesVersion::V1.tag()[1..], AesVersion::V3.tag()[1..]);
    }

    #[test]
    fn wrong_part_password_is_rejected() {
        // family8 data presented as family7 must fail the MAC, not decode garbage.
        let blob = encode_part(XML, SourceFormat::Family8(AesVersion::V2));
        assert!(matches!(decode_part("family7", &blob), Err(Error::Mac)));
    }

    #[test]
    fn corrupt_legacy_part_is_an_error() {
        let mut blob = encode_part(XML, SourceFormat::Family5);
        blob[70] ^= 0xFF; // salt byte: wrong key
        assert!(decode_part("family5", &blob).is_err());
        assert!(decode_part("family4", &[0u8; 10]).is_err());
    }
}

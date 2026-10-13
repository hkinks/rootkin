//! Encryption/compression of the `family8` part.
//!
//! Layout: `HMAC-SHA512 (64) | version tag (32) | salt (32) | IV (16) | ciphertext`.
//! The ciphertext is AES-256-CBC/PKCS7 over raw DEFLATE of the UTF-8 XML.
//! Keys derive from a constant application password via PBKDF2-SHA512 and
//! an SP800-108 counter-mode KDF; the MAC covers everything after itself.

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::Sha512;
use std::io::{Read, Write};

use crate::{Error, Result};

type HmacSha512 = Hmac<Sha512>;

const PASSWORD: &str = "1@42%e23/;.j)Y3#{#.s5INU2V#:_Dx)w-R~_7812c448,,T%92L}Yt?{B*(=$1";
const TAG_V3: [u8; 32] = [
    0x7C, 0x42, 0x9A, 0x26, 0x62, 0x18, 0x14, 0x53, 0x38, 0x6A, 0x06, 0x7D, 0x5B, 0xB8, 0xC0, 0x2A,
    0x52, 0x92, 0xCB, 0xDB, 0x15, 0xEE, 0x91, 0x58, 0x92, 0xCE, 0x38, 0xBE, 0xFF, 0x2D, 0xF5, 0x66,
];
const ITERATIONS: u32 = 210_000;
const HEADER_LEN: usize = 64 + 32 + 32 + 16;

struct Keys {
    mac: [u8; 32],
    enc: [u8; 32],
}

fn derive_keys(salt: &[u8]) -> Keys {
    let password: Vec<u8> = PASSWORD.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut master = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(&password, salt, ITERATIONS, &mut master);
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

/// Decrypt and decompress a `family8` blob into XML bytes (with BOM).
pub fn decode(blob: &[u8]) -> Result<Vec<u8>> {
    if blob.len() < HEADER_LEN || !(blob.len() - HEADER_LEN).is_multiple_of(16) {
        return Err(Error::Format("family8 part has an invalid length".into()));
    }
    if blob[64..96] != TAG_V3 {
        return Err(Error::Format("unknown family8 version tag (only v3 is supported)".into()));
    }
    let salt = &blob[96..128];
    let iv = &blob[128..144];
    let keys = derive_keys(salt);

    let mut mac = HmacSha512::new_from_slice(&keys.mac).expect("any key length");
    mac.update(&blob[64..]);
    mac.verify_slice(&blob[..64]).map_err(|_| Error::Mac)?;

    let compressed = cbc::Decryptor::<aes::Aes256>::new(&keys.enc.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(&blob[HEADER_LEN..])
        .map_err(|_| Error::Format("bad padding".into()))?;
    let mut xml = Vec::with_capacity(compressed.len() * 5);
    flate2::read::DeflateDecoder::new(&compressed[..]).read_to_end(&mut xml)?;
    Ok(xml)
}

/// Compress and encrypt XML bytes into a `family8` blob, with fresh salt and IV.
pub fn encode(xml: &[u8]) -> Vec<u8> {
    let mut salt = [0u8; 32];
    let mut iv = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut iv);
    encode_with(xml, &salt, &iv)
}

fn encode_with(xml: &[u8], salt: &[u8; 32], iv: &[u8; 16]) -> Vec<u8> {
    let keys = derive_keys(salt);
    let mut deflater = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
    deflater.write_all(xml).expect("writing to a Vec cannot fail");
    let compressed = deflater.finish().expect("writing to a Vec cannot fail");
    let ciphertext = cbc::Encryptor::<aes::Aes256>::new(&keys.enc.into(), iv.into())
        .encrypt_padded_vec_mut::<Pkcs7>(&compressed);

    let mut body = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    body.extend_from_slice(&TAG_V3);
    body.extend_from_slice(salt);
    body.extend_from_slice(iv);
    body.extend_from_slice(&ciphertext);
    let mut mac = HmacSha512::new_from_slice(&keys.mac).expect("any key length");
    mac.update(&body);

    let mut out = mac.finalize().into_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_small_xml() {
        let xml = "\u{feff}<?xml version=\"1.0\"?><Family />".as_bytes();
        let blob = encode(xml);
        assert_eq!(&blob[64..96], &TAG_V3);
        assert_eq!(decode(&blob).unwrap(), xml);
    }

    #[test]
    fn tampering_is_detected() {
        let mut blob = encode(b"<Family />");
        let last = blob.len() - 1;
        blob[last] ^= 1;
        assert!(matches!(decode(&blob), Err(Error::Mac)));
    }
}

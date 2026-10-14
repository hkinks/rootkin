//! The `FamilyId` attribute of the root `<Family>` element: the app's tree password record.
//!
//! Format: `base64(hash name) NUL base64(iterations as decimal text) NUL base64(salt) NUL base64(hash)`,
//! e.g. `SHA512`, `210000`, 32 salt bytes, 64 hash bytes, where
//! `hash = PBKDF2(hash name, password as UTF-16LE, salt, iterations, hash length)`.
//!
//! This is only a *lock screen* check, not encryption: the tree part itself is always
//! encrypted with constant application passwords (see [`crate::codec`]). A tree without
//! a user password stores the hash of a fixed, built-in default password under a random
//! salt; the app treats any `FamilyId` that does not verify against that default as
//! "protected" and asks for the user's password on open.

use base64::Engine;
use hmac::Hmac;
use rand::RngCore;

use crate::doc::Document;

/// The built-in password whose hash marks a tree as *not* password protected.
const DEFAULT_PASSWORD: &str = "pUzFT2frmyN8VQoZp53aImOSB6sOELKVHj1HRr7saRpnfmmDgCgsFTmin0LzUs2";

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn pbkdf2(algorithm: &str, password: &[u8], salt: &[u8], iterations: u32, out: &mut [u8]) -> bool {
    let result = match algorithm {
        "SHA1" => pbkdf2::pbkdf2::<Hmac<sha1::Sha1>>(password, salt, iterations, out),
        "SHA256" => pbkdf2::pbkdf2::<Hmac<sha2::Sha256>>(password, salt, iterations, out),
        "SHA384" => pbkdf2::pbkdf2::<Hmac<sha2::Sha384>>(password, salt, iterations, out),
        "SHA512" => pbkdf2::pbkdf2::<Hmac<sha2::Sha512>>(password, salt, iterations, out),
        _ => return false,
    };
    result.is_ok()
}

/// Hash `password` into a `FamilyId` record (SHA512, 210000 iterations, random salt).
pub fn hash_password(password: &str) -> String {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    hash_password_with_salt(password, &salt)
}

fn hash_password_with_salt(password: &str, salt: &[u8; 32]) -> String {
    let mut hash = [0u8; 64];
    assert!(pbkdf2("SHA512", &utf16le(password), salt, 210_000, &mut hash));
    [b64(b"SHA512"), b64(b"210000"), b64(salt), b64(&hash)].join("\0")
}

/// The `FamilyId` of a tree without a user password (what the app writes for a new tree).
pub fn default_family_id() -> String {
    hash_password(DEFAULT_PASSWORD)
}

/// The `FamilyId` of an unprotected tree in the notation of app versions before schema 14.7 (My Family
/// Tree 12.x): `base64(salt 32) + base64(PBKDF2-SHA256(default password, salt, 10000, 32))`, 88 characters.
pub fn legacy_default_family_id() -> String {
    let mut salt = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut salt);
    let mut hash = [0u8; 32];
    assert!(pbkdf2("SHA256", &utf16le(DEFAULT_PASSWORD), &salt, 10_000, &mut hash));
    format!("{}{}", b64(&salt), b64(&hash))
}

/// Does `password` match the `FamilyId` record?
pub fn verify_password(password: &str, family_id: &str) -> bool {
    let parts: Vec<&str> = family_id.split('\0').collect();
    let [algorithm, iterations, salt, hash] = parts[..] else { return false };
    let engine = &base64::engine::general_purpose::STANDARD;
    let (Ok(algorithm), Ok(iterations), Ok(salt), Ok(hash)) =
        (engine.decode(algorithm), engine.decode(iterations), engine.decode(salt), engine.decode(hash))
    else {
        return false;
    };
    let (Ok(algorithm), Ok(iterations)) = (String::from_utf8(algorithm), String::from_utf8(iterations)) else {
        return false;
    };
    let Ok(iterations) = iterations.parse::<u32>() else { return false };
    let mut derived = vec![0u8; hash.len()];
    pbkdf2(&algorithm, &utf16le(password), &salt, iterations, &mut derived) && derived == hash
}

/// True if the tree is password protected, i.e. the app would ask for a password.
pub fn is_protected(family_id: Option<&str>) -> bool {
    !family_id.is_some_and(|id| verify_password(DEFAULT_PASSWORD, id))
}

/// Is the document's `FamilyId` something other than the default (user-protected tree)?
pub fn document_is_protected(doc: &Document) -> bool {
    is_protected(doc.root.attr("FamilyId").as_deref())
}

/// The app's upgrade of a `FamilyId` written by old versions: a bare
/// `base64(salt 32) + base64(hash 32)` (88 characters), meaning PBKDF2-SHA256 with
/// 10000 iterations. Returns the replacement value, or `None` when `family_id` is
/// already in a form the current app accepts.
///
/// Missing or differently shaped values (and legacy hashes of the default password)
/// become a fresh [`default_family_id`], like the app does. Hashes of a real user
/// password are carried over to the new notation.
pub fn upgrade_legacy_family_id(family_id: Option<&str>) -> Option<String> {
    match family_id {
        Some(id) if id.contains('\0') => None,
        Some(id) if id.len() == 88 && id.is_ascii() => {
            let upgraded = [b64(b"SHA256"), b64(b"10000"), id[..44].to_string(), id[44..].to_string()].join("\0");
            Some(if verify_password(DEFAULT_PASSWORD, &upgraded) { default_family_id() } else { upgraded })
        }
        _ => Some(default_family_id()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_id_has_documented_shape_and_verifies() {
        let id = default_family_id();
        let parts: Vec<&str> = id.split('\0').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], b64(b"SHA512"));
        assert_eq!(parts[1], b64(b"210000"));
        assert!(verify_password(DEFAULT_PASSWORD, &id));
        assert!(!is_protected(Some(&id)));
    }

    #[test]
    fn user_password_marks_tree_protected() {
        let id = hash_password("hunter2");
        assert!(verify_password("hunter2", &id));
        assert!(!verify_password("hunter3", &id));
        assert!(is_protected(Some(&id)));
        assert!(is_protected(None));
        assert!(is_protected(Some("not a record")));
    }

    #[test]
    fn random_hash_would_look_protected() {
        // What new_family_document used to write: right shape, random hash.
        let id = [b64(b"SHA512"), b64(b"210000"), b64(&[1u8; 32]), b64(&[2u8; 64])].join("\0");
        assert!(is_protected(Some(&id)));
    }

    #[test]
    fn legacy_ids_are_upgraded_like_the_app() {
        // Missing / wrong length -> default.
        let default = upgrade_legacy_family_id(None).unwrap();
        assert!(!is_protected(Some(&default)));
        assert!(!is_protected(upgrade_legacy_family_id(Some("short")).as_deref()));
        // Current-format ids are left alone.
        assert_eq!(upgrade_legacy_family_id(Some(&default)), None);

        // Legacy 88-char record for a user password: SHA256 / 10000 / 32-byte hash.
        let salt = [7u8; 32];
        let mut hash = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(&utf16le("secret"), &salt, 10_000, &mut hash);
        let legacy = format!("{}{}", b64(&salt), b64(&hash));
        assert_eq!(legacy.len(), 88);
        let upgraded = upgrade_legacy_family_id(Some(&legacy)).unwrap();
        assert!(verify_password("secret", &upgraded));
        assert!(is_protected(Some(&upgraded)));

        // Legacy record for the default password -> replaced by a fresh default.
        pbkdf2::pbkdf2_hmac::<sha2::Sha256>(&utf16le(DEFAULT_PASSWORD), &salt, 10_000, &mut hash);
        let legacy_default = format!("{}{}", b64(&salt), b64(&hash));
        let upgraded = upgrade_legacy_family_id(Some(&legacy_default)).unwrap();
        assert!(!is_protected(Some(&upgraded)));
        assert_ne!(upgraded.split('\0').nth(2), Some(b64(&salt).as_str()));
    }

    #[test]
    fn legacy_default_id_is_recognised_as_unprotected() {
        let legacy = legacy_default_family_id();
        assert_eq!(legacy.len(), 88);
        assert!(!legacy.contains('\0'));
        // the app's (and this crate's) upgrade treats it as the default password
        assert!(!is_protected(upgrade_legacy_family_id(Some(&legacy)).as_deref()));
    }
}

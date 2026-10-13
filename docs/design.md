# Rootkin — design

Goal: a pure-Rust (egui/eframe) Linux replacement for Chronoplex My Family Tree
that opens, edits and saves `.familyx` files which the Windows app can still open.

## family8 format (v3, reverse-engineered from MyFamilyTree 16.2)

`HMAC-SHA512(64) | version tag(32) | salt(32) | IV(16) | AES-256-CBC/PKCS7(raw DEFLATE(UTF-8 XML))`

- password: constant in the app (see `crates/familyx/src/codec.rs`), UTF-16LE
- `k = PBKDF2-HMAC-SHA512(password, salt, 210000, 32)`
- SP800-108 counter KDF: `HMAC-SHA512(k, 00000001 | "ChronoplexSoftware" 00 "xml|<suffix>" | 00000100)[:32]`,
  suffix `mac_key` / `encryption_key`
- MAC covers bytes 64..end. Fresh random salt + IV on every save.
- XML is .NET `XmlSerializer` output of `DataCollection`, root `<Family>`, with a BOM
  and `&#x0;` inside `FamilyId` (strict parsers reject it).

## Tree part variants (read-only import of older files)

The tree lives in one zip entry; its name tells the era. `Package::open` picks the newest
present (`family8`, `family7`, `family5`, `family4`, `family`, then plain `content.xml`),
exposes `Package::source_format()` and **always saves as `family8` V3**: the old entry is
dropped and `[Content_Types].xml` / `_rels/.rels` are retargeted to `/family8`. Other tree
entries that were not read are kept verbatim.

| entry | container | password | KDF |
|-------|-----------|----------|-----|
| `family8` | AES container (below) | app constant A, UTF-16LE | tag-dependent |
| `family7` | AES container | app constant B, UTF-16LE | tag-dependent |
| `family5` | legacy: `32 junk \| salt(32) \| ct` | constant, UTF-8 | PBKDF2-SHA1, 10622 iterations |
| `family4` | legacy, same layout | constant, UTF-8 | PBKDF2-SHA1, 10564 iterations |
| `family`  | legacy, no header: `ct` | GUID constant, UTF-8, which is also the salt | PBKDF2-SHA1, 1258 iterations |
| `content.xml` | plain XML | - | - |

Legacy containers: `key(32) \| iv(16) = PBKDF2-HMAC-SHA1(password, salt, n, 48)`;
AES-256-CBC/PKCS7 over raw DEFLATE of the XML; no MAC (a wrong key shows up as bad padding
or a failed inflate). The constants are in `codec.rs`.

AES container versions, selected by the first byte of the 32-byte tag (bytes 1..32 are
identical in all versions); the KDF/MAC part is always SP800-108 counter mode with
HMAC-SHA512, label `ChronoplexSoftware`, context `xml|mac_key` / `xml|encryption_key`:

| tag[0] | name | PBKDF2 | iterations | master key length |
|--------|------|--------|------------|-------------------|
| `0x3A` | V1 | HMAC-SHA256 | 10000 | **256 bytes** |
| `0x5A` | V2 | HMAC-SHA256 | 10000 | 32 bytes |
| `0x7C` | V3 | HMAC-SHA512 | 210000 | 32 bytes |

(The app's decrypt path picks the version from the tag, so both `family8` and `family7`
may carry any version.)

**How this was verified.** No real old files were available. The parameters above were read
from the app's `EncryptionOptionsV1/V2/V3` and `XmlEncryption`. Test vectors
(`crates/familyx/tests/data/old/`, the fictional example tree) were produced with a separate
Python implementation of the documented algorithms, and every one of them was fed to the
app's own decrypt code (loaded by reflection): all passed the MAC/padding/inflate stages and
reached XML deserialization, while wrong-key controls failed earlier. The vectors are also
checked by the Rust tests. What is *not* verified against real files: the exact byte layout of
genuine old files (e.g. what the 32 leading bytes of `family5`/`family4` are for), and the
packaging metadata of old versions.

**Not done: schema migration.** Old versions serialized an older schema. The app reads it
into the same `DataCollection` class and then converts (`FileConversions`): per-person
`Relationships` become `ParentChildLink`/`SpouseLink`, `Name`/`AlternativeName` become
name parts, `Photos`/`Attachments`/`AudioVideo` and root-level `PhotoCollection` etc. become
`PackagedFile`s, `Story` becomes notes, `ContactAddress`/location fields become place
details. This crate decrypts such files and keeps the XML verbatim, but does not convert it,
so those trees can appear empty or partial (`Package::has_legacy_schema()` flags this).
Implementing the conversion needs real samples. `FamilyId` is upgraded on import (below).

## FamilyId (tree password record)

`<Family FamilyId="...">` is the app's password hash, `crate::protection`:

`base64("SHA512") NUL base64("210000") NUL base64(salt, 32) NUL base64(hash, 64)`

with `hash = PBKDF2-HMAC-SHA512(password UTF-16LE, salt, 210000, 64)`. (In the XML the NULs
appear as `&#x0;`.) It is validated when a file is opened: if the stored record does **not**
verify against a built-in default password, the app considers the tree *protected* and asks
the user for a password (`ProtectionHandler.IsProtected`, set from `FamilyId`). Findings:

- A tree without a user password stores the hash of the built-in default password (see
  `protection.rs`) under a random salt - confirmed by verifying the `FamilyId` of a real file
  against it. The app writes this for every new tree.
- A user password simply replaces the record by `hash(user password)`. The password is a
  UI lock only: no key is derived from it, the tree part and photos are not re-encrypted.
  Since the tree part is encrypted with constants, Rootkin can read protected trees; it
  preserves `FamilyId` on save, and `Package::is_password_protected()` tells the GUI to warn.
- `new_family_document` must therefore write `default_family_id()`. A random hash, as it
  used to, makes the Windows app prompt for a password nobody knows.
- Legacy records (old versions): 88 characters `base64(salt) base64(hash)` meaning
  SHA256 / 10000 / 32 bytes. On conversion the app rewrites them to the 4-part notation, or to
  a fresh default record if they hash the default password or have any other shape.
  `Package::open` does the same for any non-current tree part.

## Data model

- `ParentChildLink` (Person1 = parent, Person2 = child), one per parent.
- `SpouseLink` = couple/"family"; each spouse has a `FamilySpouseLink` back-reference.
- Child has `FamilyChildLink` → parents' `SpouseLink` when both parents are a couple.
- Media: `PackagedFile` (`RelativePath` `Multimedia\x.png`), person `PrimaryImageId`.

## Architecture

- `crates/familyx` — `codec` (crypto, all tree variants), `protection` (`FamilyId`), `doc` (lossless XML tree, byte-exact round-trip),
  `package` (OPC zip, atomic save + `.bak`), `model` (read-only `Tree` snapshot rebuilt
  after each edit), edits mutate the `doc` directly so unknown data survives.
- `crates/rootkin-cli` — `dump`, `pack`, `roundtrip`, `summary`, `check`, `example`.
- `crates/rootkin` — egui app: people list, tree canvas, person panel.

New elements must follow XmlSerializer child order (see `ensure_child`).

## Dev

`nix-shell` provides the toolchain and runtime libs (vulkan, wayland, GL).
The decoded XML contains personal data: never commit `*.xml` dumps.

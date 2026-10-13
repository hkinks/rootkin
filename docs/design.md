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

## Data model

- `ParentChildLink` (Person1 = parent, Person2 = child), one per parent.
- `SpouseLink` = couple/"family"; each spouse has a `FamilySpouseLink` back-reference.
- Child has `FamilyChildLink` → parents' `SpouseLink` when both parents are a couple.
- Media: `PackagedFile` (`RelativePath` `Multimedia\x.png`), person `PrimaryImageId`.

## Architecture

- `crates/familyx` — `codec` (crypto), `doc` (lossless XML tree, byte-exact round-trip),
  `package` (OPC zip, atomic save + `.bak`), `model` (read-only `Tree` snapshot rebuilt
  after each edit), edits mutate the `doc` directly so unknown data survives.
- `crates/rootkin-cli` — `dump`, `pack`, `roundtrip`, `summary`, `check`, `example`.
- `crates/rootkin` — egui app: people list, tree canvas, person panel.

New elements must follow XmlSerializer child order (see `ensure_child`).

## Dev

`nix-shell` provides the toolchain and runtime libs (vulkan, wayland, GL).
The decoded XML contains personal data: never commit `*.xml` dumps.

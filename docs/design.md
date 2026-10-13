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

- Dates are the app's `CustomDate` in `Fact/FactDate`: `Start*`, `End*`, `DateDescriptor`
  (`Exact` default/omitted, `About`, `Estimated`, `Calculated`, `Before`, `After`, `Range`,
  `Span`, `FromOnly`, `ToOnly`, `EitherOr`, `Interpreted`, `TextOnly`) and `Text`.
  `FactDatePeriod/DateDescriptor` is always `Span` in app-written files and is not the qualifier.
- Enum values the app accepts (writing anything else may break loading):
  `ParentChildModifier` Natural|Adopted|Foster|Step|Guardian|Sealing|Custom;
  `NameType` Adopted|Aka|Birth|FormerSurname|Immigrant|Maiden|Married|Nick|Professional|Religious|Custom;
  `SpouseRelationshipStatus` Engaged|Current|Separated|DivorceFiled|Deceased|Former|Divorced|Annulled|Dissolved;
  `Gender` Male|Female|Other|Unknown|NotYetKnown. Fact `Tag` is a free string (GEDCOM tags).

- `ParentChildLink` (Person1 = parent, Person2 = child), one per parent.
- `SpouseLink` = couple/"family"; each spouse has a `FamilySpouseLink` back-reference.
- Child has `FamilyChildLink` → parents' `SpouseLink` when both parents are a couple.
- Media: `PackagedFile` (`RelativePath` `Multimedia\x.png`), person `PrimaryImageId`.

## Architecture

- `crates/familyx` — `codec` (crypto), `doc` (lossless XML tree, byte-exact round-trip),
  `package` (OPC zip, atomic save + `.bak`), `model` (read-only `Tree` snapshot rebuilt
  after each edit), edits mutate the `doc` directly so unknown data survives.
- `crates/rootkin-cli` — `dump`, `pack`, `roundtrip`, `summary`, `check`, `example`, `import`, `export`.
- `crates/rootkin-formats` — GEDCOM and Gramps XML import/export (see below).
- `crates/rootkin` — egui app: people list, tree canvas, person panel.

New elements must follow XmlSerializer child order (see `ensure_child`).

## Import/export formats (`rootkin-formats`)

API: `import(bytes, Format) -> (Package, ImportReport)`, `export(&Package, Format) -> Vec<u8>`,
plus `import_path` / `export_path`; `Format::{Gedcom, Gramps}` detected by content, then extension.
Imports only use the `familyx` edit API (so the result is a valid `.familyx`); exports read the
`Tree` snapshot. `ImportReport.warnings` lists every dropped or approximated item (`dropped 2x INDI.SOUR`).

Mapping (GEDCOM 5.5.1 UTF-8 / Gramps XML 1.7.1):

- Names: primary + alternates (`NAME` with `TYPE` / `<name alt="1" type>`); prefix, suffix, nickname
  are written but dropped on import (the edit API has no setter yet; prefix is folded into the surname).
- Dates map onto the app's `DateDescriptor` (ABT/EST/CAL, BEF/AFT, BET..AND = Range, FROM..TO = Span,
  FROM/TO only, free text = TextOnly). Non-Gregorian calendars become text. Gramps has no open-ended
  spans: those are written as `datestr`.
- Attribute-like facts (`OCCU`, `EDUC`, `RESI`, ...) keep their text as the GEDCOM value; events keep
  it as `NOTE`. Tags GEDCOM does not know are written as `EVEN` + `TYPE`. In Gramps, details are the
  event description and unknown types are custom event types (read back as `EVEN`).
- Families: one `FAM` / `<family>` per couple, HUSB/WIFE (father/mother) by gender, plus synthetic
  families for single parents and unmarried pairs so every child is attached. `PEDI adopted/foster/sealing`
  and Gramps `frel`/`mrel` map to the parent-link modifier (GEDCOM: per family, not per parent).
- Places are flat strings: written as one `PLAC` / one flat `placeobj`; a Gramps hierarchy is joined
  with commas on import.
- Living flag: neither format has it; import guesses (no death data and born < 110 years ago).
- Not carried: photos/media, sources and citations, person notes, addresses, couple status, event
  roles other than primary, Gramps tags/attributes/repositories, `_CUSTOM` tags, ANSEL encoding.
  Unsupported couple facts without two spouses are dropped (reported).

## Dev

`nix-shell` provides the toolchain and runtime libs (vulkan, wayland, GL).
The decoded XML contains personal data: never commit `*.xml` dumps.

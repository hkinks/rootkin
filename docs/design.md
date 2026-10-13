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

## Schema migration (older element layouts)

Older app versions serialized an older schema. The app reads it into the current `DataCollection` classes
(the old members live on as `[Obsolete]` properties with their old XML names, never written back) and then runs
`FileConversions`: a chain of ~45 conversions, each gated on `FileProperties/Version` (a schema number; 14.8 is
current). `crate::migrate` performs the structural part of that chain on the XML when a tree is opened
(`Package::from_bytes`; `Package::migration()` returns the `Report`) and sets `Version` to 14.8, because the
app skips every conversion below the stored version - leaving the old number would make it run them again
over converted data (e.g. overwrite `StartYear` from the missing `StartDate`). Trees already at 14.8 are not
touched; unversioned trees are converted only if they contain old elements.

**How this was derived and checked.** Read statically from the decompiled classes (`Person`, `Relationship` and
subclasses, `CustomDate`, `Fact`, `Photo`, `Story`, `DataCollection`, ...) and `FileConversions`; the old element
names are the `XmlElement`/`XmlArray` names on the `Obsolete*` properties. Running the vendor's conversion code
on synthetic input was tried and failed: it needs the WPF assemblies, whose R2R/native parts do not load on
Linux. So the fixtures (`tests/data/old/legacy_rel.xml`, `legacy_old.xml`, fictional) are hand-written after the
classes and the expected results come from reading the conversion code, not from a differential run. Strings in
the vendor assembly are encrypted, so enum/tag literals the conversions use were not all recoverable.

Old layout (all per `Person` unless noted):

| old element | meaning |
|-------------|---------|
| `Relationships/Relationship xsi:type=Spouse\|Child\|Parent\|SiblingRelationship` | per-person, mirrored list. `PersonId` = the relative, `RelationshipType`; `Child` = "PersonId is my child", `Parent` = "PersonId is my parent". Spouse: `SpouseRelationshipStatus` (or old `SpouseModifier`), `Marriage`, `Divorce` (`Fact`), `Facts`, `NumberOfChildren`, `Notes`, `Multimedia`; Child/Parent: `ParentChildModifier` (string) |
| `Name`, `AlternativeName` | one `Name` each instead of `PersonNameCollection/Names` |
| `Photos`, `Attachments`, `AudioVideo` (and root `PhotoCollection`, `AttachmentCollection`, `AudioVideoCollection`, `PackagedFilePropertiesCollection`) | files with `RelativePath`, `IsAvatar`; properties by file id |
| `Story` | `RelativePath` of a story file, `GedcomImportStoryText`, or database ids (`FileId`, `AnnotationsFileId`) |
| `ContactAddress` | `Address` with `ContactName/Line1-3/City/State/PostalCode/Country` |
| `LDSFacts` | `Fact` subclasses with temple codes |
| `Fact`: `Place`, `FactType`, `Address`, `Witnesses`, `Citation`, `Note` (string) | |
| `CustomDate`: `StartDate`, `EndDate` (DateTime), `Descriptor`, `IsTextDate`, `IsDateRange`, `IsStartBC`, `IsEndBC`, `CalendarType` | |
| root `Author` | single author |
| 36-character GUID ids | before 2.1 |

Mapping (what `migrate` does; version gate of the app's conversion in brackets):

- **Relationships -> links** [< 11.3]: the lists are made symmetric first (a Spouse entry without mirror gets
  one, `Child` without `Parent` on the other side and vice versa). Then one `SpouseLink` per unordered pair
  (person 1 = the person listed first in file order, status from rel 1, `Facts` = its `Facts` + `Marriage` +
  `Divorce` [< 2.4, each with `IsFamily`; empty placeholders skipped] + `NumberOfChildren` as an `NCHI` fact,
  `Notes`/`Multimedia` of both sides without duplicates) and one `ParentChildLink` per `Child` entry (parent =
  owner, `ParentChildModifier` from the `Child` entry, Natural omitted), appended spouses first, in person order.
  The `FamilySpouseLinks`/`FamilyChildLinks` back-references come from the editor's own `link_spouse`/`link_parent`.
  `Sibling` entries are dropped (derived data). Entries whose relative does not exist, and parent entries with a
  modifier the current app does not have (the app turns old godparent/sponsor/witness roles into *associations*,
  not implemented) stay in a trimmed `Relationships` list and are reported as warnings.
- **Names** [< 1.4]: `Name` becomes the primary entry of `PersonNameCollection/Names`, `AlternativeName` a
  non-primary one; `Note` strings become `Notes` [string note conversion].
- **Media** [< 5.0]: every distinct `RelativePath` (root collections first, then per person) becomes a
  `PackagedFile` with a fresh id, `Type` Photo/Attachment/AudioVideo, `MultimediaType` from the old `MediaType`
  string, properties from `PackagedFilePropertiesCollection` by old id; person `Multimedia` gets the ids,
  `IsAvatar` sets `PrimaryImageId`. `RelativePath` is resolved against the package parts (as is, under
  `Multimedia/`, by file name).
- **Story**: `GedcomImportStoryText` and a story file found in the package become story notes
  (`IsStoryNote` true; `text/richtext` for `.rtf`). Database-backed stories (`FileId`) are not converted and stay.
- **Facts**: `Place` -> `PlaceDetails/Description`; `ContactAddress` -> a `FACT_ContactAddress` fact with the
  address lines (newline separated) as place.
- **Dates**: `StartDate`/`EndDate` -> `Start*`/`End*` (time of day -> `StartTime`/`EndTime`); `IsTextDate` ->
  `TextOnly`, `IsDateRange` -> `Range` (`FromOnly`/`ToOnly` if one end is missing), else `Descriptor` parsed
  case-insensitively; BC flags -> `StartEra`/`EndEra` 0; non-Gregorian `CalendarType` -> `StartCalendar`/`EndCalendar`.
  `0001-01-01` (DateTime.MinValue) counts as unset.
- **Ids** [< 2.1]: 36-character GUIDs become 22-character base64url of `Guid.ToByteArray()`, everywhere they are
  referenced.
- **Author**: root `Author` moves into `AuthorCollection`.

Guesses that could not be confirmed against the vendor code (encrypted literals): the `NCHI` tag and
`NumberDetails` element of the number-of-children fact, element names `StartEra`/`StartCalendar` (and their
position), and `text/richtext` for `.rtf` stories.

**Not migrated (kept verbatim, `Package::has_legacy_schema()` stays true):** `LDSFacts` (LDS fact types ->
tags/temples), database-backed stories, godparent/sponsor/witness roles, obsolete `Biology` fields (hair/eye
colour, blood group, haplogroups -> facts), `FactType`, fact `Address`/`Witnesses`/`Citation`, name `Citation`,
`FamilyProperties/FamilyCrest` and `Description`, address/repository/source/task obsolete members, per-file
`Tags`, calendar month adjustments (French/Icelandic), `AncestralFileNumber`/`RecordFileNumber`/`CustomId`.
Because `Version` is bumped, the Windows app will not convert these either once the file was saved by Rootkin.
`FamilyId` is upgraded on import (below).

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

- `crates/familyx` — `codec` (crypto, all tree variants), `protection` (`FamilyId`), `doc` (lossless XML tree, byte-exact round-trip),
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

## Photos in GEDCOM / Gramps

Exports copy each photo into `<export name>_media/` and reference it relatively
(GEDCOM `0 @O1@ OBJE` / `1 FILE`, Gramps `<objects><object><file src>`); the
primary photo is referenced first. Imports resolve references relative to the
imported file, then as given, then by bare file name next to it; PNG and JPEG
are attached, other types are reported as dropped.

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
current; a conversion runs for files *older* than its gate, in ascending order of the gates). `crate::migrate`
performs the structural part of that chain on the XML when a tree is opened
(`Package::from_bytes`; `Package::migration()` returns the `Report`) and sets `Version` to 14.8, because the
app skips every conversion below the stored version - leaving the old number would make it run them again
over converted data (e.g. overwrite `StartYear` from the missing `StartDate`). Trees already at 14.8 are not
touched; unversioned trees are converted only if they contain old elements (`migrate::has_legacy_elements`).
Unlike the app, `migrate` does not gate each conversion on the version: an old element is converted wherever
it is found (a few conversions depend on the version, see below).

**How this was derived and checked.** Read from the decompiled classes (`Person`, `Relationship` and
subclasses, `CustomDate`, `Fact`, `FactWitness`, `Association`, `Citation`, `Biology`, `LDS*`, `Story`,
`DataCollection`, ...) and `FileConversions`; the old element names are the `XmlElement`/`XmlArray` names on the
`Obsolete*` properties. The string literals of the vendor assembly are encrypted; they were read by calling the
assembly's own decryptor. The conversion chain itself can be run on Linux with a small throw-away harness
(not in the repository): load the assembly by reflection, stub the WPF assemblies, deserialize a hand-written
old-schema XML with the app's `XmlSerializer`, call the (private) conversion methods in the order the entry
point runs them, serialize the result. That differential run was done for every conversion listed below as
"checked"; the result is what the tests in `tests/migrate.rs` assert. Not runnable: anything that needs the
app's data folders or WPF imaging (family crest, story *files*, photos). The story database step runs with
the real LiteDB 4.1.4 standing in for the private `LiteDB4` (see "Story"). Those parts are *verified only by
reading the code*, as marked. The fixtures
(`tests/data/old/legacy_rel.xml`, `legacy_old.xml`, fictional) are hand-written; they mix constructs of several
schema versions to cover the cases, which the app's own chain would not do (it only sees what its version could
contain), so they are not a differential test of the whole chain. Real old files would still confirm all of it.

Old layout (all per `Person` unless noted):

| old element | meaning |
|-------------|---------|
| `Relationships/Relationship xsi:type=Spouse\|Child\|Parent\|SiblingRelationship` | per-person, mirrored list. `PersonId` = the relative, `RelationshipType`; `Child` = "PersonId is my child", `Parent` = "PersonId is my parent". Spouse: `SpouseRelationshipStatus` (or old `SpouseModifier`), `Marriage`, `Divorce` (`Fact`), `Facts`, `LDSFacts`, `NumberOfChildren`, `Notes`, `Multimedia`; Child/Parent: `ParentChildModifier` (string) |
| `Name`, `AlternativeName` | one `Name` each instead of `PersonNameCollection/Names`; a single `Citation` on a name |
| `Photos`, `Attachments`, `AudioVideo` (and root `PhotoCollection`, `AttachmentCollection`, `AudioVideoCollection`, `PackagedFilePropertiesCollection`) | files with `RelativePath`, `IsAvatar`; properties by file id |
| `Story` | `RelativePath` of a story file, `GedcomImportStoryText`, or story database ids (`FileId`, `AnnotationsFileId`) |
| `ContactAddress` | `Address` with `ContactName/Line1-3/City/State/PostalCode/Country` |
| `LDSFacts` | `Fact` subclasses (`xsi:type` LDSBaptismFact, LDSEndowmentFact, LDSChildSealingFact, LDSSpouseSealingFact) with `TempleCode` and a per-type `...DateStatus` |
| `Biology`: `HairColor`, `LeftEyeColor`, `RightEyeColor`, `BloodGroup`, `HaplogroupY`, `HaplogroupM` | strings |
| `Fact`: `Place`, `Note` (strings), `FactType` (`Individual`\|`Family`), `Address` (string) / `Addresses/Addresses/Address` (list) / `FullAddress` with address lines, `Witnesses/FactWitness` (`PersonId`, `PersonName`, `Type`, `CustomType`, `Notes`, `WitnessCitationCollection`), `Citation` | |
| `Citation`: `Event`, `CustomEvent`, `RoleInEvent`, `Text` | strings |
| `CustomDate`: `StartDate`, `EndDate` (DateTime), `Descriptor`, `StartDateDescriptor`, `IsTextDate`, `IsDateRange`, `IsStartBC`, `IsEndBC`, `CalendarType` | |
| root `Author` | single author |
| `FamilyProperties/FamilyCrest` | a `Photo` |
| 36-character GUID ids | before 2.1 |

Mapping (what `migrate` does; version gate of the app's conversion in brackets):

- **Relationships -> links** [< 11.3]: the lists are made symmetric first (a Spouse entry without mirror gets
  one, `Child` without `Parent` on the other side and vice versa, copying the entry). Then one `SpouseLink`
  per unordered pair (person 1 = the person listed first in file order, status from rel 1, `Facts` = its
  `Facts` + `Marriage` + `Divorce` [< 2.4, each with `IsFamily`; empty placeholders skipped] + `NumberOfChildren`
  as an `NCHI` fact, + the `LDSFacts` of both sides, `Notes`/`Multimedia` of both sides without duplicates) and
  one `ParentChildLink` per `Child` entry (parent = owner). The link's modifier is the one on the *child's*
  `Parent` entry (checked: the parent's `Child` entry is only its mirror and counts when the `Parent` entry is
  missing); the text is matched case-insensitively and an unknown text counts as natural. The
  `FamilySpouseLinks`/`FamilyChildLinks` back-references come from the editor's own `link_spouse`/`link_parent`.
  `Sibling` entries are dropped (derived data), and so are entries whose relative does not exist
  (`Report::dropped`), as in the app.
- **Godparent, sponsor, witness -> associations** [< 5.1] (checked): a `Child` entry (after the lists were made
  symmetric) whose `ParentChildModifier` is exactly `GodParent`, `Sponsor` or `Witness` is no link; instead the
  child's birth fact (the primary `BIRT`, a new one if there is none) gets an `Association` with the owner as
  `PersonId` and that `Role` (once per person and role). Any other text, e.g. `Godparent`, is an unknown
  modifier and so a natural parent link.
- **Names** [< 1.4]: `Name` becomes the primary entry of `PersonNameCollection/Names`, `AlternativeName` a
  non-primary one; `Note` strings become `Notes` [string note conversion]; a single `Citation` becomes the name's
  `NameCitationCollection` (checked).
- **Media** [< 5.0]: every distinct `RelativePath` (root collections first, then per person) becomes a
  `PackagedFile` with a fresh id, `Type` Photo/Attachment/AudioVideo, `MultimediaType` from the old `MediaType`
  string, properties from `PackagedFilePropertiesCollection` by old id; person `Multimedia` gets the ids,
  `IsAvatar` sets `PrimaryImageId`. `RelativePath` is resolved against the package parts (as is, under
  `Multimedia/`, by file name). (Reading the code only.)
- **Family crest** [< 12.0] (reading the code only): the `FamilyCrest` photo becomes a `PackagedFile` (a crest
  whose file is not in the package is dropped, as in the app) and is attached to the first person of every
  separate tree (people connected by couple or parent links): `FamilyCrestId` and `Multimedia`.
- **Story**: `GedcomImportStoryText` becomes a plain story note, a story file found in the package a
  `text/richtext` story note (the app reads every story file as rich text; its `... - annotations.xml` is not
  converted). Both have `IsStoryNote` true. (Reading the code only.)
  **Database-backed stories** [`Story/FileId`, `AnnotationsFileId`; versions 7.0 to < 14.2] (the conversion step
  `_0092_0087_0087`, and the string literals via the vendor's decryptor): the texts are in a LiteDB v4
  `FileStorage` database, the package part `stories7` (the app extracts the package to a folder and opens the
  file `<folder>/stories7`; the part name is the literal `stories7`). `crate::litedb` reads it (see below) and
  `crate::migrate::stories` maps it:
  - the connection string is `Filename=<file>; Password=<p>; EncryptionVersion=<n>`; `(p, n)` depends on the
    file `Version`: 7.0 -> (A, 1); 7.1 to 7.6 -> (A, 2); 7.7 and later -> (B, 2), with A
    `5EuBD8Bfmx53Re9oyiEYvJO7AQF9dLLMbON2ZBMICzQeaRKAbUNE1JrUi22X518` and B
    `gkBrhnC6uSzoU2KMOL4UeNfNpeIqSSRkEn0PliJJ5NJP3IJViZE9o1eAQw6dbQI` (checked by running the app's chain with
    stock LiteDB 4.1.4 behind the `LiteDB4` name: the connection strings it builds, and a database made with A
    is refused with B). Rootkin does not rely on the version: the header of the database stores `SHA1(password)`,
    so the matching one of the two is used;
  - `FileId` / `AnnotationsFileId` are compared with the `_id` of the `_files` records (strings, here GUIDs).
    The app only looks at non-empty files, ordered by length descending and de-duplicated by `filename`
    (so of several stories with the same file name only the longest would survive; Rootkin does not de-duplicate
    by name, an id is an id). The file content is read as UTF-8 text into a `Note` with `Format` `text/richtext`
    and `IsStoryNote` true, added to the person's notes like a story file; an empty file makes no note;
  - the annotations file is a WPF annotation store (`XmlStreamStore`, base64 XAML per annotation) which the app
    converts to further rich-text notes. **Not converted** here (no XAML reader): the annotations are dropped and
    `Report::warnings` says so;
  - cleanup: the app deletes the extracted `stories7` after converting. Rootkin drops the part (with its
    content-type override and relationships, `Report::consumed_parts`) when every story id was resolved, so
    `has_legacy_schema()` is false afterwards and saving writes no `stories7`. If an id is missing from the
    database, or the database is absent / unreadable (wrong password, corrupt), the `Story` element keeps that
    id, the part stays and a warning says why (`has_legacy_schema()` stays true).
  Not verified: real databases of the app. The app's `LiteDB4` is a private build (its `FindAll` takes a flag, the
  connection string has `EncryptionVersion`), which is not LiteDB 4.1.4 itself; what `EncryptionVersion` 1 / 2
  change is unknown. `crate::litedb` implements the stock LiteDB 4.1.4 encryption (below); if the private build
  derives its key differently, opening fails with a warning and the story stays as it was.
- **Facts**: `Place` -> `PlaceDetails/Description` (the app does not convert this one: a `Place` string is lost
  there; it is kept here); `ContactAddress` -> a `FACT_ContactAddress` fact with the address lines
  (newline separated) as place (the app loses it: its line-joining step does not reach the contact address);
  `FactType` `Family` -> `IsFamily` for files from version 7.0 (checked; before 7.0 the app ignores the element);
  `Address` (string), the first of `Addresses`, and the lines of a `FullAddress` (`ContactName, Line1-3, City,
  State, PostalCode, Country`, one per line, blanks skipped) -> `FullAddress/Details`; a single `Citation` ->
  `FactCitationCollection` [< 1.4]; `Witnesses` -> `Associations` of the same fact [< 12.5] (checked): `Type`
  (case-insensitive `Role` enum name; unknown text `Custom`; none `Witness`; a `Role` element wins if there is no
  `Type`) is the `Role`, `CustomType` the `CustomRole`, `PersonId`/`PersonName`, notes and
  `WitnessCitationCollection` (-> `AssociationCitationCollection`) are carried over.
- **Citations** (checked): `Event` -> `EventType` (case-insensitive GEDCOM-tag enum, unknown dropped),
  `CustomEvent` -> `CustomEventType`, `RoleInEvent` -> `Role` (`CitationRoleFATH/MOTH/HUSB/WIFE/CHIL/SPOU` ->
  Father/Mother/Husband/Wife/Child/SpousePartner, `Custom`, anything else blank), `Text` -> `ActualTexts`.
- **LDS facts** [< 10.0] (checked): every entry of `LDSFacts` becomes a `Fact` appended to the person's facts
  with `ReligiousAffiliation` "LDS Church", the `Tag` (CONL, ENDL, SLGC, BAPL, SLGS stay, any other tag gets an
  `EVEN_` prefix), `FactDate`, `PlaceDetails`, notes and citations carried over; `Details`, flags and files are
  not; the temple (`TempleCode`, unless `BLANK`) replaces the place text with the temple name (`lds_temples.rs`:
  GEDCOM temple codes, short aliases, a longer text matches the start of a name, anything else stays as typed);
  `DateStatus` comes from the per-type status element (`LDSDateStatus` enum, blank not written). The sealing
  facts on a `SpouseRelationship` are lost by the app's chain for files older than 11.1; here they become
  family facts of the couple (deliberate difference).
- **Biology** [< 11.5, haplogroups < 7.3] (checked): `BloodGroup`, `RightEyeColor`, `LeftEyeColor`, `HairColor`
  become facts `FACT_BloodGroup`, `FACT_RightEyeColor`, `FACT_LeftEyeColor`, `FACT_HairColor` (in this order,
  after the other facts and the LDS facts) with the text as `Details`; a missing element, "Unknown" and
  "NotYetKnown" make no fact, an empty element a fact without details. `HaplogroupY`/`HaplogroupM` become
  `Biology/YDNA/YDNA` and `Biology/MtDNA/MtDNA` entries (`Haplogroup`, `STRs`, `SNPs`) unless "Unknown" (the app
  also adds empty entries for missing ones; here nothing is added).
- **Dates** (checked): `StartDate`/`EndDate` -> `Start*`/`End*` (time of day -> `StartTime`/`EndTime`);
  `IsTextDate` -> `TextOnly`; `IsDateRange` -> `Span`, or `Range` if `StartDateDescriptor` is "Between" (and
  `Exact` for "To"); otherwise `Descriptor`, case-insensitive enum name or the words "From"/"To" (-> `FromOnly`/
  `ToOnly`), else `Exact`; BC flags -> `StartEra`/`EndEra` 0; a `CalendarType` other than Gregorian ->
  `StartCalendar`/`EndCalendar`. The children come out in the class's declaration order (`StartDay`, `StartMonth`,
  `StartYear`, `EndDay`, `EndMonth`, `EndYear`, `StartTime`, `EndTime`, `DateDescriptor`, `StartCalendar`,
  `EndCalendar`, `StartEra`, `EndEra`, `Text`). `0001-01-01` (DateTime.MinValue) counts as unset here (the app
  turns it into the year 1).
- **Ids** [< 2.1]: 36-character GUIDs become 22-character base64url of `Guid.ToByteArray()`, everywhere they are
  referenced (checked).
- **Author**: root `Author` moves into `AuthorCollection`.

Earlier guesses that the differential run confirmed: the `NCHI` tag and `NumberDetails` element of the
number-of-children fact (with `IsFamily`), the element names `StartEra`/`StartCalendar`/`EndCalendar` and their
position, and `text/richtext` for stories (a literal of the app's story reader). It also corrected two:
a date range is `Span` (not `Range`/`FromOnly`/`ToOnly`), and the parent-link modifier comes from the child's
`Parent` entry.

Not added (the app's chain does it; no data involved): empty `BIRT`/`DEAT` facts and `Birth`/`Death` defaults,
empty `Source` notes and repository entries.

**Not migrated** (kept verbatim; does not set `has_legacy_elements`): `FamilyProperties/Description` and the
other obsolete members of address/repository/source/task/author records, per-file `Tags`, `ChangedNotes`/
`FlatNotes`, calendar month adjustments (French/Icelandic/Hebrew), `AncestralFileNumber`/`RecordFileNumber`/
`CustomId`. Because `Version` is bumped, the Windows app will not
convert these either once the file was saved by Rootkin. `FamilyId` is upgraded on import (below).

## Story database (LiteDB v4 reader)

`crate::litedb` is a small read-only reader of LiteDB v4 data files (data file version 7), written from the
public LiteDB 4.1.x format (MIT). 4096-byte pages, page 0 (header) in the clear: 25-byte page header
(`id u32`, `type u8`, `prev u32`, `next u32`, `items u16`, `free u16`, 8 reserved), then `"** This is a LiteDB file **"`,
version 7, change id, free-list head, last page, user version, `SHA1(password)` (20, zeros if none), salt (16),
the collection name -> page table. With a password every other page is AES-256-CBC without padding, key and IV
`PBKDF2-HMAC-SHA1(UTF-8 password, salt, 1000 iterations)` (`key(32) | iv(16)`, the same IV for each page). A
collection page names the primary-key index whose skip list (`IndexNode`s, level 0) leads to the documents (BSON
in data blocks, or in a chain of extend pages for large ones). `FileStorage` is two collections: `_files`
(`_id`, `filename`, `mimeType`, `length`, `chunks`, `uploadDate`, `metadata`) and `_chunks` (`_id` =
`<file id>\<index, 5 digits>`, `data` binary, at most 255 KiB each). Verified against files written by the real
LiteDB 4.1.4 (`tests/data/old/stories7*`, generated by `make_stories7.cs`): encrypted (both passwords) and
unprotected, a story of two chunks. Not handled: journal areas, files other than version 7, other BSON types
than the ones LiteDB writes.

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

## Sources, repositories, citations (derived from the app's classes, not from real files)

No sample file with sources was available. The schema below was read from the vendor's
decompiled classes (`Source`, `Citation`, `CitationCollection`, `Repository`,
`SourceRepositoryCitation`, `Fact`, `Person`, `Name`, `ObjectBase`): `XmlSerializer` writes public
read/write properties in declaration order, base class first, honouring `[XmlElement]`,
`[XmlAttribute]`, `[XmlIgnore]` and `[DefaultValue]`/`ShouldSerialize*`. It is **unverified
against a real file**; only the fields listed under "written" are ever written.

- Collections: `SourceCollection/Source*` and `RepositoryCollection/Repository*` directly under `Family`
  (already in the skeleton, after `PeopleCollection`).
- Record attributes (`ObjectBase`): `Id`, `LastSaved` (only `ILastSaved` types: `Source`,
  `Repository`, `Person`), `Creation`. `CustomIds`/`CustomTags` are omitted when empty.
- `Source` element order: `Title`, `Abbreviation`, `Author`, `Publisher`, `ActualText`, `DataEvents`,
  `DataNotes`, `ResponsibleAgency`, `Repositories`, `Multimedia`, `Notes`, `LastSavedNotes`, `Privacy`.
  Null strings are omitted. `Repositories` has no `ShouldSerialize`, so it is always present (possibly
  empty). Written: `Title`, `Abbreviation`, `Author`, `Publisher`, `Repositories`, `Notes` (plain text, as
  for person notes). Read-only/preserved: everything else (`ActualText` is a `FlatNote`, structure unknown).
- `Repositories/SourceRepositoryCitation` (a record): `CallNumber`, `MultimediaType` (enum, omitted when
  `Blank`), `CustomMultimediaType`, `Repository` (repository id), `Notes`. Written: `CallNumber`, `Repository`.
- `Repository` order: `Title`, `FullAddress` (an `Address`: `Details`, `Phone`, `Fax`, `Email`, `WWW`;
  not written), `Notes`, `LastSavedNotes`, `Privacy`. Written: `Title`.
- `Citation` (a record, no `LastSaved`): `Source` (source id), `Page`, `EventType` (enum), `CustomEventType`,
  `Role` (enum, serialized from `RoleString`), `CustomRole`, `Url`, `Date` (`CustomDate`), `ActualTexts`,
  `Quality`, `CustomReference`, `Multimedia`, `Notes`, `Privacy`. Written: `Source`, `Page`.
  `Quality` enum: `Weak`, `Satisfactory`, `Good`, `Excellent`, `Blank` (default, omitted); it is read
  but not written, and GEDCOM `QUAY` / Gramps `confidence` are not mapped (the app's mapping is obfuscated).
- Citations hang on a `CitationCollection` (`Citations/Citation*`, then `Index`, always `0` as written by
  Rootkin) stored under the owner as: `Person/PersonCitationCollection` (between `FamilySpouseLinks` and
  `PrimaryImageId`), `Fact/FactCitationCollection` (between `Ages` and `Multimedia`; couple facts included),
  `Name/NameCitationCollection` (between `Nickname` and `Notes`; not supported). The collection is
  omitted when it has no citations (`ShouldSerializeCitations`).
- Deleting a source removes every citation of it; `edit::check` reports citations of missing sources
  and sources whose repository is missing.

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
- Sources: GEDCOM `0 SOUR` (`TITL`, `ABBR`, `AUTH`, `PUBL`, `NOTE`, `REPO` + `CALN`), `0 REPO` (`NAME`),
  `SOUR @S1@` + `PAGE` on individuals and events (inline `SOUR text` becomes a source titled with that text,
  one per distinct text). Gramps `<sources>` (`stitle`, `sauthor`, `spubinfo`, `sabbrev`, `noteref`,
  `reporef` with `callno`), `<repositories>` (`rname`; type is written as `Unknown`), `<citations>`
  (`page`, `sourceref`; `confidence` is written as 2 because the DTD requires it and ignored on import) with
  `citationref` on people and events, source notes as `Source Note` notes. Not carried: `QUAY`/`confidence`,
  source `DATA`/`TEXT`, citation notes/media/dates, repository addresses, citations on names.
- Not carried: person notes, addresses, couple status, event
  roles other than primary, Gramps tags/attributes, `_CUSTOM` tags, ANSEL encoding.
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

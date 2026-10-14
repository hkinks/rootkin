# TODO

See also [ux-review.md](ux-review.md) and [onboarding-review.md](onboarding-review.md)
(importing and first-hour findings; its "Recommended order" comes first).

## Onboarding fixes (from onboarding-review.md)

- [x] C1 Import asks before replacing a tree with unsaved changes
- [x] C2 Import on a background thread with progress; fix the import slowdown (20,000 people: 347 s)
- [x] C3 Decode ANSEL GEDCOM (names are corrupted today)
- [x] H1 Import `.gdz` and `.zip` containing `.ged` / `.gramps`
- [x] H2 Import summary dialog in plain language (grouped report, duplicates, broken links, "Copy details" and "Save report")
- [x] H3 Keep married names, GEDCOM 7 shared notes, date phrases, `EVEN`+`TYPE`, photo URLs, Ancestry "Living" placeholders (M4)
- [x] H4 Import into the current tree; find and merge duplicates (File > Import into this tree, Edit > Find duplicate people, Merge with... in the person panel)
- [x] H5 Second parent becomes the first parent's partner
- [x] H6 "Add child" focuses Given names; Enter needs a given name
- [x] H7 Toasts no longer cover the person panel buttons
- [x] M7 Symbols the default fonts can't draw
- [x] M1 Save as suggests the tree or file name; "Imported from ..., not saved yet" in the menu bar
- [x] M4 Ancestry "Living" placeholders
- [ ] M2, M3, M5, M6, M8 and Low items, see the review

## Next steps

1. Verify in the Windows app that Linux-saved files open (see "Verify")
2. Launch: Flathub/AUR packaging, announcements (plan kept locally, not in git)
3. Older formats: everything is converted but should be confirmed with real old files (database stories: private LiteDB build; story annotations are not converted)

## Publishing

- [x] License: Unlicense OR MIT, all dependencies permissive
- [x] No personal data in git history; fictional test fixture
- [x] README with interoperability / non-affiliation notice
- [x] Cargo metadata (repository, keywords, categories, rust-version)
- [x] `cargo fmt` + rustfmt config
- [x] GitHub Actions CI (fmt, clippy, test)
- [x] Screenshot of the example tree in the README
- [x] Decide commit email (personal vs GitHub noreply) before first push
- [x] Create the GitHub repo and push
- [x] Release workflow (`v*` tags → Linux/Windows/macOS archives)
- [x] Verify CI and release workflows on GitHub (release dry run via workflow_dispatch builds Linux/Windows/macOS)

## Import/export formats

Goal: import (and where sensible export) as many genealogy formats as possible.

Crate `rootkin-formats` (modules `gedcom`, `gramps`); `familyx` stays format-specific.

- [x] GEDCOM 5.5.1 export: INDI (NAME `Given /Surname/`, SEX, events with DATE/PLAC),
      FAM per couple (HUSB/WIFE/MARR/CHIL), single-parent FAM, `PEDI adopted`
- [x] GEDCOM import into a new `.familyx` via the edit API; dates mapped onto DateDescriptor
      (BEF/AFT/BET… native, unparseable as text); import report of dropped data
- [x] Gramps XML (`.gramps`, gzip) export: people, events, places, families with handles
- [x] Gramps XML import
- [x] CLI: `rootkin-cli export tree.familyx out.ged|out.gramps`, `rootkin-cli import in.ged|in.gramps out.familyx`
- [x] Editor: File → Import… (opens as new unsaved tree), File → Export…
- [x] Tests: example tree → GEDCOM → familyx and → Gramps → familyx preserve people/links/facts;
      a hand-written GEDCOM with unsupported tags
- [x] Photos in export/import (GEDCOM `OBJE`, Gramps `objects`; files in `<name>_media/`)
- [x] GEDCOM 7.0 export (import already tolerant of 7.0)
- [x] Sources/citations in import/export (schema derived from the app's classes, unverified against real files)

## Verify
- [x] Re-encrypted unmodified file, a newly created file, a GEDCOM-imported file and an edited file
      all load with the Windows app's own code (run on Linux via a .NET harness): OPC open,
      decrypt + HMAC + inflate, password check (unprotected), full `DataCollection` deserialization;
      the app's re-save reads back in Rootkin unchanged. Differences in Rootkin's XML are cosmetic
      (`IsPrimary` now limited to BIRT/DEAT/MARR/OCCU like the app).
- [ ] Visual check in the Windows app's UI (rendering of dates/photos/charts). Needs a Windows machine (under Wine 11 the app gets past font setup once the fonts are registered, but never opens its main window, even with software rendering)

## Editing
- [x] Notes: view/edit person notes and fact notes (`Notes/Notes/Note`, text/html/rtf formats)
- [x] Edit couple facts other than marriage (residence, divorce, …) and couple status (`SpouseRelationshipStatus`)
- [x] Alternative names in the UI (library: `add_alternate_name` done)
- [x] Parent-child modifier in the UI (library: `set_parent_modifier` done)
- [x] Set home person in the UI (library: `set_home_person` done)
- [x] Custom fact tags (`CustomFactTag`)
- [x] Reorder children / spouses

## Media
- [x] Gallery of all photos per person
- [x] Add photo in the UI (library: `Package::add_photo` done, png/jpg)
- [x] Set primary photo, remove photo
- [x] Face crop (`Face` element) for thumbnails in tree boxes

## Tree view
- [x] Compact ancestor layout (don't reserve 2^n slots for missing ancestors)
- [x] Show more than two parents (adoptive) and half-siblings
- [x] Photo thumbnails in boxes
- [x] Configurable generations up/down
- [x] Keyboard navigation (arrows to parent/child/spouse)

## Data / format
- [x] Sources window (View → Sources…): list, add, edit, delete (with its citations)
- [x] Edit sources/citations: "+ Cite source" on events and on the person, page field, remove
- [x] Older formats (`family7`, `family5`, `family4`, `family`, plain `content.xml`, family8/7 V1/V2): containers
      decrypt (verified against the app's own decrypt code) and the old element schema is migrated on open
      (`crate::migrate`, `Package::migration()`): relationships -> links with back-references, godparent/sponsor/
      witness -> associations, names, photos/files, family crest, stories, contact address, old dates, GUID ids, LDS
      facts, obsolete biology fields -> facts/DNA, `FactType`, fact addresses/witnesses/citations (docs/design.md,
      "Schema migration"). Derived from the decompiled conversion code and checked by running the app's own
      conversion chain on small synthetic inputs (reflection harness, not in the repo); crest and story files only
      by reading the code. Confirmation with real old files is still desirable.
- [x] Older formats: database-backed stories (`Story/FileId`, versions 7.0-14.1): the app's LiteDB file (`stories7`,
      password by version) is read by `crate::litedb` and becomes story notes; the part is dropped on save.
      Checked against files from the real LiteDB 4.1.4 and the app's own conversion (with 4.1.4 behind the
      private `LiteDB4` name). Open: no real database seen (the private build's `EncryptionVersion` is unknown),
      story annotations (WPF sticky notes) are dropped with a warning
- [x] Copies for My Family Tree 12.x (`family7` V2, schema 14.4), checked in 12.6.10: the example copy
      opens, photos show and survive its save; a file it saved opens in Rootkin (test fixture)
- [ ] Forward steps 14.1 (citation `Event` -> `EventType`) and 14.3 (Find a Grave, custom id kinds) for
      trees from versions before 12.6
- [x] "New family tree" in the UI (library: `Package::new` done)
- [x] Understand `FamilyId` attribute: password hash record, UI lock only; new files now get the default-password hash (docs/design.md)
- [x] Update `FileProperties/LastSavedDate` and `FamilyProperties/LastSavedPersonId` on save

## UX
- [x] Load/save on a background thread (PBKDF2 210k iterations blocks the UI briefly)
- [x] Recent files
- [x] Sort/filter options in the people list (by birth year, living only)
- [x] Date validation (day vs month length), partial dates
- [x] Reports / printing / export tree to PNG/SVG
- [x] Packaging: nix flake / desktop file / icon

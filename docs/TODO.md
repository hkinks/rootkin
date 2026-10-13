# TODO

See also [ux-review.md](ux-review.md) — its "Top 5" are the next UX priorities.

## Next steps

1. Verify in the Windows app that Linux-saved files open (see "Verify")
2. Launch: Flathub/AUR packaging, announcements (plan kept locally, not in git)
3. Remaining older-format leftovers (LDS facts, associations, ...) and source editing, once real sample files are available

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
- [ ] Sources/citations in import/export (needs the app's source schema from real samples)

## Verify
- [x] Re-encrypted unmodified file, a newly created file, a GEDCOM-imported file and an edited file
      all load with the Windows app's own code (run on Linux via a .NET harness): OPC open,
      decrypt + HMAC + inflate, password check (unprotected), full `DataCollection` deserialization;
      the app's re-save reads back in Rootkin unchanged. Differences in Rootkin's XML are cosmetic
      (`IsPrimary` now limited to BIRT/DEAT/MARR/OCCU like the app).
- [ ] Visual check in the Windows app's UI (rendering of dates/photos/charts) — needs a Windows machine

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
- [x] Sources and repositories shown read-only (View → Sources); preserved on save
- [ ] Edit sources/citations (needs real sample data for the schema)
- [~] Older formats (`family7`, `family5`, `family4`, `family`, plain `content.xml`, family8/7 V1/V2): containers
      decrypt (verified against the app's own decrypt code) and the old element schema is migrated on open
      (`crate::migrate`, `Package::migration()`): relationships -> links with back-references, names, photos/files,
      stories, contact address, old dates, GUID ids (docs/design.md, "Schema migration"). Derived from the decompiled
      conversion code and verified against hand-written fictional fixtures only (running the vendor code was not possible).
      Open: LDS facts, database-backed stories, godparent/sponsor/witness roles (-> associations), obsolete biology
      fields (-> facts), `FactType`, citations/witnesses/addresses on facts, family crest, a few guessed element
      names (see design.md); real old files would confirm all of it. `Package::has_legacy_schema()` flags the leftovers.
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

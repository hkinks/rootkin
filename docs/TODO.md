# TODO

See also [ux-review.md](ux-review.md) — its "Top 5" are the next UX priorities.

## Next steps

1. Publish on GitHub (see "Publishing" below)
2. Import/export: support as many genealogy formats as possible, starting with GEDCOM and Gramps
3. UX review top 5: reliable commit/save state, welcome screen, labelled relationship
   buttons, simpler tree interaction, free-text dates
4. Verify Linux-saved files open in the Windows app

## Publishing

- [x] License: Unlicense OR MIT, all dependencies permissive
- [x] No personal data in git history; fictional test fixture
- [x] README with interoperability / non-affiliation notice
- [x] Cargo metadata (repository, keywords, categories, rust-version)
- [x] `cargo fmt` + rustfmt config
- [x] GitHub Actions CI (fmt, clippy, test)
- [x] Screenshot of the example tree in the README
- [ ] Decide commit email (personal vs GitHub noreply) before first push
- [ ] Create the GitHub repo and push
- [x] Release workflow (`v*` tags → Linux/Windows/macOS archives)
- [ ] Verify CI and release workflows on GitHub after the first push

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
- [ ] Editor: File → Import… (opens as new unsaved tree), File → Export…
- [x] Tests: example tree → GEDCOM → familyx and → Gramps → familyx preserve people/links/facts;
      a hand-written GEDCOM with unsupported tags
- [ ] Later: photos in export (OBJE / Gramps objects), sources/citations, GEDCOM 7

## Verify
- [ ] Open a re-encrypted, unmodified file in the Windows app
- [ ] Open the generated example (`rootkin-cli example`) in the Windows app (new-file skeleton, random `FamilyId`)
- [ ] Open a file edited + saved by the Linux editor in the Windows app

## Editing
- [ ] Notes: view/edit person notes and fact notes (`Notes/Notes/Note`, text/html/rtf formats)
- [ ] Edit couple facts other than marriage (residence, divorce, …) and couple status (`SpouseRelationshipStatus`)
- [ ] Alternative names in the UI (library: `add_alternate_name` done)
- [ ] Parent-child modifier in the UI (library: `set_parent_modifier` done)
- [ ] Set home person in the UI (library: `set_home_person` done)
- [ ] Custom fact tags (`CustomFactTag`)
- [ ] Reorder children / spouses

## Media
- [ ] Gallery of all photos per person
- [ ] Add photo in the UI (library: `Package::add_photo` done, png/jpg)
- [ ] Set primary photo, remove photo
- [ ] Face crop (`Face` element) for thumbnails in tree boxes

## Tree view
- [ ] Compact ancestor layout (don't reserve 2^n slots for missing ancestors)
- [ ] Show more than two parents (adoptive) and half-siblings
- [ ] Photo thumbnails in boxes
- [ ] Configurable generations up/down
- [ ] Keyboard navigation (arrows to parent/child/spouse)

## Data / format
- [ ] Sources, repositories, citations (currently preserved but not shown)
- [ ] Older formats (`family7`, `family5`, `family4`, `family`) — read-only import
- [ ] "New family tree" in the UI (library: `Package::new` done)
- [ ] Understand `FamilyId` attribute (looks like a password-hash record: `SHA512\0210000\0salt\0hash`) — password-protected trees?
- [ ] Update `FileProperties/LastSavedDate` and `FamilyProperties/LastSavedPersonId` on save

## UX
- [ ] Load/save on a background thread (PBKDF2 210k iterations blocks the UI briefly)
- [ ] Recent files
- [ ] Sort/filter options in the people list (by birth year, living only)
- [ ] Date validation (day vs month length), partial dates
- [ ] Reports / printing / export tree to PNG/SVG
- [ ] Packaging: nix flake / desktop file / icon

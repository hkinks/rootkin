# TODO

See also [ux-review.md](ux-review.md) — its "Top 5" are the next UX priorities.

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
- [ ] GEDCOM import/export
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

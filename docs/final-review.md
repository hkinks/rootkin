# Pre-beta review (2026-10-15)

A code and app review before the public beta, at commit `2b71f3d`. All tests passed and clippy
was clean. Findings marked **[T]** were confirmed with a throwaway test or probe; **[R]** were
found by reading the code.

**Verdict:** not ready for a public beta until the Critical items and High 5 to 8 are fixed.
They can lose a tree or unsaved work.

## Critical

1. **A finished save runs a waiting action whose prompt is still open** (`rootkin/src/app.rs`,
   `Job::Saved` calls `after_save()`). Ctrl+S on a big tree, keep editing, File > Open: when the
   first save finishes, the other file opens and the newer edits are lost. **[T]**
   *Fix:* run the waiting action only for the save the prompt started, and only if nothing changed since. **Fixed in `90951aa`.**
2. **Background job results land on whatever tree is open.** Edits made while Open or Import runs
   are replaced without asking; "Import into this tree" adds to a tree opened in the meantime; a save
   that finishes after another tree was opened marks that tree saved under the first tree's path, so
   the next Ctrl+S overwrites the first file. **[T]** (first two)
   *Fix:* a generation number per loaded tree, tag every job with it and ignore stale results; block
   editing and file actions while a job runs. **Fixed in `90951aa` (generation and version per job, stale results ignored, modal "Working" overlay).**
3. **Deeply nested XML crashes the app** (`familyx/src/doc.rs` parser, recursive write/clone/drop).
   An 837-byte `.familyx` with 100,000 levels overflows the stack. Same for Gramps XML and GEDCOM
   levels (`rootkin-formats/src/gedcom.rs`). **[T]**
   *Fix:* reject nesting deeper than about 256 levels in both parsers. **Fixed in `ee4be5a` (256 levels).**
4. **Undoing "Remove photo" restores the record but not the image** (`familyx/src/package.rs`
   `remove_photo` deletes the part; undo restores only XML). Remove, Undo, Save: the photo is gone. **[T]**
   *Fix:* never delete parts on remove; drop unreferenced media parts when saving. **Fixed in `cb131fa`.**

## High

5. **Decompression bombs and huge declared sizes abort the app** (`package.rs`
   `Vec::with_capacity(entry.size())`, `codec.rs` inflate, `gramps.rs` gunzip, all without limits). **[T]**
   *Fix:* no capacity hint from the file; read through `.take(LIMIT)` with a clear error. **Fixed in `ee4be5a` (1 GB per entry, 2 GB per package).**
6. **RTF notes with non-ASCII text after an escape panic** (`familyx/src/richtext.rs`, byte slicing
   inside a character). **[T]**
   *Fix:* character-safe indexing. **Fixed in `ee4be5a`.**
7. **Closing the window while a "Save your changes?" prompt is open quits without asking**
   (`app.rs`, close is cancelled only when no prompt is showing). **[T]**
   *Fix:* always cancel the close when there are unsaved changes, and make the waiting action Close. **Fixed in `90951aa`.**
8. **Parent cycles are possible and freeze the tree view.** `link_parent` refuses only two-person
   cycles, merge never checks, "Add parent > Existing person" lists descendants, and the layout has no
   visited set. A 5-person cycle at 30 generations never finishes drawing. The layout is also rebuilt
   every frame (8,000 people at 12 generations: 4 s per frame). **[T]**
   *Fix:* `edit::is_ancestor`, refuse cycles in `link_parent` and `merge_people`, skip people already on
   the current path in the layout, cache the layout per (tree version, focus, settings). **Fixed in `6d98e3e` (layout of an 8,191-person pedigree at 12 generations: 4.0 s before, 37 ms per rebuild after, and only rebuilt when the tree, focus or settings change).**

## Medium

9. **Saves can overlap and aren't crash-safe** (`package.rs`, `app.rs`): Ctrl+S isn't blocked during a
   save, both use the same `.familyx.tmp` (false "Could not save" on a 40 MB tree in 20 of 20 runs),
   no fsync before the rename. **[T]**
   *Fix:* one save at a time, a unique temp name, `sync_all` before the rename. **Fixed in `90951aa` (one job at a time) and `06269f8`.**
10. **Godparent, witness and sponsor links (`Association/PersonId`) aren't repointed on merge or
    cleared on delete**, and `check()` doesn't look at them. **[T]** (merge) **Fixed in `e4082ac`.**
11. **Merge drops some of the removed person's data**: DNA (`Biology` is copied only if the kept person
    has none, and everyone has one), and `Tags`, `Associations`, `CustomIds` and couple notes/photos when
    the kept person already has that element. **[R]**
    *Fix:* merge these lists entry by entry, skipping duplicates. **Fixed in `e4082ac`.**
12. **An invalid date in the form is silently dropped**: Save goes ahead, "All changes saved" shows,
    and moving to another person discards the text. **[R]**
    *Fix:* count invalid fields as unsaved, or block Save with a message. **Fixed in `eeab597` (counts as unsaved; Save and moving on are refused with a message).**
13. **A loop in a `stories7` LiteDB index uses unbounded memory** (1 MB file, over 3 GB). BSON is read
    recursively without a depth cap. **[T]** **Fixed in `ee4be5a`.**
14. **Each photo reference in a `.gdz`/`.zip` stores its own copy of the image** (one 100 MB image
    referenced 40 times left 1.9 GB in the package). **[T]**
    *Fix:* read each archive entry once and share the media id; cap per photo and in total. **Fixed in `ee4be5a`.**
15. **Photo part extensions come from the untrusted `RelativePath` on append**, so a part name can
    contain `../`; the Windows app extracts packages to a folder. **[R]**
    *Fix:* allow only `png` and `jpg`. **Fixed in `eeab597`.**

## Low

- A panic inside a background job never reports back, so the busy state stays forever (`catch_unwind`). **Fixed in `90951aa`.**
- PNG export: `iw as u32 - x0` can underflow and panic (`saturating_sub`, return early). **Fixed in `eeab597`.**
- Photos are decoded on the UI thread and the whole cache is cleared on every edit. **Partly fixed in `eeab597`: the cache is no longer cleared on edits; decoding is still on the UI thread (once per photo).**
- `duplicates.rs`: `(x - y).abs()` on years from the file can overflow (`abs_diff`). **Fixed in `eeab597`.**
- `index.rs`: cached link hits aren't verified against the element (all callers are correct today). **Fixed in `eeab597`.**
- Imports can pull any local png/jpg through absolute or `../` paths; list photos from outside the
  import folder in the import report. **Not done yet.**
- Wording: "image (SVG)" vs "picture (PNG)"; "Remove link" vs "Remove partnership" vs "Delete";
  "Text size: Ctrl + / Ctrl −" is a plain label in the View menu. **Fixed in `eeab597`.**

## Checked and fine

Unknown XML round-trips byte for byte; `.bak` copies; index invalidation on every current edit path;
merge refuses merging a person with themselves and repoints couples, back-references and the home
person; append gives fresh ids; undo snapshots start with fresh index tables; exported media stays
inside the media folder; zip entries are never extracted to disk; 20,000 mutated inputs found no panics
in dates, ANSEL or the GEDCOM line parser.

# Onboarding review: getting my family tree into Rootkin (2026-10-14)

A UX review of the first hour with Rootkin, done in the role of an end user: someone
with design experience who is not a genealogy-software expert and wants to move their
family tree into Rootkin. It follows on from [ux-review.md](ux-review.md) (2026-10-13).
Most of that review's top five have been fixed. This one is about **getting data in**.

## How the review was done

- **Persona.** Maria, 50s. She has about 1,500 relatives on Ancestry and a smaller tree
  on MyHeritage that a cousin keeps. Her father left a PAF file from 2009. She also wants
  to type in her own branch by hand. She uses Linux and cares about privacy, which is
  why she picked Rootkin.
- **The real app, driven headlessly.** A throwaway `egui_kittest` harness (not committed)
  clicked through the real `App` with the wgpu renderer at 1400x900 and 1280x720, typed
  text, pressed keys and saved screenshots. Imports went through the same code path as
  *File → Import*, minus the file picker.
- **Test files.** All fictional, made to look like real exports:

  | File | Mimics | Result |
  |---|---|---|
  | `ancestry.ged` | Ancestry: CRLF, `_APID`, `_TREE`, online photo URL, `EVEN`+`TYPE`, "Living" placeholder | Imported; 3 warnings |
  | `myheritage.ged` | MyHeritage: UTF-8 BOM, `_UID`, `RIN`, `_UPD`, `_MARNM`, `PEDI adopted`, online photo | Imported; married name lost |
  | `paf-ansel.ged` | PAF 5 / Legacy: `CHAR ANSEL` with diacritics | Imported, **names corrupted** |
  | `ftm-ansi.ged` | Family Tree Maker: `CHAR ANSI` (cp1252) | Imported correctly; confusing warning |
  | `legacy-utf16.ged` | UTF-16 LE with BOM | OK |
  | `oldmac-cr.ged` | CR-only line endings | OK |
  | `familysearch-g7.ged` | GEDCOM 7: `SNOTE`, `PHRASE`, `EXID` | Imported; shared note lost |
  | `familysearch.gdz` | GEDCOM 7 zip archive (`.gdz`) | **Rejected** |
  | `Ancestry-download.zip` | A `.ged` inside a zip, as some sites deliver it | **Rejected** |
  | `messy-no-trlr.ged` | No `TRLR`, 31 Feb, missing pointers, duplicate person | Imported; partly flagged |
  | `large-5000.ged` / `large-20000.ged` | Big Ancestry-sized trees | **5.6 s / 347 s** in a release build |
  | `example-export.gramps` | Gramps XML (gzip) | OK |
  | `app12_6.familyx` | Tree saved by My Family Tree 12.6 | Opened with a conversion notice |
  | `contacts.csv`, `tree.ftm` | Wrong files a user might try | Rejected with jargon |

## What works well

- **The welcome screen is clear.** Three big choices (new, open, import) plus recent
  files. Within seconds I knew where to start.
- **"Start a new family tree" goes straight to "New person"** with the cursor already in
  "Given names". Type, Tab, type, Enter, and the first person exists. That is the
  fastest start I have seen in a genealogy app.
- **Date entry is forgiving.** `12.03.1950`, `March 12, 1950` and `ca 1950` all parse,
  and "Understood as: 12 Mar 1950" confirms it while typing. `31 Feb 1950` gets a clear
  red message and "(not saved yet)". This is good.
- **Wide encoding support.** UTF-8 with and without BOM, UTF-16, cp1252 and CR-only line
  endings all came through intact. Gramps round-trips cleanly.
- **Colour plus symbol for gender** (♂/♀) in the tree, labelled buttons ("+ Add parent",
  "+ Add child with Toomas"), and undo toasts all make editing feel safe.
- **Older My Family Tree files open** and explain what happened, including the hint
  about saving a copy for version 12.

## Findings

Severity: **Critical** = data loss or the user cannot finish. **High** = likely to
derail or mislead. **Medium** = friction. **Low** = polish.

### Critical

**C1. Importing throws away unsaved work without asking.**
With unsaved changes open, *Import GEDCOM / Gramps…* replaces the tree straight away. The
only sign is a toast: "Your previous tree had unsaved changes; it is still on disk
unchanged". So everything typed since the last save is gone, and Undo can't bring it
back. *Open* and *New* ask first through `guarded()`; Import does not (`app.rs`,
`import`).
*Fix:* run Import through the same "Save your changes?" modal.

**C2. Big imports freeze the window for minutes.**
The import runs on the UI thread, and its time grows much faster than the tree does (release build):

| People | Import time |
|---:|---:|
| 5,000 | 5.6 s |
| 20,000 | 347 s (almost 6 minutes) |

Ancestry trees of 5,000 to 30,000 people are not unusual. During that time there is no
spinner and the desktop reports "not responding". Most users would kill the app and
decide Rootkin can't handle their tree. Opening the saved 5,000-person `.familyx`
afterwards takes only 0.2 s, so the slow part is the import itself.
*Fix:* move import to the background job thread as Open already is, show progress
("Reading people… 3,200 of 20,000"), and profile the import. Four times the people took 62 times as long,
which points to a search through the whole document for each person or family added.

**C3. ANSEL files come in with corrupted names, and the user can't tell.**
`CHAR ANSEL` is the default for PAF, older Legacy and RootsMagic exports, and for
many older GEDCOM files. "Mihkel Põder" becomes "Mihkel Päoder", "Liisu Mäe"
becomes "Liisu Mèae", and "René" becomes "Renâe". The only warning is in a toast that
fades away, and it is written for developers ("read as Latin-1 (ANSEL is not supported)").
Users then save, and the corruption is permanent.
*Fix:* implement ANSEL decoding (a fixed table: combining marks come before the base
letter). Until then, show a blocking dialog with a preview of affected names and a
"Cancel import" button.

### High

**H1. Zipped exports are rejected.**
Two cases fail with "cannot tell the file format… (expected .ged or .gramps)". One is the
GEDCOM 7 archive `.gdz` (the standard format for GEDCOM 7 with media). The other is a
`.zip` holding a `.ged`, which is how some sites and email attachments deliver trees.
Users don't know they should unzip first.
*Fix:* look inside zip files for `.ged` / `.gramps` (and the media folder), and accept `.gdz`.

**H2. The import report is written for developers and disappears.**
Example toast: "Imported 4 people and 1 families (3 notes: dropped 1x INDI.BIRT.SOUR._APID;
dropped 1x SOUR._APID; dropped 1x photo of a type other than PNG/JPEG). Save it as a
.familyx file to keep it." With longer reports the text runs off the left edge of the
screen (seen with the ANSEL file). Nobody can act on `INDI.BIRT.SOUR._APID`.
*Fix:* after every import, show an **import summary dialog**:
- what came in: "1,512 people, 604 families, 230 sources, 0 photos"
- what was left out, in plain words and grouped: "Ancestry record links (2): these only
  work on ancestry.com", "Online photos (1): Rootkin works offline, so download them
  and add them by hand", "Married names (12)"
- **"Copy details" and "Save report"** so the user can keep the list
- buttons: *Save as .familyx…* (primary) and *Look around first*

Also fix "1 families" (plural).

**H3. Data that people care about is dropped.**
These were silently lost or listed only in the toast:
- **Married names** (`_MARNM`, written by MyHeritage, Ancestry and RootsMagic). Rootkin
  already supports "Other names" of type *Married*, so map them there.
- **GEDCOM 7 shared notes** (`SNOTE`). These are the main way notes are stored in
  GEDCOM 7, so dropping them loses stories. Convert them to person or event notes.
- **Date phrases** (`PHRASE`, e.g. "sometime in the spring of 1901"). Keep them as the
  date's text, or add them to the note.
- **Custom event types**: `EVEN` + `TYPE Arrival` became an untitled event with
  "Arrival" in its note. Use it as a custom event name, which the editor supports.
- **Online photos** (Ancestry, MyHeritage URLs). The message "photo of a type other than
  PNG/JPEG" is wrong, because the problem is that it is a URL. Say so, and keep the URL
  in the person's notes so the user can download the photo later.

**H4. You can't add an import to a tree you already have.**
Import always creates a new tree. My realistic path was: type my own branch by hand,
then add my cousin's MyHeritage export and my father's PAF file. That isn't possible,
and there is no way to find or merge duplicate people either.
*Fix (staged):*
1. "Import into this tree…" that adds people next to the current ones
2. A duplicate finder (same name with a birth year within 2 years) and a "Merge
   these two people" action with a side-by-side comparison

**H5. Two parents added one after the other aren't partners.**
On "Maria", I added Jüri as a parent, then Anna as a parent. The tree draws them side by
side, but they are not linked as a couple. Their partner list is empty, so "+ Add child
with Anna" never appears on Jüri, and a sibling added from Jüri gets only one parent.
*Fix:* when the person already has exactly one parent, add the second parent as that
parent's partner by default (with a checkbox "Anna and Jüri are a couple").

**H6. "Add child" doesn't put the cursor in the name field.**
The surname is pre-filled, and focus is only requested when both fields are empty
(`relation_dialog.rs`). So the cursor isn't in "Given names" and the first keystrokes
go nowhere. Pressing Enter then creates a child called just "Kask". This happened to me
in the walkthrough.
*Fix:* always focus "Given names" when the dialog opens. Also don't allow Enter to add
a person who has only a pre-filled surname.

**H7. Toasts cover the family buttons.**
The undo toasts stack in the bottom-right corner, which is exactly where the person
panel has "+ Add partner", "+ Add child" and "Delete this person…". After adding three
people quickly, the next button was hidden behind toasts.
*Fix:* show toasts at the bottom centre of the tree area, or keep only the newest one.

### Medium

**M1. After an import it isn't clear what file you're in.** The title says "New family
tree" even though I imported `ancestry.ged`, and Save offers `family.familyx`.
Suggest the source name (`Kask Family Tree.familyx`, from the GEDCOM's `_TREE` or file
name) and show "Imported from ancestry.ged, not saved yet" in the title area.

**M2. The conversion notice for older files is alarming.** "Rootkin converted it…:
0 relationships and 0 photos were carried over" for a 17-person tree full of families.
To a user that reads as "all my links are gone". Only mention counts that are meaningful,
and drop "(format 14.4)". The bar says "All changes saved" although the converted tree
hasn't been written yet. Show "Converted, not saved yet".

**M3. Error dialogs are technical.** Opening a CSV shows "zip: invalid Zip archive:
Could not find EOCD" under "Something went wrong". The full temp path wraps over three
lines. Better: "This isn't a family tree file. Rootkin opens .familyx files from My
Family Tree. To bring in a tree from another program, use Import (GEDCOM or Gramps)."
Recognise common foreign formats by name: `.ftm`/`.ftmb` (Family Tree Maker: "export
GEDCOM from Family Tree Maker first"), `.rmtree`, `.paf`, `.csv`.

**M4. Ancestry's "Living" placeholders come in as real names.** Ancestry exports
private people as given name "Living". Detect `NAME Living /X/` and import them as
unnamed living people marked private, so the list doesn't fill up with "Living Kask".

**M5. Gender always starts at "Unknown".** In "Add parent" and "Add partner" every new
person needs an extra click. Offer "Add father" / "Add mother", or default to the
opposite of the partner's gender (still editable). This also removes the "?" from the
tree boxes, which looks like an error.

**M6. Dates: a few common forms are missing.**
- `1950s` and localised month names (`12 märts 1950`) are rejected.
- `3/12/1950` silently means 3 December, while US users mean 12 March. The preview
  shows it, but an ambiguous numeric date should say "3 Dec 1950 (day/month); for
  12 Mar type 12 Mar 1950".
- On import, `31 FEB 1850` is accepted although the editor rejects it, and
  `1899-1900` became `1899` (the range was lost).

**M7. Some symbols render as empty boxes.** The default font has no glyph for "●"
(Unsaved changes), "→" ("View → Sources…", "File → Save a copy…") or "✕" (remove
citation). The remove-citation button is therefore an empty square. Use characters the
bundled font has, or bundle a font that has them.

**M8. Large trees: the people list needs grouping.** With 5,000 people, "Sort: surname"
lists 18 "Aleksander Ilves" entries with birth years out of order. Sort by birth year
within a name, and add an A–Z jump or surname groups.

### Low

- **Recent files** show full absolute paths in link blue. Show the file name in bold
  with the folder in grey below it, plus "Remove from list".
- **Duplicate people** in the messy file (`Ott Ilves b. 1850` twice) were not mentioned
  in the import report. A "possible duplicates: 1" line would help (see H4).
- **Broken links** (`CHIL @I404@`) are reported in the toast, but a `FAMS` to a missing
  family is silent. Put both in the import summary.
- **At 1280x720** the tree is clipped at the right edge and the person panel needs a
  lot of scrolling. Make the person panel collapsible, or let it overlay on narrow
  windows.
- **No sample tree in the app.** The CLI can make the fictional example; a "Try the
  example tree" link on the welcome screen would let people explore before committing
  their own data.

## Recommended order

1. **Make imports safe** (C1, C3, H6): ask before replacing unsaved work, decode ANSEL,
   and fix the focus bug. These are small changes that prevent losing or corrupting data.
2. **Make imports scale** (C2): background thread, progress, and fix the quadratic step.
   Without this, real Ancestry users can't get in at all.
3. **Import summary dialog** (H2, M1, M4): plain-language report, suggested file name,
   "not saved yet".
4. **Keep what people care about** (H3, H1): married names, shared notes, custom event
   types, photo URLs, zip/gdz.
5. **Build the family by hand smoothly** (H5, H7, M5): parents become partners, toasts
   out of the way, father/mother buttons.
6. **Combine sources** (H4): import into the current tree, then find and merge duplicates.
7. Polish (M2, M3, M6–M8, Low).

## Acceptance checks for a fix

A good regression set is the table of test files above. For each import, check:

- the people, families and sources counts match the source file
- names with diacritics are identical
- every dropped item is in the summary dialog in plain language
- a 20,000-person import finishes in under 10 seconds and the UI stays responsive
- importing with unsaved changes asks first

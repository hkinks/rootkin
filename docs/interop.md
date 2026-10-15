# Interoperability with other genealogy programs

What was tested, what survives a trip through other software and what does not, and how to
repeat the tests. Written 2026-10-15 against Rootkin 0.1.0-beta.1. All test data is
fictional: Gramps' own example tree, a Gramps GEDCOM sample, Rootkin's built-in example
family, and small files written by hand.

## What was tested

| Tool | Version | Used for |
|---|---|---|
| Gramps (command line only, no windows) | 6.0.6 | Real import and export of GEDCOM and Gramps XML, in both directions |
| `gedcom7` (Python, built from the FamilySearch GEDCOM 7.0.18 tables) | 1.2.0 | Validating GEDCOM 7 exports |
| `ged4py` | 0.5.5 | Reading GEDCOM 5.5.1 exports with an independent parser |
| `python-gedcom` | 1.1.0 | Same, second independent parser |
| `xmllint` (libxml2) with Gramps' own `grampsxml.dtd` and `grampsxml.rng` | nixpkgs | Validating Gramps XML exports |
| A small structure checker written for this review (not in the repo) | | GEDCOM 5.5.1: `HEAD`/`TRLR`, 255 byte lines, level jumps, xref format, dangling pointers, `FAMC`/`FAMS` against `HUSB`/`WIFE`/`CHIL`, date grammar, `SEX`, `PEDI` |

Data sets:

- Gramps' `example.gramps`: 2,157 people, 762 families, 3,432 events, 7 media files, in
  many scripts (Latin, Cyrillic, Greek, Arabic, Chinese), with non-Gregorian calendars,
  adoptions, witnesses, notes, tags and addresses. The most realistic large test.
- Gramps' `sample.ged`: 42 people, written by a different program.
- Rootkin's example family (`rootkin-cli example`), with photos.
- A hand-written GEDCOM with names in several types, every date form (exact, `ABT`,
  `CAL`, `EST`, `BEF`, `AFT`, `BET..AND`, `FROM..TO`, `INT`, phrases), a custom event, an
  address, a long note with line breaks and an `@`, adoptive and foster children, a
  citation with a repository, and a photo.

Paths tested:

1. Gramps XML to Rootkin to Gramps XML (and the same through GEDCOM 5.5.1 and 7).
2. Gramps GEDCOM export to Rootkin.
3. Rootkin to GEDCOM 5.5.1, GEDCOM 7 and Gramps XML, then into Gramps, then out again.
4. Rootkin to each format and back into Rootkin, compared through Gramps XML.
5. A `.gdz` archive built from a GEDCOM 7 export plus its photos, imported back.

## What survives

Legend: **yes** means no difference found, **partial** means kept in a different or
reduced form, **lost** means gone (Rootkin reports most of these on import).

Columns: **Gramps to Rootkin** is a Gramps XML file imported into Rootkin. **Rootkin to
Gramps** is a Rootkin tree opened in Gramps through GEDCOM 5.5.1 or Gramps XML.
**Rootkin to Rootkin** is a Rootkin tree exported and imported again.

| Data | Gramps to Rootkin | Rootkin to Gramps (5.5.1 / XML) | Rootkin to Rootkin (5.5.1 / 7 / XML) |
|---|---|---|---|
| People, families, parent links | yes (2157 of 2157, 762 of 762) | yes | yes |
| Names with diacritics and non-Latin scripts | yes | yes | yes |
| Alternate names (married, birth, aka) | yes | yes | yes |
| Name title, suffix, nickname | yes | yes | yes |
| Second surname or surname prefix | partial: joined into one surname | yes | yes |
| Call name, family nickname, name groups | lost | n/a | n/a |
| Exact, approximate, estimated, calculated, before, after dates | yes | yes | yes |
| Ranges (`BET..AND`) and spans (`FROM..TO`) | yes | yes | yes |
| Open ended `FROM` or `TO` | n/a | partial in Gramps XML (kept as text) | yes |
| Free text dates and phrases | yes | yes (GEDCOM 7: empty `DATE` plus `PHRASE`) | yes |
| Non-Gregorian calendars (Julian, Islamic, ...) | partial: kept as text | n/a | yes (as text) |
| Dual dating, new year marker | lost | n/a | n/a |
| Places | partial: hierarchy joined into one text ("Orlando, Orange, FL, USA") | yes, as one flat place | yes |
| Place coordinates (`MAP`) | lost | n/a | n/a |
| Standard events and facts | yes | yes | yes |
| Custom event types | yes (kept as a named event) | yes (type name kept) | yes |
| Event description | yes | partial: GEDCOM puts it in a `NOTE`, Gramps XML keeps it | yes |
| Event roles (witness, clergy, informant) | lost | n/a | n/a |
| Addresses | partial in GEDCOM (text in the fact's details), lost in Gramps XML | n/a | n/a |
| Person attributes (Gramps) | lost | n/a | n/a |
| Person notes | yes | yes | yes |
| Notes on events, families, names, places | lost | n/a | n/a |
| Sources, repositories, call numbers | yes (author, title, publisher, abbreviation, note) | yes | yes |
| Citations on people and events (with page) | yes | yes | yes |
| Citations on families and on names | lost (735 of 2854 in the example tree) | n/a | n/a |
| Citation confidence (`QUAY`), citation dates, source data text | lost | n/a | n/a |
| Photos attached to people (PNG and JPEG) | yes | yes (relative paths in `<name>_media/`) | yes |
| Photo captions, other media types, media on events or sources | lost | n/a | n/a |
| Couple relationship type (married, civil union, living together, ...) | yes, as the closest of Rootkin's types | yes (GEDCOM `MARR` or `EVEN` with `TYPE`, Gramps `rel`) | yes |
| Couple status (together, divorced, ...) | n/a | lost (no counterpart) | lost |
| Adoptive and foster children | yes (per parent) | yes | partial: GEDCOM has `PEDI` per child and family, so a child adopted by one parent only becomes adopted by both |
| Tags, LDS ordinances, bookmarks, associations | lost | n/a | n/a |
| Living flag | guessed (no death data and born under 110 years ago) | n/a | guessed again |

"n/a" means the data is not something Rootkin can hold, so there is nothing to send.

### Numbers from the Gramps example tree

Gramps XML, to Rootkin and out as Gramps XML again, compared with what Gramps itself
writes for the same file:

| | Original | After Rootkin |
|---|---|---|
| People | 2,157 | 2,157 |
| Families | 762 | 762 |
| Sources / repositories | 4 / 3 | 4 / 3 |
| Citations | 2,854 | 2,094 (family and name citations are not carried) |
| Photos | 7 | 6 (one belongs to an event, not a person) |
| Notes | 19 | 5 (only person and source notes are carried) |
| Places | 1,296 (hierarchy) | 857 (flat texts) |
| Events | 3,432 | 3,453 (21 empty marriage events for families Gramps calls "Married" without an event) |

The same file through GEDCOM 5.5.1 (written by Rootkin, read by Gramps) matches what
Gramps made from the Gramps XML directly for people, families, names, dates, places and
sources. A Rootkin tree exported and imported again in Rootkin, in all three formats,
is identical in event types, dates, names, places, sources, citations, notes and photos.

## Problems found and fixed

Each has a regression test in `crates/rootkin-formats/tests/interop.rs`.

1. Estimated and calculated dates were written to Gramps XML as `type="estimated"`, which
   the Gramps DTD does not allow. Gramps read them as "to" dates. Now written with the
   `quality` attribute and read back.
2. Custom event types from Gramps lost their name (the type became a generic event and
   the name was glued to the description). They are now named events and keep their name
   on export.
3. Name title, suffix and nickname were dropped on import. They are kept, and the title
   is written (`NPFX`, `<title>`).
4. GEDCOM 7 files failed validation: lower-case name types, free text dates, and every
   `@` doubled. Now upper-case types (`OTHER` plus `PHRASE` for the rest), an empty
   `DATE` with a `PHRASE`, and only a leading `@` doubled. The 7.0 validator reports no
   errors for any export.
5. Notes on people were not carried to or from Gramps XML.
6. The relationship type of a couple (civil union, living together, ...) was lost
   through GEDCOM and Gramps. A "Married" Gramps family without a marriage event became
   "relationship unknown".
7. `ADDR` on a fact and the words of an `INT` date were dropped without a trace. They
   now go into the fact's details.
8. The Gramps XML header lacked the empty `<researcher/>` element that Gramps' RelaxNG
   schema requires. With it, exports validate against both the DTD and the schema.
9. `rootkin-cli export ... --gedcom7` writes GEDCOM 7 from the command line (it only
   wrote 5.5.1 before).

## Known limitations

- **Gramps 6.0 cannot read GEDCOM 7.** It treats the file as ANSI, so every accented
  and non-Latin letter turns into mojibake. This is Gramps, not Rootkin: the same file
  is fine in readers that support 7. Use GEDCOM 5.5.1 or Gramps XML to go to Gramps.
- Rootkin reads Gramps XML and GEDCOM, but not a Gramps package (`.gpkg`).
- Rootkin writes Gramps XML schema 1.7.1. Gramps 6 reads it without complaint but
  writes 1.7.2 itself. Gramps XML written by Rootkin has no media metadata beyond the
  file path and no tags.
- Places are flat texts. A hierarchy is joined with commas on import and is not
  rebuilt on export.
- Photos in GEDCOM exports sit in `<name>_media/` next to the file. A GEDCOM that
  points at absolute paths on another computer (Gramps does this) imports without its
  photos and Rootkin says how many were not found.
- Only PNG and JPEG photos are imported.
- Neither GEDCOM nor Gramps XML has Rootkin's "living" flag, so on import it is guessed
  from the dates.
- Rootkin writes no custom tags (`_TAG`) and no GEDCOM 7 `SCHMA`. It also does not
  import user defined `_TAG` facts.
- `SEX X` is written in GEDCOM 5.5.1 as well. It is not in the 5.5.1 list, but common
  readers accept it.
- Not tested for lack of a free tool or data: RootsMagic, Family Tree Maker, Ancestry
  and MyHeritage exports, Legacy, PAF. Their quirks (for example Ancestry's `Living`
  placeholder names) are handled from specifications and earlier bug reports only.

## How to run the tests again

The regression tests need nothing but the Rust toolchain:

```
nix-shell --run "cargo test -p rootkin-formats --test interop"
```

The checks with real programs need a scratch directory outside the repository and a
throwaway Gramps home, so your own Gramps data is never touched:

```
export GRAMPSHOME=$(mktemp -d) HOME=$GRAMPSHOME
nix-shell -p gramps --run "gramps -y -C t -i in.ged -e out.gramps -f gramps"
```

1. Build the tool: `cargo build --release -p rootkin-cli`.
2. Get Gramps' fictional example: it is installed with Gramps under
   `share/doc/gramps/example/gramps/example.gramps` (and `.../gedcom/sample.ged`).
3. Convert: `rootkin-cli import example.gramps t.familyx`, then
   `rootkin-cli export t.familyx t.ged`, `rootkin-cli export t.familyx t7.ged --gedcom7`
   and `rootkin-cli export t.familyx t.gramps`.
4. Let Gramps read each export: `gramps -y -C name -i t.ged -e back.gramps -f gramps`,
   once per file, each with its own tree name.
5. Compare `back.gramps` with what Gramps makes from the original
   (`gramps -y -C base -i example.gramps -e base.gramps -f gramps`). A few lines of
   Python that count `person`, `family`, `event` (by type), `source`, `citation`, `note`
   and `object` elements and list the names and dates is enough; both files are gzip
   compressed XML.
6. Validate GEDCOM 7: in a virtual environment, `pip install gedcom7`, then
   `gedcom7.validate(gedcom7.load(open("t7.ged", "rb")))` should return an empty list.
   `pip install ged4py python-gedcom` give two more independent GEDCOM 5.5.1 readers.
7. Validate Gramps XML: `xmllint --noout --dtdvalid grampsxml.dtd t.gramps` (remove the
   `xmlns` attribute first), and `xmllint --noout --relaxng grampsxml.rng t.gramps`
   (change the namespace from 1.7.1 to 1.7.2 first). Both files ship with Gramps in
   `share/gramps/`.

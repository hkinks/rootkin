# Familyx .familyx test fixture

Files:

- `db.familyx` -- original Familyx export (Chronoplex Software), copied from the vault. Do not publish.
- `db.obfuscated.familyx` -- test fixture: same container, obfuscated media.

## Container format

`.familyx` is an OPC package (zip, same convention as .docx):

| Part | Content |
|---|---|
| `[Content_Types].xml` | Declares `family8` as `application/vnd.chronoplexsoftware.familyx`, plus png/jpg/rels defaults |
| `_rels/.rels` | Relationship `MyFamilyTree/Relationship` from the package root to `/family8` and to every `/Multimedia/<id>.png\|jpg` |
| `family8` | 91,504 bytes. Single uniformly high-entropy blob (7.95 bits/byte, no zlib/gzip/bz2/lzma/zstd/snappy/lz4 magic). Compressed or encrypted with an undocumented vendor codec. Tree data (names, dates, relationships) lives here, opaque. |
| `Multimedia/*` | 48 photos, random-hash file names, no personal data in names |

## Obfuscation applied to the fixture

- All 48 photos: `-auto-orient -strip -resize 8% -filter Box -resize 1250% -blur 0x4` (ImageMagick). Heavy pixelation, faces not recognizable, valid images, metadata stripped.
- Zip entry timestamps normalized to 2021-01-01; parts re-packed as STORED.
- `[Content_Types].xml`, `_rels/.rels`, `family8`: byte-identical to the original.

## Caveat

Because the `family8` codec is unknown, the tree text (names, dates) is still recoverable in principle from the opaque blob. Photos -- the clearest personal data -- are gone. If the codec gets cracked during the parser work, re-derive the fixture: decode `family8`, substitute names/dates, re-encode, re-pack.

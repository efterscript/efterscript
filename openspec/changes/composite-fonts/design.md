# Design: Composite fonts

See proposal.md for motivation and the spec deltas for the contract.
This document fixes the CMap model, how CID-keyed fonts and Type 0
fonts live in the VM, how composite text flows through the show frame,
the widened glyph, and the PDF Type 0 output.

## Context

- The show frame works on a `Vec<u8>` of codes, one glyph per byte,
  with `FontKind::{Resident, Type3, Embedded}`; `Glyph { code: u8, dx,
  dy }` crosses the boundary and lands in `IrOp::Text`; the dump prints
  the run's bytes as a string; the content writer emits one-byte `Tj`.
- The CFF reader handles CID-keyed fonts (`glyph_by_cid`, per-glyph
  private data, `ros`, `cid_count`); `StartData` parks CID-keyed
  programs under `Interp::cid_program(name)`. The TrueType parser and
  subsetter exist; the CFF writer subsets name-keyed fonts only.
- Resource categories are an enum (`Font`, `Encoding`, `ProcSet`,
  `FontSet`); procedure sets are built-in dictionaries.
- Adobe's CMap resources are BSD-licensed; `Identity-H` is 8 KB and
  `Identity-V` 3 KB; the Unicode CMaps for CJK orderings are 100 KB
  and more each.

## Goals / Non-Goals

**Goals:** composite text correct for CID-keyed CFF and TrueType with
Identity and embedded CMaps; PDF output a viewer handles without any
predefined CMap beyond Identity; the glyph widening done once.

**Non-Goals:** FMapTypes 1–8; predefined CJK CMaps; `W2`; `cshow`;
Type 1 to CFF conversion.

## Decisions

**D1. The CMap model lives in `ps-fonts` and is built by the VM.**
`ps_fonts::cmap::CMap { name, wmode, codespaces: Vec<Range{len, low,
high}>, single: BTreeMap<(len, code), cid>, ranges: Vec<(len, low,
high, cid)>, notdef: Vec<…>, parent: Option<Rc<CMap>>, unicode_based:
bool }` with `decode(&[u8]) -> (len, Code, Option<Cid>)` per the
partial-match rule and `next` iteration. The `CIDInit` procedure set's
operators (`begincmap`, `endcmap`, `begincodespacerange`/`end…`,
`begincidrange`/`end…`, `begincidchar`/`end…`, `beginnotdefrange`/
`end…`, `beginbfchar`/`beginbfrange` accepted and stored for
completeness, `usecmap`, `CIDInit`'s `StartData`) build a `CMap` on a
builder kept on the interpreter between `begincmap` and `endcmap`;
`endcmap` produces a read-only dictionary (`CMapName`, `WMode`,
`CIDSystemInfo`, `CMapType`) with an internal id resolving to the
`Rc<CMap>` cached on the interpreter, and `defineresource` into `CMap`
stores the dictionary. `unicode_based` is true when the CMap name
contains `UCS2` or `UTF16`, the convention the Adobe orderings follow.
*Alternative:* interpret the CMap program lazily at show time — the
program is PostScript and must run through the interpreter anyway.

**D2. Identity CMaps are shipped files run through the interpreter.**
`crates/ps-fonts/data/cmap/{Identity-H,Identity-V}` with Adobe's
`LICENSE.md` and provenance (URL, commit, retrieval date, SHA-256);
`/Identity-H /CMap findresource` runs the file's text as a string
source with `CIDInit` in scope on first use and caches the result, so
the predefined and embedded paths are one code path. `resourcestatus`
reports 2 for the two names. *Alternative:* hard-code Identity in Rust
— less code than the parser exercise, but two paths to keep equal.

**D3. CID-keyed fonts are dictionaries with a cached program.**
`CIDFont` resource dictionaries carry `CIDFontType`, `CIDSystemInfo`,
`CIDCount`, `FontMatrix`, `FontBBox`, `FID`, and an internal program
id. Three loaders fill the cache: (a) FontSet `StartData` for CID-keyed
CFF, now also defining the resource; (b) `CIDInit` `StartData` for the
Type 1 charstring form — reads `GlyphData` binary of the declared
length, parses `CIDMap` entries (`GDBytes`, `FDBytes`, `CIDMapOffset`)
into CID → (font dictionary index, charstring range), and each
`FDArray` entry's `Private` (`Subrs`, `lenIV`) into a
`ps_fonts::cidfont::Type1CidProgram` interpreted with the existing Type
1 charstring engine; (c) `defineresource`/`definefont` of a
`CIDFontType 2` dictionary with `sfnts` and `CIDMap` (a string of
`GDBytes` per CID, or a dictionary/array form) into a `Program::TrueType`
with a CID → GID map. `Program` gains `glyph_by_cid` across variants
(CFF CID-keyed: charset; TrueType: the map; Type 1 CID: the map).
*Alternative:* a separate `CidFont` type outside `Program` — the show
frame and the backend want one snapshot type per font.

**D4. Type 0 fonts.** `definefont` accepts `FontType 0` with `FMapType
9`, a `CMap` dictionary (or name resolved through the category), an
`FDepVector` of font dictionaries (CIDFonts or, for completeness,
simple fonts), and an `Encoding` array of descendant indices (default
`[0]`); other map types are `invalidfont`. `composefont` builds that
dictionary and defines it. `FontKind::Composite { cmap: Rc<CMap>,
descendants: Vec<Descendant { dict, program, matrix }> }` in the show
frame, built at first show like other kinds.

**D5. Composite decoding in the show frame.** `advance` decodes
`codes[next..]` through the CMap to `(len, code, cid)`, selects the
descendant through `Encoding[font number]` (font number 0 unless the
CMap says otherwise), asks the descendant program for the advance and,
in outline mode, the outline, and emits `Glyph { code, len, cid, dx,
dy }` in the descendant's glyph units; `xshow`/`yshow`/`xyshow` consume
one displacement per decoded code. A notdef decodes to CID 0. Writing
mode 1: the displacement is `(0, -1000)` in glyph units (the default
vertical advance) and the glyph is positioned at the default vertical
origin (`w0/2, 880`), applied by the backend from the run's writing
mode. Simple fonts set `len = 1` and `cid = code`. Nested composites
(a descendant that is itself Type 0) are `invalidfont` (recorded).

**D6. The widened glyph.** `Glyph { code: u32, len: u8, cid: u16, dx:
f32, dy: f32 }` in `ps-vm`, mirrored in `ps-graphics`; `IrOp::Text`
gains `wmode: u8`. The dump prints `(bytes)` as before when every glyph
has `len == 1`, else `<hex>` with each code padded to its length, and
appends `wmode=1` to the `text` line only in vertical mode. `FontSpec`
gains `Composite { cmap_name, wmode, unicode_based, descendant:
Box<FontSpec-like Embedded data>, cid_to_code: BTreeMap<u16, (u32,
u8)> }` interned per page by CMap and descendant family; the CID → code
map feeds ToUnicode. Every pre-existing golden stays byte-identical
because simple-font runs print exactly as before.

**D7. PDF Type 0 output.** A composite resource becomes `/Type0
/Encoding /Identity-H|V /DescendantFonts [one]`, content strings of
two-byte CIDs (the run's CIDs, not its original codes), `W` built from
used CIDs' advances in thousandths through the descendant's font
matrix (`DW 1000`; consecutive CIDs grouped), `CIDSystemInfo` from the
CIDFont (or `Adobe/Identity/0` for TrueType). Descendants: CID-keyed
CFF subset (D8) as `CIDFontType0` + `FontFile3 /Subtype
/CIDFontType0C`; TrueType subset as `CIDFontType2` + `FontFile2` with a
`CIDToGIDMap` stream (two bytes per CID up to the highest used CID)
since the subset renumbers glyphs. ToUnicode: for a Unicode-based CMap
each CID maps to the code bytes it came from interpreted as UTF-16BE;
for TrueType descendants without one, the program's Unicode cmap
reversed by GID; else the entry is omitted. Fonts written at finish
with the document-wide used-CID set, as embedded simple fonts are.

**D8. CID-keyed CFF writer.** `cff::write::subset_cid` keeps used CIDs
plus CID 0, writes charset format 0 with CIDs, `FDSelect` format 3 over
the kept glyphs, an `FDArray` with only the font dictionaries used
(each with its pruned, renumbered local subroutines and its private
numbers), global subroutines pruned across all kept glyphs, and the
`ROS`, `CIDCount`, `FontBBox`, `FontMatrix` top entries. The round-trip
test parses the subset and compares every kept CID's outline and
advance.

**D9. Type 3 fallback for the Type 1 charstring CID form.** PDF has no
embedding form for it. The IR still records a composite run; the PDF
writer, seeing a descendant without an embeddable program, writes a
Type 3 font whose CharProcs are the used glyphs' outlines from the
engine (`d1` with the outline's bounds, then the path and `f`), with
`FontMatrix` the descendant's, an `Encoding` mapping consecutive codes
to CID-named glyphs, and content strings re-encoded to those one-byte
codes. Text stays selectable through ToUnicode when available.
*Alternative:* convert Type 1 charstrings to Type 2 — a real
translator (flex, seac, hint replacement, `div`) that a policy change
can add later; the fallback keeps output correct meanwhile.

**D10. Test fonts.** `testing::CffFont` already builds CID-keyed
programs; `testing::TrueTypeFont` gains a Type 42-style `CIDFontType 2`
wrapper with a `CIDMap` string; a new `testing::CidType1Font` builds the
`CIDInit` `StartData` form from named outlines through the Type 1
charstring encoder; corpus CMaps are small text programs written by
the generator. Binary sections in corpus files reuse the parse-survival
skip for `StartData`.

## Risks / Trade-offs

- [The glyph widening touches every text consumer] → mechanical, and
  the byte-identical golden check over 121 files is the regression net.
- [Codespace partial-match subtleties] → unit tests per rule case and
  the corpus mixed-length scenario.
- [Viewers differ on `CIDToGIDMap` and `W` edge cases] → the poppler
  checks (`pdffonts`, `pdftotext`, `pdftoppm`) run on every composite
  golden; grouping in `W` follows the simplest form.
- [Type 3 fallback loses hinting and font identity] → recorded as the
  trigger for the converter; the alternative is dropping the text.

## Open Questions

- Whether `composefont` should also accept a CMap given as a name of a
  not-yet-loaded predefined CMap beyond Identity once the asset change
  lands. No effect on this change.

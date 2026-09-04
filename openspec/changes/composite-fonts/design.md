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

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

### Part 1

Covers the CMaps (1.x) and the CID-keyed and Type 0 fonts with
composite decoding in the VM (2.x): the `cmap` and `cidfont` modules,
the `CIDInit` procedure set and the two new resource categories, the
shipped Identity CMaps, CIDFont dictionaries of the three loading forms,
`composefont`, the show frame over a CMap, and the widened `Glyph` with
the minimal adaptations that keep the IR and PDF crates compiling. The
IR resource, the dump's composite line, the CID-keyed CFF writer, the
PDF Type 0 output, and the Type 3 fallback are part 2.

- **CMap model (D1) as built.** `ps_fonts::cmap::{CMap, CMapBuilder,
  CMapError, Codespace, CidRange, BfRange, CidSystemInfo, Decoded,
  PREDEFINED, IDENTITY_H, IDENTITY_V, predefined}`. `CMap { name,
  wmode, system_info, codespaces: Vec<Codespace { len, low, high }>,
  single: BTreeMap<(len, code), (cid, font)>, ranges: Vec<CidRange {
  len, low, high, cid, font }>, notdef: Vec<CidRange>, bf_single,
  bf_ranges, parent: Option<Rc<CMap>>, unicode_based }` — an addition to
  D1's shape: every mapping records the descendant font number `usefont`
  was last given (0 otherwise), `system_info` is what `endcmap` read
  from `CIDSystemInfo`, and the `bf` entries are kept for a ToUnicode
  built from the job's own mapping (`CMap::bf(len, code)`, carrying and
  incrementing the destination's last byte over a range). `decode(bytes)
  -> Decoded { len, code, cid: Option<u16>, font }`: the codespaces
  (this CMap's own, then the parent chain's) are matched byte by byte —
  each byte of the code within the corresponding byte of the range's
  ends, so `<8140>–<FEFE>` excludes `<81FF>` — and the first range that
  contains the bytes fixes the length; failing that, the shortest range
  whose leading bytes match (at least the first) fixes the length of a
  notdef (`cid: None`, which the frame takes as CID 0); failing that,
  one byte; never more than the bytes left. Inside a codespace, `cid`
  is the single mapping, else the range (numeric containment, the CID
  offset saturating), else the parent's answer, else a notdef range's
  CID, else `None`. `decode_all` iterates. The builder rejects codes of
  zero or more than four bytes, range ends of different lengths or in
  the wrong order, and CIDs outside sixteen bits (`CMapError`, which
  the VM reports as `rangecheck`); `use_cmap` inherits the parent's
  writing mode until `WMode` is read; `unicode_based` is the name
  containing `UCS2` or `UTF16`.
- **Identity CMaps (D2) as built.** `crates/ps-fonts/data/cmap/
  {Identity-H,Identity-V,LICENSE.md}` from `adobe-type-tools/
  cmap-resources` at `f5cf3bca7fdfeaceb77aa82847e974f2306c20b4`,
  byte-identical, with `LICENSES/BSD-3-Clause.txt`, a `REUSE.toml`
  annotation, the provenance rows, `fetch-fonts` intake and `--check`,
  and the provenance test hashing them; embedded by `include_str!` and
  reachable as `cmap::predefined(name)`. Loading runs the file's text
  through the interpreter — in the same interpreter, not a nested one:
  an operator needing a predefined CMap not yet loaded (`findresource`,
  `usecmap`, `composefont`, `definefont` with a CMap name) pushes a
  `Marker::CMapLoad { name, global, retry }` and a string source above
  it and returns with its operands untouched; when the marker is
  reached the resource the program `defineresource`d moves out of the
  category dictionary into `Interp::predefined_cmaps` and the operator
  runs again (`Frame::Object` of its operator), finding it there. The
  load runs in global allocation mode, restored by `pop_frame` however
  the frame ends, so the dictionaries outlive every `save`. Identity-V
  chains to Identity-H through `usecmap`, so the nested load is
  exercised by the asset itself. `resourcestatus` reports 2 for the two
  names before and after loading, `resourceforall` lists them after the
  program's own, and `findresource` returns the same read-only global
  dictionary each time.
- **CIDInit (D1) as built.** `ops::cidinit::OPS`, `ProcSet`
  visibility, entered in the read-only `CIDInit` dictionary
  (`Interp::cid_init`) with `StartData`: `begincmap` pushes a
  `CMapBuilder` on `Interp::cmap_builders` (a stack, since a parent's
  load runs inside), every `begin…` pops its count and pushes a mark,
  every `end…` collects the objects above the innermost mark
  (`unmatchedmark` without one; a wrong count is `rangecheck`), and
  `endcmap` reads `CMapName` (name or string), `WMode`, and
  `CIDSystemInfo` (a dictionary, or the first of an array) from the
  current dictionary, builds the CMap, registers it on the interpreter
  (`register_cmap`/`cmap(id)`), and puts `/CodeMap` — a font-id object
  holding the id, which a program can copy but not forge — into that
  dictionary. `/CMap defineresource` requires `CodeMap` (`typecheck`
  otherwise), stores the dictionary in the category of the current
  allocation mode, and makes it read-only. `usecmap` resolves its name
  like `findresource`; `usefont` sets the builder's font number;
  `beginbfchar`/`beginbfrange` store their destinations (strings, names,
  or an array of them); `beginusematrix`/`endusematrix` are accepted
  and dropped; an operator outside `begincmap…endcmap` is `undefined`.
- **CIDFont dictionaries (D3) as built.** `ops::font::define`
  recognises a dictionary with `CIDFontType` whatever its `FontType`:
  type 0 needs a matrix, type 2 also `sfnts`; the `FID` is added, the
  dictionary made read-only, and it is registered in the `CIDFont`
  category (`Interp::cidfont_category`, local, and global in global
  mode) rather than a font directory, so `/X exch definefont` and `/X
  exch /CIDFont defineresource` are the same. The program cache is the
  `FID` one (`Interp::font_programs`): (a) FontSet `StartData` builds
  the dictionary for a CID-keyed CFF — `CIDFontType 0`, `FontType 9`,
  `CIDFontName`, `CIDSystemInfo` from the `ROS` (`Adobe-Identity-0`
  when absent), `CIDCount`, `FontMatrix`, `FontBBox`, `PaintType`,
  `FID` — caches the program under the `FID` and, as before, under the
  name for `Interp::cid_program`; the `FontSet` array still lists the
  name-keyed fonts only. (b) The `CIDInit` `StartData` form is `dict
  (Binary|Hex) count StartData` — the string naming the data's form
  precedes the count, and the dictionary the program built lies beneath
  (the change's task text had `count StartData`; the resource files
  have the string). The one `StartData` operator serves both procedure
  sets, dispatching on the operand before the count (a name: FontSet; a
  string: CIDFont). It reads `count` bytes (hexadecimal digits with
  whitespace for `(Hex)`), takes `CIDMapOffset`, `FDBytes` (0 means
  dictionary 0), `GDBytes`, `CIDCount`, and per `FDArray` entry
  `Private/lenIV` (default 4), `SubrMapOffset`/`SDBytes`/`SubrCount` or
  a `Subrs` string array, and `FontMatrix`, parses a
  `ps_fonts::cidfont::Type1CidProgram`, inserts `FontMatrix [0.001 0 0
  0.001 0 0]` when the dictionary has none (so `scalefont` and the
  composition below have a matrix to work with), allocates the `FID`,
  caches the program, and defines the resource under `CIDFontName`; the
  three operands stay in place until everything has parsed. Whether
  `StartData` pops the procedure set's dictionary is not pinned by the
  reference text this was built from; it does not, and the corpus files
  `end` explicitly. (c) `ops::embedded::snapshot` builds `CIDFontType 2`
  from `sfnts` and `CIDMap`: a string or string array of `GDBytes`
  (default 2) per CID over `CIDCount`, a dictionary of CID to glyph
  index, or an integer added to every CID (`truetype::CidMap::{Table,
  Offset}` on the `TrueTypeProgram`). A CIDFont used directly as the
  current font is `invalidfont` from the show operator.
- **`Type1CidProgram` as built.** `cidfont::{Type1CidProgram, CidLayout,
  FdLayout, SubrSource}`. Each font dictionary becomes its own
  `Type1Program` whose charstrings are keyed by the CID's two big-endian
  bytes, so the Type 1 engine, its subroutine handling, and its glyph
  cache serve unchanged; a CID whose map entry has an empty charstring
  is absent. A dictionary whose `FontMatrix` differs from the CIDFont's
  has its glyphs mapped by `fd × CIDFont⁻¹` into the CIDFont's space,
  once, at first request. API: `parse`, `cid_count`, `fd_count`,
  `fd_index(cid)`, `cids()`, `charstring(cid)`, `subrs(fd)`,
  `glyph_by_cid`. `Program` gained `Type1Cid`, `ProgramKind::Type1Cid`,
  `glyph_by_cid` across variants (TrueType through its CID map, CFF
  through the charset, Type 1 none), `is_cid_keyed`, and `cid_count`.
  `testing::CidType1Font` (`new`, `fd(len_iv, subrs)`, `fd_matrix`,
  `glyph`, `charstring`, `glyph_data() -> (bytes, CidLayout)`, `file()`,
  `program()`, `corpus()`) lays the data out as the CID map at offset 0
  (one byte of dictionary index, three of offset), then each
  dictionary's subroutine map and subroutines, then the charstrings;
  `corpus()` has dictionary 0 with `lenIV` 4 and dictionary 1 with
  `lenIV` 1 and one subroutine. `testing::TrueTypeFont::cidfont_type2`
  writes the Type 42-style wrapper with a two-byte `CIDMap` string;
  `testing::corpus_cid_cff()` is `SynCID` (CIDs 1, 2, 34, 35, 200 across
  two dictionaries, a 250-unit notdef); `testing::corpus_cmap()` is the
  `Syn-H` program.
- **Type 0 fonts (D4) as built.** `definefont` accepts `FontType 0`
  with `FMapType 9` (anything else, or none, is `invalidfont`), a `CMap`
  that is a CMap dictionary or the name of a defined or predefined one
  (an unknown name is `invalidfont`; a predefined one not yet loaded
  loads first, through the retry), a non-empty `FDepVector` of defined
  fonts none of which is itself `FontType 0`, and an `Encoding` of
  indices into it — absent means `[0]`, the decision the change left
  open, since `composefont` always writes one and a hand-built font
  without one has nothing else sensible to mean. `composefont` builds
  `FontType 0`, `FMapType 9`, `FontMatrix [1 0 0 1 0 0]`, `FontName`,
  `CMap` (the dictionary), `FDepVector` (a copy of the array),
  `Encoding [0 1 …]`, and `WMode` from the CMap, defines it under the
  key, and returns it; an empty array is `rangecheck`. Composition of
  matrices happens at `setfont`: the `FontRef` the graphics state holds
  carries `descendant.FontMatrix.then(Type0.FontMatrix)` for the
  descendant font number 0 selects (`font::effective_matrix`), so the
  backend's advance, `stringwidth`, and every displacement conversion
  work with one matrix; `currentfont` still returns the dictionary with
  its own.
- **Composite decoding (D5) as built.** `FontKind::Composite(Composite
  { cmap, numbers, descendant: Descendant { dict, encoding, kind:
  Cid { program, scale } | Simple(FontKind) } })`, built at the start
  of each show like the other kinds. The frame decodes the next code
  at its byte position (`next`) with `CMap::decode`, keeps a glyph
  counter (`glyphs`) the positioning variants index by, and a
  `last_code` for `kshow`, which now hands its procedure the decoded
  codes. A CIDFont descendant answers `glyph_by_cid(cid)`, else CID 0,
  else a zero-width blank; a simple descendant (D4's "for
  completeness": resident or embedded, not Type 3) takes the CID as a
  one-byte code through its own `Encoding`, a CID above 255 being its
  notdef. Only the descendant of font number 0 is drawable: a code
  whose font number selects another descendant is `invalidfont` from
  the operator, mid-run (the operands are already gone, as for any
  error inside a show frame), since the backend is described one
  descendant per instance; the CMap model records the font numbers so
  this can grow. `glyphshow` in a Type 0 font is `invalidfont`.
  `xshow`/`yshow`/`xyshow` check their counts against the decoded
  glyph count. Writing mode 1: the displacement is `(0, −em)` in the
  descendant's glyph units — 1000 for charstring programs, 1 for
  TrueType after the scale to the unit em — and `charpath` shifts each
  outline by `(−w0/2, −0.88 em)` so the glyph hangs from its vertical
  origin at the current point; the backend will position shown runs the
  same way from the run's writing mode (part 2).
- **Boundary (D6) as built.** `ps_vm::Glyph { code: u32, len: u8, cid:
  u16, dx, dy }` with `Glyph::simple(code, dx, dy)` (`len 1`, `cid =
  code`) and `code_bytes()`; the IR reuses the VM's type.
  `FontSource::Composite { family, cmap_name, wmode, unicode_based,
  cmap: Rc<CMap>, descendant: Box<FontSource> }` — the CMap itself is
  carried (compared by pointer) so a backend can build ToUnicode from
  its `bf` entries or, when `unicode_based`, from the code bytes; the
  descendant is `FontSource::Embedded { family: the CIDFont's FID,
  kind, program, font_matrix: the CIDFont's own matrix, font_name:
  CIDFontName }` for a CIDFont and the simple font's own source
  otherwise. `FontInfo.encoding` is all `None` for a composite. The
  recording mock gained `Recording::with_fonts` and a side list of the
  `FontInfo`s received. `ps_graphics::Graphics::font_resource` answers
  `invalidfont` for a composite source, so a `show` in a Type 0 font
  under the real backend is `invalidfont` until part 2; the dump
  already writes a run's codes as `<hex>` when any glyph's `len` is
  more than one and `(bytes)` exactly as before otherwise, and prints
  `cidtype1` for the new program kind; `remelt` takes `code as u8`
  (only simple fonts reach it) and describes a `Type1Cid` program
  without a stream. Every pre-existing golden is byte-identical.
- **Resource categories.** `CMap` and `CIDFont` join the `Kind` enum
  with local and global dictionaries (`Interp::{cmap_category,
  cidfont_category}`); `ProcSet` lists `CIDInit` before `FontSetInit`,
  which the `procset-status` corpus file now expects.
- **Corpus.** `corpus/unit/fonts/`: `cmap-embedded`,
  `cmap-identity-predefined`, `cmap-category-listing`,
  `cidfont-fontset`, `cidfont-type2-cidmap`, `cidfont-type1-charstrings`
  (binary glyph data after `(Binary) <count> StartData`, which
  `parse-survival`'s existing skip covers — the count precedes the
  operator either way), `composefont`, `type0-older-map-types`,
  `composite-two-byte-width`, `composite-vertical-width`, and
  `composite-partial-match`; the generated ones come from
  `tests/corpus_fonts.rs`. The three composite-text files measure with
  `stringwidth` (`12.0 0.0`, `0.0 -10.0`, `14.5 0.0`) because a `show`
  in a Type 0 font cannot reach the page yet; part 2 rewrites them to
  the spec's `show … currentpoint` form with `.ir` and `.pdf` goldens
  and adds the mixed-length and dump scenarios, which part 1 covers in
  `crates/ps-vm/tests/composite.rs` through the mock.
- **Verification.** `cargo test --workspace`: 624 passed, 2 ignored
  (596 before); `cargo test -p ps-fonts --no-default-features`: 116
  passed; clippy clean on all targets with and without the feature;
  `cargo fmt --check` clean; `difftest run` 132 of 132 (121 before);
  `parse-survival` 132 files, no failures; `fetch-fonts --check` every
  file ok; `openspec validate composite-fonts` valid.
- **For part 2.** The IR resource takes `FontSource::Composite` as it
  is: intern by `cmap_name`/`wmode` and the descendant's snapshot;
  `IrOp::Text` needs `wmode` and the vertical positioning from D5's
  `(w0/2, 880)`; the dump's `text` line already prints hexadecimal
  codes, the `font … composite …` line is missing. ToUnicode: for a
  Unicode-based CMap `Glyph::code_bytes()` is UTF-16BE; `CMap::bf`
  gives the job's own destinations. The CID subset writer reads the
  unchanged `CffProgram` API (`charset`, `fd_index`, `private_for`,
  `privates()`, `gid_of_cid`); `CIDToGIDMap` comes from
  `TrueTypeProgram::cid_map()`; the Type 3 fallback reads
  `Type1CidProgram::{cids, glyph_by_cid}` through
  `Program::glyph_by_cid`, with the text's CIDs from `Glyph::cid`. The
  three composite-text corpus files and the `procset-status` expectation
  are the only corpus files to touch; `corpus_cid_cff`, `corpus_cmap`,
  and `CidType1Font::corpus` are the fonts to reuse.

### Part 2

Covers the IR resource and its dump forms (3.1), the CID-keyed CFF
writer (4.1), the PDF Type 0 output (4.2), the Type 3 fallback (4.3),
and the corpus with its goldens (5.x).

- **IR (D6) as built.** `FontSpec::Composite { cmap_name, wmode,
  unicode_based, descendant: Box<FontSpec>, cid_to_code: BTreeMap<u16,
  (u32, u8)> }`, the descendant an `Embedded` snapshot (family the
  CIDFont's `FID`, kind, `CIDFontName`, the CIDFont's own matrix, the
  program, an empty encoding). `IrOp::Text` gained `wmode: u8`, which
  the backend copies from the composite source (0 for every other
  kind) and `transformed` carries through a Type 3 capture.
  `FontSpec::same_font` compares a composite by everything but
  `cid_to_code`, and is what `Graphics::resource_for` interns by — one
  resource per CMap name, writing mode, and descendant snapshot per
  page, whatever the instance — and what the PDF table shares fonts
  across pages by; `show` then enters each glyph's `(cid → (code, len))`
  into the resource, first code kept. `FontSpec::cid_glyph(cid)`,
  `cid_width(cid)` (advance × `1 / units_per_em` for TrueType, as
  `width` scales), and `glyph_width(&Glyph)` (by CID for a composite,
  by `cid as u8` otherwise, which is the code for a simple font) serve
  the content writer; `encoding()` and `font_matrix()` answer with the
  descendant's. Vertical positioning needs no arithmetic in the
  backend: the run's matrix already places the pen, which in mode 1 is
  each glyph's vertical origin `(w0/2, 0.88 em)` — the model PDF's
  `Identity-V` with the default `DW2` applies itself — so the IR's
  contract is the semantics of `wmode`, documented on `IrOp::Text`.
- **A simple descendant.** D4's "for completeness" descendant (a
  resident or embedded simple font under a Type 0 font) is recorded as
  that font's own resource: the run keeps its two-byte codes and the
  CID is the descendant's one-byte code, so the dump prints `<0048>`
  over `font 0 Helvetica`, and the content writer, which now takes a
  simple font's code from `Glyph::cid` (equal to the code for every
  simple-font run), writes `(H)`. For this the one change to `ps-vm`:
  `show::describe` describes a composite with the simple descendant's
  `Encoding` rather than the Type 0 font's index array (a CIDFont
  descendant still gets all `None`), amending part 1's "`FontInfo.
  encoding` is all `None` for a composite". A simple descendant shown
  in writing mode 1 is drawn at its horizontal origin with `Td` moves
  for the vertical advances (PDF has no vertical simple fonts); recorded
  as a limit.
- **Dump.** `font <n> composite <CMapName> wmode=<m> <cff|truetype|
  cidtype1> <FontName> glyphs=<count>`; `text … <hex> <dx> <dy>…` with
  ` wmode=1` appended only in vertical mode. `Program::glyph_count` of
  a TrueType program with a CID map is now its glyph count (it was the
  Type 42 name count, zero for a `CIDFontType 2`), the one change to
  `ps-fonts` outside the writers. Every pre-existing golden is
  byte-identical (132 of 132 before the new files were added).
- **CID-keyed CFF writer (D8) as built.** `cff::write::subset_cid(
  program, font_name, &BTreeSet<u16>)`. Kept glyphs: glyph 0 and the
  glyph of every CID the program has, written in CID order (CID 0 is
  glyph 0 of a CID-keyed program), the charset in format 0 of CIDs, the
  select table in format 3 over the new indices with the font
  dictionaries renumbered densely in old order over those the kept
  glyphs run under. Layout: header, name index, top dictionary, string
  index, global subroutines, charset, select table, charstrings, the
  font dictionary array (each entry a five-byte `Private` pair), then
  each kept dictionary's private entries (verbatim, `Subrs` excluded)
  with a five-byte `Subrs` offset and its local index directly after.
  The top dictionary opens with `ROS` (registry and ordering
  re-interned), then the copied strings and entries of the name-keyed
  writer (`FontBBox`, `FontMatrix` when present, …), `CIDCount`, and
  the four offsets. Pruning traces each dictionary's kept glyphs
  separately (`Reached` per dictionary), renumbers local subroutines
  per dictionary and global ones over the union, and rewrites every
  kept charstring with its dictionary's numbering; a global subroutine
  is rewritten with the local numbering of the one kept dictionary, or
  with none when several are kept, since a `callsubr` inside it means
  a different subroutine under each — so a font whose global
  subroutine calls a local one and whose kept glyphs span dictionaries
  takes the stub tier (counts kept, unreached subroutines as `return`),
  as does a mask two dictionaries' traces size differently; a glyph
  that cannot be interpreted keeps everything, as before. Round-trip
  tests cover the corpus font's two dictionaries (both kept, one kept,
  CID 0 alone, an unknown CID), a font with local subroutines in each
  dictionary and two global ones, and the global-calls-local case in
  both tiers. Sizes: the corpus `SynCID` program is 249 bytes; CIDs 1
  and 2 (both dictionaries) subset to 188, CID 1 alone (one) to 149,
  CIDs 34, 200, 35 to 212. No CID-keyed production program is among the
  test assets (TeX Gyre Pagella is name-keyed); the writer's structural
  correctness rests on the reader that parses every subset back.
- **Type 0 output (D7) as built.** `remelt::composite`, sharing
  `EmbeddedTable` and its finish-time writing: `CompositeFont { spec
  (the composite with `cid_to_code` merged over every page), font,
  cids, codes }`, found by `same_font`. `write_font` writes the
  descendant first: a `CIDFontType0` with `FontFile3 /Subtype
  /CIDFontType0C` from `subset_cid` (a program the writer refuses —
  only a name-keyed one — is described without a stream, as an
  unembeddable simple font is), `CIDSystemInfo` from the program's
  `ROS` (`Adobe`/`Identity`/0 without one); or a `CIDFontType2` with
  `FontFile2` (`Length1`) from the TrueType subsetter over the CIDs'
  glyph indices through `cid_map()` (CID without one: glyph 0), no
  code map (an empty `(3,0)` subtable — poppler reads the font without
  one), and a `CIDToGIDMap` stream of two bytes per CID from 0 to the
  highest used, the subset's new index for a used CID and 0 elsewhere.
  The map is always a stream: the subsetter renumbers densely, so the
  identity form never applies; `truetype::write::subset_with_map`
  returns the old-to-new map beside the bytes and `subset` is now a
  wrapper over it. Descriptor as for embedded simple fonts: `Flags`
  symbolic (4) plus fixed pitch, box through the descendant's matrix
  in thousandths, `ItalicAngle`, `Ascent`/`Descent` from the box,
  `StemV` from the first font dictionary's `StdVW` (CFF) else 80. `DW
  1000`; `W` from the used CIDs' `cid_width` × `FontMatrix.a` × 1000
  (rounded to a thousandth) as `c [w …]` over runs of consecutive CIDs.
  The Type 0 dictionary: `Type0`, `BaseFont` the tagged name (the tag
  hashes the font name and the used CIDs as big-endian bytes; the CMap
  name is not appended), `Encoding /Identity-H` or `/Identity-V` by
  `wmode`, `DescendantFonts [one]`, `ToUnicode` when any CID maps.
  ToUnicode sources, in order: a Unicode-based CMap gives each CID its
  code bytes as UTF-16BE (a one- or three-byte code is padded with a
  leading zero byte; a code that is not valid UTF-16, such as a lone
  surrogate, maps nothing); a TrueType descendant's `(3,1)` cmap read
  backwards from the CID's glyph; else the entry is omitted (a CFF
  under `Identity-H`, as in most goldens — `CMap::bf` is carried by
  the VM's source but not by the resource and is not used). The CMap
  text is `to_unicode_text(len, mapped)`, the one-byte writer's body
  generalised to a `<0000> <FFFF>` codespace and four-digit sources.
- **Content writer.** A composite run's `Tm` is the run's matrix back
  through the descendant's matrix (as for embedded fonts); the string
  is the CIDs, two bytes each, always in hexadecimal; positions are
  tracked along a main axis (vertical in mode 1) and a cross axis: a
  difference along the main axis is a `TJ` adjustment (in mode 1 PDF's
  own advance is the default vertical advance, one text-space unit down),
  across it a `Td`. Composite runs inside Type 3 glyph procedures count
  toward the font's CIDs as simple-font runs did. `content::render`
  takes a `Recode` (font index → CID → one-byte code) that
  `write_fonts` returns beside the font references and `Objects`
  carries to the page's stream.
- **Type 3 fallback (D9) as built.** A descendant of kind `Type1Cid`
  (or, defensively, `Type1`) is written as `Type3`: `FontMatrix` the
  descendant's, one CharProc per used CID named `cid<N>`, opening `wx 0
  llx lly urx ury d1` over the outline's control box and rendered
  through the content writer from an `IrOp::Fill` of the outline's
  segments (`m`/`l`/`c`/`h`, then `f`; nothing after `d1` for an empty
  outline), `Encoding` `Differences` from the codes, `FirstChar`/
  `LastChar`/`Widths` (glyph space, `0 0 [0]` when nothing was shown),
  `FontBBox` the union of the CharProcs' boxes, `Resources << >>`, and
  ToUnicode over the one-byte codes when the composite has a source.
  Codes are assigned as pages arrive — the document-wide font object
  and the page's content stream, written before the document ends,
  must agree — in order of first use across pages and in CID order
  within a page, from 1; a 256th distinct CID is noted (`page N: CID …
  is beyond the 255 glyphs a Type 3 fallback font holds`) and shown as
  code 0, the notdef. In writing mode 1 every CharProc is drawn shifted
  by `(−w0/2, −880)` (charstring units, the em being 1000) with width
  0, so PDF's own advance is nothing and the writer emits a `Td` per
  glyph for the recorded vertical displacement; the content writer
  treats a recoded run as horizontal for its axis rule.
- **Corpus.** `corpus/unit/fonts/`: `composite-two-byte-width`
  (`12.0 0.0`), `composite-vertical-width` (`0.0 90.0`), and
  `composite-partial-match` (`14.5 0.0`) now `show … currentpoint` and
  produce pages; new `composite-mixed-lengths` (the `<41814042>` dump
  scenario), `composite-dump` (`<00010002>` at (72, 700) with the
  `font 0 composite Identity-H wmode=0 cff SynCID glyphs=6` line),
  `cidfont-type2-embedded` (CIDs 3 and 4 over `SynCIDTT`),
  `cmap-ucs2-tounicode` (`Syn-UCS2-H`: `<0041>` and `<0042>` to CIDs 1
  and 2), `cidfont-type1-fallback` (CID 2 of `SynCIDT1`, the glyph
  drawn through dictionary 1's subroutine), and
  `composite-charpath-fill`; all generated by `tests/corpus_fonts.rs`
  with `.ir` and `.pdf` goldens. Poppler on every composite golden:
  `pdffonts` lists `CID Type 0C … Identity-H emb yes sub yes` (`uni yes`
  for the UCS2 file, `Identity-V` for the vertical one), `CID TrueType
  … Identity-H emb yes sub yes` for the CIDFontType2, and `Type 3
  Custom` for the fallback; `pdfinfo` accepts all 138 corpus documents
  as `EFTERSCRIPT_PDF_CHECK`; `pdftotext` extracts `AB` from the UCS2
  golden (and the CIDs as characters where no ToUnicode exists);
  `pdftoppm` renders every file, the vertical glyph at (−2..2,
  91.2..95.2) as the VM's `charpath` places it.
- **Scenario tests.** `crates/ps-graphics/tests/{backend,scenarios}.rs`
  (interning, `cid_to_code`, the dump lines, the simple descendant,
  mixed lengths, the writing mode, the composite `charpath`);
  `crates/remelt/tests/{scenarios,sink}.rs` (the four embedding
  scenarios with the CFF subset parsed back and every kept CID's glyph
  compared, the `CIDToGIDMap` bytes, ToUnicode text, the fallback's
  CharProc text and code assignment across pages, `TJ`/`Td` along and
  across the writing direction, sharing across pages); the mock-based
  VM tests of part 1 are unchanged.
- **Deviations and limits.** D6's "interned per page by CMap and
  descendant family" is by `same_font`, which compares the descendant
  snapshot (family included) and the writing mode too. A simple
  descendant in writing mode 1 is not shifted to its vertical origin.
  The Type 3 fallback holds 255 glyphs per composite font and loses
  hinting, as D9 accepts. `W2` is not written (the default `DW2` is what
  the VM's advance and origin implement). Nothing beyond `Identity-H`
  and `Identity-V` is ever named in the PDF, whatever CMap the job used.
- **Verification.** `cargo test --workspace`: 645 passed, 2 ignored
  (624 before); `cargo test -p ps-fonts --no-default-features`: 119
  passed, 1 ignored; clippy clean on all targets with and without the
  feature; `cargo fmt --check` clean; `difftest run` 138 of 138 with
  `EFTERSCRIPT_PDF_CHECK=pdfinfo`; `parse-survival` 138 files, no
  failures; `openspec validate composite-fonts` valid.

# Design: Font programs

See proposal.md for motivation and the spec deltas for the contract. This
document fixes how a font program gets in (`eexec`), how its glyphs come
out (the glyph engine), how the program travels to the PDF, and how it
is subset.

## Context

- A Type 1 font program is PostScript: cleartext that builds the font
  dictionary, an `eexec` section that fills `Private` (with `Subrs`,
  `lenIV`, hinting values) and `CharStrings` (glyph name → encrypted
  charstring), then `definefont`. The interpreter already runs
  everything except `eexec`, which is registered and raises `undefined`.
  `RD`-style procedures read charstring bytes with `readstring` from
  `currentfile`, so the decrypting layer must be a real file.
- The file table holds entries with a one-byte pushback and a position;
  the scanner reads files through it, and `currentfile` finds the
  innermost file frame. A layered entry fits without changing the
  scanner.
- A Type 42 dictionary carries the TrueType program in `sfnts` (an array
  of strings, split at glyph boundaries) and `CharStrings` mapping glyph
  names to glyph indices.
- `text-core` fixed: the font in the graphics state as an instance id
  and matrix, `FontInfo`/`FontSource` describing a font to the backend
  once per instance, `Text` runs with glyph-space displacements, font
  resources interned per page, `PdfSink` writing font objects lazily,
  and `charpath` raising `invalidfont`.
- No outline files for the resident fonts ship yet; `ps-fonts` must not
  depend on `ps-vm`.

## Goals / Non-Goals

**Goals:**
- Jobs with embedded Type 1 and Type 42 fonts run, measure, draw, and
  distil with their own glyphs embedded.
- A glyph engine interface (`program + glyph name → outline + advance`)
  that the composite-font change reuses for CFF and CID fonts.
- Subsetting that is correct by construction: the regenerated Type 1
  program is checked by running it through this interpreter.

**Non-Goals:**
- Rendering-quality hinting (hints are dropped from the interpretation,
  kept in the embedded program).
- Font caching for speed beyond a per-font outline cache.
- Recovering the original program bytes; the PDF gets a regenerated
  program (D6).

## Decisions

**D1. `eexec` is a layered file-table entry.** `FileTable` gains an
entry kind that reads from a base entry, drops the first four bytes,
detects hexadecimal form (all of the first four bytes are hexadecimal
digits) versus binary, and decrypts with the standard key, byte by byte,
so the base's position stays exact: on `closefile` of the layer the base
has consumed precisely the bytes the layer decrypted (a pushed-back byte
is returned to the base). `eexec` on a file pushes a `SourceSlot::File`
frame for the layer with `systemdict` on the dictionary stack, and a
marker frame that pops `systemdict` and closes the layer when the frame
ends, whether by `closefile`, end of data, or an error unwinding through
it. `eexec` on a string decrypts it whole and executes it as a string
source. *Alternative:* decrypt the rest of the file into a buffer and run
it as a string — breaks `currentfile`, and the binary section's end is
only known by executing it.

**D2. Where the program lives.** The font dictionary is the source of
truth, as the reference has it: charstrings and subroutines are strings
in `CharStrings` and `Private/Subrs`. On the first glyph request for a
font instance family (`FID`), the VM builds a `ps_fonts::Program`
snapshot — Type 1: `lenIV`, the subroutines, and the charstrings by
name, decrypted once; Type 42: the concatenated `sfnts` bytes and the
`CharStrings` name → index map — wrapped in `Rc` and cached on the
interpreter by `FID`. A program that later mutates its font dictionary
is not followed, the same rule `text-core` set for encodings.
*Alternative:* re-read the dictionary per glyph — cheap for Type 1,
prohibitive for `sfnts` assembly.

**D3. The glyph engine.** `ps_fonts::Program` exposes `glyph(name) ->
Option<Glyph { advance: (f32, f32), outline: Outline }>` in the
program's glyph space (Type 1: charstring units under the font's
matrix; TrueType: font units, with the engine reporting `units_per_em`
so the VM scales to the 1000-unit convention the font matrix expects:
Type 42 fonts have `FontMatrix [1 0 0 1 0 0]` and glyph space is the
unit em, so outlines and advances are divided by the units per em).
`Outline` is a list of move/line/curve/close in `f32`. The Type 1
interpreter implements the charstring operators, `callsubr`,
`callothersubr` for flex (other-subroutines 0–2, reconstructing the two
curves from the collected points) and hint replacement (3, ignored),
`div`, `pop`, `seac` by looking up the two component names through
`StandardEncoding` and composing with the accent displacement, `sbw` and
`hsbw` for the advance. TrueType glyphs: `loca`/`glyf` with composite
glyphs resolved (offsets and scales), on/off-curve points to quadratic
segments to cubics (exact elevation). A per-family cache
`name → Rc<Glyph>` lives beside the program. *Alternative:* a trait
object per font type — the two programs are an enum with the same three
methods; a trait adds nothing until CFF, when the enum grows a variant.

**D4. Show and charpath use the engine.** For a Type 1 or Type 42 font
the show frame's resident branch generalises: per glyph, advance from
the engine (through the font matrix like a resident width), the run to
the backend as before. `charpath` runs the same frame in an outline
mode: for each glyph it appends the outline to the current path through
`backend.moveto/lineto/curveto/closepath` after mapping glyph space
through the font matrix and translating to the current point (the
backend applies the CTM as usual), then advances. The boolean is accepted
and ignored for `PaintType 0`; a `PaintType 2` (stroked) font is treated
as outline too, recorded. Resident and Type 3 fonts keep `invalidfont`.

**D5. Font info to the backend.** `FontSource` gains `Embedded {
family, kind: Type1 | TrueType, program: Rc<Program>, font_matrix }`.
The program is immutable and shared, which is the value-type rule's
intent; a backend that does not embed can ignore it. `ps-graphics`
interns the resource per page by `family` and encoding as for Type 3.
The dump prints kind, `FontName`, and glyph count only.

**D6. Type 1 embedding regenerates the program.** `ps_fonts::type1::write`
produces a complete Type 1 program from the snapshot plus the entries
the PDF consumer needs: cleartext `FontName`, `FontType`, `FontMatrix`,
`FontBBox`, `PaintType`, `Encoding` (the font's own array, written as
`dup <code> /<name> put` lines), `FontInfo` when present; the encrypted
section with the `Private` numeric and array entries written back
through the VM's syntactic printer (`BlueValues`, `StdHW`, `StdVW`,
`StemSnap*`, `BlueScale`, `ForceBold`, `LanguageGroup`, `lenIV`),
`OtherSubrs` printed back from the dictionary the same way (the
procedures the font brought in, unchanged), `Subrs` re-encrypted, and
the kept `CharStrings` re-encrypted with `lenIV 4`; then the 512-zero
trailer and `cleartomark`. The three lengths are the cleartext,
binary-encrypted, and trailer sizes. Correctness check: the test suite
feeds the regenerated program back into the interpreter and compares
glyph advances and outlines with the original. *Alternative:* capture
the original bytes as they pass through `eexec` — the cleartext before
`eexec` is already consumed by the scanner and its start is unknown, and
a job that alters the dictionary would embed a program that disagrees
with what it showed.

**D7. TrueType subsetting rewrites tables.** Keep glyph 0 and the used
glyphs (and their composite components), renumber densely, rewrite
`glyf`/`loca`/`hmtx`/`hhea`/`maxp`/`head`/`post` (format 3, no names),
synthesise a `cmap` with one (3,0) subtable mapping each used code to
its new index, and copy `cvt`/`fpgm`/`prep` when present so hinted
programs stay valid. The font dictionary is `/TrueType` with the
symbolic flag and no `Encoding`, so viewers use the cmap; widths are
listed per used code. The writer is shared with the synthesised test
fonts. *Alternative:* embed the whole program — correct, wasteful, and
still needs the cmap synthesis because Type 42 addresses glyphs by
name, not by the program's own cmap.

**D8. Glyph usage and finish-time fonts.** `PdfSink` records, per
embedded family, the set of codes and glyph names shown across pages
and allocates the font object id at first use; the dictionary,
descriptor, and program stream are written in `finish`. Resident and
Type 3 fonts keep their lazy path. The document stays deterministic
(sets are ordered).

**D9. Descriptors.** `FontBBox` from the dictionary (Type 1) or `head`
(TrueType), `ItalicAngle` from `FontInfo` or `post`, `StemV` from
`Private/StdVW` or a fixed default for TrueType, `Ascent`/`Descent`
from the bounding box, `Flags`: symbolic unless the encoding is the
standard one; fixed pitch from `FontInfo/isFixedPitch` or `post`.
`CapHeight` when `FontInfo` has it.

**D10. Synthesised test fonts.** `ps_fonts::testing` (a `cfg(test)`-
free module used by tests of several crates) builds a Type 1 program
from named outlines (a charstring encoder for the few operators the
synthesised glyphs need) and a TrueType program from quadratic
contours through the D7 writer. The corpus fonts under
`corpus/unit/fonts/` are generated once by an ignored test and
committed as hexadecimal `eexec` programs and `sfnts` arrays; they are
the project's own work. A test embeds the host's DejaVu Sans when
present, checking parse, subset, and round-trip, skipping otherwise.

**D11. Errors.** A charstring that runs past its end, an unknown
operator, a `seac` component missing, a `Subrs` index out of range, a
`sfnts` too short for its tables, or a glyph index out of range raises
`invalidfont` from the operator that needed the glyph; `eexec` on a
closed file raises `ioerror`.

## Risks / Trade-offs

- [Regenerated Type 1 programs differ from the originals in byte layout;
  a consumer that fingerprints fonts sees a new font] → the `FontName`
  and `UniqueID` are dropped or tagged with the subset prefix, which is
  what subsetting implies anyway.
- [Flex and `seac` corner cases in real fonts] → the corpus covers the
  documented forms; a hellbox-tier test over harvested jobs is the
  signal for more, with clean-room reductions added to the corpus.
- [Layered file semantics around `closefile` and pushback] → pinned by
  the `eexec` scenarios including one that reads with `readstring`
  across the boundary and one that ends by end of data.
- [The `Rc<Program>` in `FontSource` crosses the value-type boundary]
  → the program is immutable and the backend only reads it; the
  alternative, an opaque handle, would need a lookup callback the trait
  does not have.

## Open Questions

- Whether `Subrs` should be pruned to those reachable from kept
  charstrings. Not needed for correctness; can follow if embedded sizes
  matter.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

### Part 1

Covers `eexec` (1.x), the glyph engine with its writer and builders
(2.x), and the VM side (3.x): program snapshots, the show family and
`stringwidth` over embedded fonts, and `charpath`. The IR resource, the
dump line, subsetting, and embedding are part 2.

- **The layered entry (D1) as built.** `FileTable::open_layer(base)`
  adds an entry of kind `Layer` beside the stream entries; the entry's
  one-byte pushback became a small stack (`Vec<u8>`) so a close can hand
  several bytes back. The form is decided when the first byte is needed:
  four base bytes are read, all hexadecimal digits means hex, and the
  four are returned to the base so decryption sees them as the section's
  start. Hex decoding skips whitespace between digits; the first byte
  that is neither ends a hex section and goes back to the base together
  with the whitespace read before it, so `closefile` leaves the base at
  the byte after the last hex digit consumed. A binary section ends with
  the base. Four plain bytes are dropped. The layer keeps the raw base
  bytes behind the one plain byte it has peeked, and `close` returns
  them with the base's position adjusted; a layer whose peek reached the
  end of the section has nothing to return. `read`/`peek`/`consume`/
  `position` count plain bytes; `write` on a layer is `ioerror`;
  `close_from` now closes newest entries first, so a layer closes before
  its base. The scanner consumes exactly one whitespace byte after the
  `eexec` token: a section preceded by a CR LF pair sees the LF as its
  first byte and is taken as binary. That is the reference's rule as
  well; a compatibility case for lenient producers is a quirk to add,
  not done here.
- **`eexec` operator.** A file operand needs read access (a closed one
  is `ioerror`); the layer's file object is read-only in the current
  allocation space, so a layer opened after `save` is closed by
  `restore`. `systemdict` is pushed (a full dictionary stack fails with
  the operand still in place), then an uncounted `Marker::Eexec {
  layer, dicts }` frame and the `SourceSlot::File` frame. The cleanup
  runs in `Interp::pop_frame`, whichever way the marker leaves the
  stack: the dictionary stack is cut back to the depth recorded before
  the push (not "pop one", so an unbalanced `begin` inside the section
  cannot leave the wrong dictionary on top) and the layer is closed if
  `closefile` did not do it. The execution loop pops a file frame whose
  file is closed — the rule the run file already had, now for every
  `File` slot — and pumps the run source for file frames too. A string
  operand is decrypted whole (`ps_fonts::type1::decrypt_section`) into
  a string object executed as a `SourceSlot::String` under the same
  marker. `systemdict` is read-only, so the spec's literal `/x 42 def`
  inside a section is `invalidaccess`, as the reference has it; the
  corpus files and tests write `userdict /x 42 put` (a font program
  begins its own dictionaries). The `standard-files` corpus file, which
  pinned `eexec` as undefined, now checks `42 eexec` is `typecheck`.
- **Corpus.** `corpus/unit/fonts/`: `eexec-hex`, `eexec-string`,
  `type1-width`, `type1-seac-advance`, `type1-malformed-charstring`,
  `type42-advance`, `charpath-advance`, generated by the ignored test
  `generate_corpus_fonts` in `crates/ps-fonts/tests/corpus_fonts.rs`
  and checked against the generator by `corpus_fonts_are_current`. The
  binary `eexec` scenario is a Rust test only
  (`crates/ps-vm/tests/eexec.rs`): `cargo xtask parse-survival` scans
  every corpus file as tokens and binary cipher bytes are not tokens.
  None of the files produces a page, so none has a golden; the seac
  scenario measures with `stringwidth` rather than `glyphshow` because
  the graphics crate cannot record embedded text until 4.1.
- **Glyph engine API (D3).** `ps_fonts::{Outline, OutlineOp, Glyph,
  Program, ProgramKind, FontError, Type1Program, TrueTypeProgram}`.
  `Outline { ops: Vec<OutlineOp> }` with `MoveTo`, `LineTo`, `CurveTo`
  (two controls and the end), `Close`, plus `map`, `translated`, and
  `control_box`. `Glyph { advance: (f32, f32), outline }`. `Program`
  is the enum `Type1(Type1Program) | TrueType(TrueTypeProgram)` with
  `kind()`, `glyph(name) -> Result<Option<Rc<Glyph>>, FontError>`
  (`Ok(None)` for a name the program lacks, `Err` for one it cannot
  interpret), `has_glyph`, `glyph_count`, `glyph_names` (sorted), and
  `units_per_em` (`None` for Type 1). Each program caches interpreted
  glyphs by name in a `RefCell<HashMap>`, so a shared `Rc<Program>`
  needs no mutable access. `FontError` variants name the fault
  (`Truncated`, `UnknownOperator`, `Operands`, `SubrIndex`, `CallDepth`,
  `MissingComponent`, `MissingTable`, `GlyphIndex`, `Malformed`); the VM
  maps every one to `invalidfont`.
- **Glyph-space units, per kind.** Type 1: `Glyph.advance` and the
  outline are charstring units exactly as the charstring has them
  (`hsbw`/`sbw`), and the VM uses them unscaled under the font's
  `FontMatrix`. TrueType: the engine reports font units and
  `units_per_em`; the VM multiplies advance and outline by
  `1 / units_per_em`, so for a Type 42 font every `Glyph.dx` the
  backend receives, and every `charpath` point before the font matrix,
  is in units of the em (`a` advancing 1024 in a 2048 em is `dx = 0.5`)
  — what D3 says the identity `FontMatrix` expects. Part 2's PDF
  `Widths` for a Type 42 font are therefore `dx × 1000` (equivalently
  the program's advance × 1000 / units per em); for Type 1 they are the
  charstring advance taken through the font's own `FontMatrix` × 1000,
  which is `dx` itself for the usual `0.001` matrix.
- **Type 1 interpreter.** `type1::{decrypt, encrypt, Decryptor,
  Encryptor, EEXEC_KEY, CHARSTRING_KEY, hex_value, is_hex_section,
  decode_hex, decrypt_section}`. `Type1Program::new(len_iv, subrs,
  charstrings)` decrypts once at construction (`lenIV -1` keeps the
  bytes); `from_decrypted` takes plain bytes; `subrs()`,
  `charstrings()`, `charstring(name)` expose the plain bytes for the
  subsetter, which re-encrypts with `encrypt(CHARSTRING_KEY, bytes,
  4)`; `seac_components(name)` lists the two names a composite glyph
  needs. Operands are taken from the bottom of the stack and the stack
  is cleared, as the format specifies; the stack holds 24 values;
  subroutine calls run through an explicit call stack limited to 30
  levels; a charstring that runs off its end without `endchar` or
  `seac` is `Truncated` (a subroutine may fall off its end). Flex:
  other-subroutine 1 starts collecting, `rmoveto` collects seven points
  while it is on, other-subroutine 0 takes three arguments, emits two
  curves from the last six points, sets the current point from its
  arguments, and leaves them for the two `pop`s; other-subroutine 3
  leaves the subroutine number for `pop`; any other other-subroutine
  hands its arguments to the PostScript stack for `pop`. `seac`
  interprets both components (components may not themselves use
  `seac`) and translates the accent by `(sbx − asb + adx, ady)`, `sbx`
  being the composite's own sidebearing; the advance is the composite's
  `hsbw`. `setcurrentpoint` moves the point without a segment; hints and
  `dotsection` clear their operands; subpaths are closed only where the
  charstring says `closepath`.
- **TrueType parser.** `TrueTypeProgram::parse(bytes)` needs `head`,
  `hhea`, `maxp`, `hmtx`, `loca`, `glyf` and checks `loca` and `hmtx`
  sizes up front (`Truncated`), rejects a collection (`Malformed`), and
  reads `post` (angle, fixed pitch, format 2 names through the 258
  standard Macintosh names in `MAC_GLYPH_NAMES`). `with_names(map)`
  installs the Type 42 `CharStrings`; `gid(name)` consults them first,
  then the `post` names, so a bare TrueType file is addressable by its
  own names. `glyph_by_index`, `outline`, `advance` (last-advance rule),
  `left_sidebearing`, `glyph_data`, `components` (direct components of
  a composite), `table(tag)`, `table_tags`, `bytes`, `bbox`,
  `units_per_em`, `num_glyphs`, `ascender`, `descender`,
  `italic_angle`, `is_fixed_pitch`, `post_name`, and `cmap(platform,
  encoding)` reading formats 4 and 0 (other formats read as absent).
  Composite components nest at most eight deep; a component placed by
  point matching rather than offsets is placed unshifted, a known
  limit. Quadratic contours become cubics exactly (controls at two
  thirds); the start point is the first on-curve point, the last one,
  or the midpoint of two off-curve ends, and a contour closes with a
  line to its start only when the last point is elsewhere.
- **Writer (`truetype::write`).** `Table`, `assemble_parts(tables) ->
  Vec<Vec<u8>>` (directory first, then each table padded to four bytes
  in tag order, checksums set, `head` adjustment computed) and
  `assemble`; `head(&Head)`, `hhea(&Hhea)`, `maxp(&Maxp)` (version 1),
  `hmtx`, `loca_and_glyf(records)` (short format when it fits, records
  padded to four), `simple_glyph(contours)`, `contours_bbox`,
  `post_format2(names, angle, fixed)`, `post_format3`, `cmap(&[(platform,
  encoding, map)])` (format 4, one segment per run of consecutive codes
  through `idRangeOffset`, codes above `0xFFFE` dropped), and `name`.
  Part 2's subsetter reuses these; the pieces `assemble_parts` returns
  are what the `sfnts` strings hold.
- **Builders (`ps_fonts::testing`, D10).** `encode_number`,
  `CharstringBuilder` (one method per operator, `bytes`, `encrypted`),
  `charstring(sbx, wx, &Outline)`, `rectangle`, `hex_lines` (64
  digits per line), `TRAILER`, `eexec_hex`, `eexec_binary` (lead bytes
  chosen so the cipher is not all hexadecimal). `Type1Font::new(name)`
  (a zero-width `.notdef`), `bbox`, `glyph(name, wx, &Outline)`,
  `charstring(name, bytes)`, `subr`, `standard_subrs` (the four flex and
  hint-replacement entries), `encode(code, name)`, `program()`,
  `cleartext()`, `private_text()`, `pfa()`, `pfa_binary()`. The private
  section stores and seals `Private` with `noaccess put` while it is
  still on the dictionary stack, then `dup /CharStrings … dup begin …
  end readonly put end`: the order the operators need, whatever order
  other producers use. `OtherSubrs` are four empty procedures.
  `TrueTypeFont::new(units_per_em)` (an empty `.notdef` of half an em),
  `glyph(name, advance, contours)`, `map(code, gid)`, `family`, `gid`,
  `bbox`, `tables`, `build`, `parts`, `program`, and `type42(font_name,
  encoding)` writing `FontBBox` in units of the em, `CharStrings` as
  name → index, and one `sfnts` string per piece with the trailing zero
  byte. `crates/ps-fonts/tests/truetype_host.rs` parses the host's
  DejaVu Sans when present (names, metrics, a composite glyph, cmaps)
  and prints a skip message otherwise.
- **Snapshots (D2).** `Interp::font_program(dict)` builds
  `ops::embedded::snapshot` on the first `show`, `stringwidth`, or
  `charpath` in the font and caches the `Rc<Program>` by `FID`; nothing
  invalidates it. The dictionary is read without access checks
  (`Private` is `noaccess`, charstrings are `noaccess` strings by
  design). Type 1: `Private/lenIV` (default 4), `Private/Subrs`
  (optional), `CharStrings` (required; a non-string value is
  `invalidfont`). Type 42: `sfnts` strings joined, a string of odd
  length ending in a zero byte losing that byte (tables and glyph
  records are even-sized, so only the conventional padding byte makes
  a string odd), `CharStrings` name → integer (other values skipped);
  a program the parser rejects is `invalidfont`. A Type 1 or Type 42
  dictionary without `CharStrings` or `sfnts` is `invalidfont` at that
  first need, which keeps the `type1-without-program` corpus file true.
- **Show and charpath (D4).** `FontKind::Embedded { program, scale }`
  joins `Resident` and `Type3` (the enum is `Clone`, no longer `Copy`).
  Per glyph: the encoding's name, else `.notdef`, else a zero-advance
  blank; an interpretation error is `invalidfont` from the operator.
  `charpath` is `show::begin_charpath`: the frame's `outline` flag
  appends each glyph's outline through `backend.moveto/lineto/curveto/
  closepath` with points `FontMatrix.apply(p × scale) + position`,
  `position` being the run's origin (read once) plus the run's
  displacement through the matrix; the backend applies the CTM. A
  `makefont` translation therefore offsets the glyphs but not the
  advance, which is a vector. The run ends with `moveto` to the end
  position, so the current point advances as `show` would and the path
  carries a trailing single-point subpath — which is what `pathforall`
  shows after `charpath` in the reference too; part 2's fill of a
  charpath must tolerate it. The boolean is accepted and ignored; a
  `PaintType 2` font outlines the same way (tested). Resident and Type
  3 fonts are `invalidfont` from `charpath` with the operands left in
  place.
- **Boundary (D5).** `FontSource::Embedded { family, kind, program,
  font_matrix, font_name }`: `font_name` (the dictionary's `FontName`
  as bytes) is an addition to D5, for the dump line and the PDF
  `BaseFont`; `FontSource` lost `Copy` and compares programs by
  `Rc::ptr_eq`. `ps_graphics::Graphics::define_font` stores the
  description; `font_resource` answers `invalidfont` for an embedded
  source, so under the real backend `show` in an embedded font raises
  `invalidfont` until 4.1 replaces the stub — measuring and `charpath`
  work, and the dump is untouched.
- **Spec readings.** The Type 42 "Outline conversion" scenario's
  bounding box: after cubic conversion the control box of the path is
  not the quadratic contour's control box (the cubic controls sit at two
  thirds of the way to the quadratic control), so the corpus file of
  that scenario, which waits for a `pathbbox` over a real path, should
  compare against the on-curve extremes or a box the file computes,
  not the quadratic control box; the mock-backend test checks the
  on-curve x extremes and containment in y.
- **For part 2.** `FontSource::Embedded` carries everything the IR
  resource needs (`program.glyph_count()`, `font_name`, `kind`); the
  Type 1 subsetter reads `Type1Program::{charstrings, subrs, len_iv,
  seac_components}` and re-encrypts; the TrueType subsetter reads
  `TrueTypeProgram::{glyph_data, components, advance, left_sidebearing,
  table, bbox, post_name, cmap}` and writes through `truetype::write`;
  `Glyph.dx` is charstring units for Type 1 and units of the em for
  Type 42 (see the units bullet for `Widths`); the corpus generator in
  `ps-fonts/tests/corpus_fonts.rs` is where the painting and IR
  scenarios' files are added, with the same fonts.

### Part 2

Covers the IR resource and its dump line (4.1), the Type 1 writer and
the TrueType subsetter (5.1, 5.2), embedding in the PDF (5.3, 5.4), and
the page-producing corpus files with their goldens (6.x).

- **The IR resource (D5).** `FontSpec::Embedded { family, kind,
  font_name, font_matrix, program: ProgramRef, encoding }`.
  `font_matrix` is an addition to the shape 4.1 listed: the PDF text
  matrix and the widths need the matrix glyph space is measured in, and
  the resource is the only thing the PDF writer sees. `ProgramRef` wraps
  the `Rc<Program>` and compares by pointer, which makes `FontSpec`
  comparable: since the VM builds one snapshot per `FID`, two instances
  of one family share a snapshot, and `Resources::intern_font` interns
  the resource by snapshot, encoding, name, and matrix without a
  separate family list. `FontSpec::width(code)` answers from the program
  in the space the font matrix maps (the advance × `1 / units_per_em`
  for TrueType), falling back to `.notdef` exactly as the VM's show does,
  so the content writer's positioning arithmetic sees the same numbers
  the run was recorded with; `program_glyph(code)` exposes the glyph
  itself. Dump line: `font <n> embedded <type1|truetype> <FontName>
  glyphs=<count> enc=[<code> /<name> …]` over every code that names a
  glyph — an embedded font has no built-in encoding to print
  differences from, and the full list is what the PDF `Differences`
  and the regenerated program's `Encoding` are built from. Program bytes
  never appear. Every pre-existing golden is byte-identical.
- **What the snapshot carries for the writer (D6, resolved).** The
  design had the Private entries "printed back through the VM's
  syntactic printer"; `ps-fonts` cannot call the VM, so the printing
  happens at snapshot time: `ops::embedded::snapshot` attaches a
  `ps_fonts::type1::Type1Dict { font_bbox, paint_type, font_info,
  private }` to the `Type1Program`, the last two as `(key, source text)`
  pairs written by `ops::output::source` (the `==` form with operators
  as bare names). Entries whose value cannot be printed as source —
  `executeonly` procedures such as `RD`/`ND`/`NP`, dictionaries, files —
  are left out, as are `Subrs` and `lenIV`; the writer then drops
  `UniqueID` and the reading-procedure names too and supplies its own.
  The descriptor's numbers (`ItalicAngle`, `isFixedPitch`, `StdVW`,
  `CapHeight`) are read back from that text by `Type1Dict::
  font_info_number`/`font_info_bool`/`private_number` (the first number
  of the printed value, so `StdVW [80]` gives 80). This is the only
  change to `ps-vm` in part 2 and it is additive.
- **Type 1 writer (D6) as built.** `type1::write::write(program,
  &Header { font_name, font_matrix, encoding }, &glyphs) -> Written {
  bytes, length1, length2, length3 }` and `subset_names(program, used)`
  (the names the program has among `used`, `.notdef`, and every `seac`
  component). Cleartext: `%!FontType1-1.0: <name>`, `11 dict begin`,
  `FontInfo` when the snapshot has entries, `FontName` (the tagged
  name), `PaintType`, `FontType 1`, `FontMatrix` (the resource's, six
  numbers), `FontBBox` as a procedure, `Encoding` as `256 array` filled
  with `.notdef` and `dup <code> /<name> put` lines — `StandardEncoding`
  when the resource's encoding is exactly that — then `currentdict end`
  and `currentfile eexec`; `length1` counts through that line's
  newline. The encrypted section is binary: `dup /Private <n> dict dup
  begin`, `RD`/`ND`/`NP` and `lenIV 4`, the printed entries as `/<key>
  <value> def`, `Subrs` re-encrypted with four lead bytes (all of them;
  pruning stays the open question), `noaccess put`, `CharStrings` of
  the kept glyphs re-encrypted likewise in name order, `end readonly
  put end`, `dup /FontName get exch definefont pop`, `mark currentfile
  closefile`. A program without `.notdef` gets a blank one. The four
  plain lead bytes are chosen so the cipher's first four bytes are not
  all hexadecimal digits and its first byte is not whitespace (a lenient
  reader skips whitespace after `eexec`); `testing::eexec_binary` now
  delegates to that rule and the committed corpus bytes were unchanged
  by it. The trailer is a newline, the 512 zeros, and `cleartomark`;
  `length3` counts it. `Written.bytes` is `length1 + length2 +
  length3` long.
- **TrueType subsetter (D7) as built.** `truetype::write::subset(program,
  &used_gids, &codes) -> Result<Vec<u8>, FontError>`: glyph 0, the used
  glyphs, and the components of every kept composite (transitively),
  renumbered densely in old-index order; composite records have their
  component indices rewritten; `hmtx` gets one full metric per glyph,
  `hhea`, `maxp`, and `head` are the original tables with the metric
  count, glyph count, and `loca` format patched (the checksum
  adjustment is recomputed by the assembler); `post` is format 3 with
  the original angle and pitch; the `cmap` has one `(3,0)` format 4
  subtable mapping each code to its new index both as the bare code and
  as `0xF000 + code`, a code that resolves to glyph 0 left out; `cvt `,
  `fpgm`, and `prep` are copied when present. No `OS/2` or `name` table
  is written. Verified by re-parsing, including a composite.
- **Embedding (D8, D9) as built.** `remelt::embedded::EmbeddedTable`,
  held by `FontTable`: `write_fonts` routes every `FontSpec::Embedded`
  to it; the table keys fonts by the whole resource (snapshot pointer
  and encoding included), allocates the font object at first use, and
  collects the codes each page shows in that font — in the page's own
  text operations and inside its Type 3 glyph procedures. `PdfSink::
  finish` writes the fonts before the page tree, so their objects are
  the last in the file whatever their ids. One font object per snapshot
  *and* encoding: a re-encoded copy of an embedded font gets its own
  dictionary, descriptor, and program stream, because the TrueType cmap
  maps codes and two encodings could disagree on one; sharing the
  Type 1 program across encodings is possible and not done. The font
  dictionary: `Type1` or `TrueType`, `BaseFont` tagged, `FirstChar`/
  `LastChar`/`Widths` over the used codes (zero for an unused code in
  between; a width is the advance through the font matrix's `a` × 1000,
  rounded to a thousandth — for the usual `0.001` matrix the charstring
  advance, for Type 42 `dx × 1000`), `Encoding` `Differences` for Type 1
  over the used codes whose name is not the standard encoding's (both
  the standard-encoding base of a non-symbolic font and the built-in
  base of a symbolic one — which the regenerated program's own
  `Encoding` makes the same array — leave those as the only entries
  needed; none means no `Encoding`), no `Encoding` for TrueType,
  `FontDescriptor`, `ToUnicode` over the used codes with a name. The
  content writer's text matrix for an embedded font is the run's matrix
  taken back through the font matrix, computed in double precision so
  a `0.001` matrix at size 10 prints `10`, and the pen advance is
  `wx × a` as for the other kinds. Descriptor: `Flags` symbolic (4)
  unless a Type 1 font's encoding is the standard one (32); TrueType
  always symbolic; fixed pitch from `FontInfo/isFixedPitch` or `post`;
  italic when the angle is not zero. `FontBBox` is the dictionary's box
  (Type 1) or `head`'s (TrueType) through the font matrix × 1000;
  `Ascent`/`Descent` are its top and bottom for both kinds (D9 as
  written; `hhea`'s values are not used); `ItalicAngle` from `FontInfo`
  or `post`; `CapHeight` when `FontInfo` has it; `StemV` from `StdVW`,
  else 80 for both kinds (D9 named the default for TrueType only; a
  Type 1 font without `StdVW` gets the same). `FontFile` carries
  `Length1/2/3`; `FontFile2` carries `Length1`; both streams follow
  `Options::compress`. Should the TrueType subset ever fail, the whole
  program is embedded instead (it cannot fail for a program whose used
  glyphs the VM already interpreted). ToUnicode: the glyph list on the
  encoding's names; for TrueType a name the list lacks is looked up
  through the program's `(3,1)` cmap read backwards from the glyph the
  name selects. `Report` is unchanged.
- **Subset tag (5.4).** Six upper-case letters from a 64-bit FNV-1a hash
  over the font name and the sorted kept glyph set (each item followed
  by a zero byte; for TrueType the kept glyph indices as big-endian
  bytes), taken as successive base-26 digits. The tag prefixes
  `BaseFont`, the descriptor's `FontName`, and the regenerated program's
  `FontName`. The same font with the same glyph set gets the same tag in
  every run; snapshot identity leaves no trace in the output.
- **Corpus.** Eight new files under `corpus/unit/fonts/` with `.ir` and
  `.pdf` goldens: `charpath-fill`, `type1-seac-glyphshow`,
  `type42-charpath-bbox`, `embedded-font-dump` (also the text spec's
  "showing an embedded font"), `type1-subset-round-trip`,
  `type1-embedded-two-pages`, `truetype-subset-cmap` (the Type 42
  wrapper encoded at 65 and 66), and `truetype-embedded`; the fonts are
  `ps_fonts::testing::corpus_type1()` and `corpus_truetype()`, shared
  with the tests of `ps-graphics` and `remelt`, and the seven part-1
  files were regenerated unchanged. The Type 42 bounding-box file uses
  `[20 0 0 20 0 -5] makefont`: `pathbbox` counts the run's start point
  and the trailing point `charpath` leaves at the advance, so the x
  extent is the run's and only the y extent can show the cubic control
  box; the translation puts both points inside it, and the `.ir` golden
  pins the outline. The fill of a charpath keeps the trailing
  single-point subpath (and the program's own `moveto`), as part 1 said
  it would. The round-trip scenario is a `remelt` test that extracts
  the `FontFile` from the distilled document, runs it through the
  interpreter, and compares glyph count, membership, advance, and the
  filled outline with the original program's. With `pdffonts`, every
  embedded golden lists its font as `emb yes sub yes uni yes`, `pdftotext`
  extracts the shown text (`a`, `e`, `ao`, `é`), `pdfinfo` as
  `EFTERSCRIPT_PDF_CHECK` accepts all 106 corpus files, and `pdftoppm`
  renders the glyphs, so the regenerated Type 1 and subset TrueType
  programs are accepted by an independent rasteriser.
- **Known limits.** A `glyphshow` of a name outside the encoding is
  recorded under code 0 (text-core's limit) and so embeds the code-0
  glyph; a Type 1 program shared between two encodings is embedded
  twice; `Subrs` are not pruned.

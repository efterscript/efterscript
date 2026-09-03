# Design: CFF fonts

See proposal.md for motivation and the spec deltas for the contract.
This document fixes the CFF engine's shape, how CFF enters the VM, and
how subsets are written.

## Context

- `ps_fonts::Program` is an enum over Type 1 and TrueType with
  `glyph(name)`, a per-program cache, and `ProgramKind` that the VM's
  snapshot and the PDF writer dispatch on. The Type 1 charstring
  interpreter has a trace mode and the writer renumbers subroutines.
- The VM snapshots a font's program from its dictionary by `FontType`
  (1: `CharStrings`/`Private`; 42: `sfnts`/`CharStrings`) on first glyph
  use, cached by `FID`. Resource categories are an enum of `Font` and
  `Encoding` with per-category dictionaries.
- PDF embeds CFF as `FontFile3` (subtype `Type1C` for name-keyed,
  `CIDFontType0C` for CID-keyed) per ISO 32000-1 §9.9; the writer's
  descriptor path takes a stream name.

## Goals / Non-Goals

**Goals:** a complete CFF reader (name- and CID-keyed) and a name-keyed
writer; FontType 2 fonts through the FontSet path working end to end.

**Non-Goals:** CID-keyed fonts visible to PostScript; CFF2; converting
other formats to CFF; hinting semantics.

## Decisions

**D1. `cff` module with one reader for both keyings.** `CffProgram`
parses eagerly into: strings, global subroutines, charset (glyph index
→ SID or CID), encoding (code → glyph index, name-keyed only), top
dictionary values, and either one private dictionary with its local
subroutines or, CID-keyed, a vector of them plus the select table.
`glyph_by_index`, `glyph(name)` (name-keyed, through the charset's
SIDs), and `glyph_by_cid` (CID-keyed, through the charset's CIDs);
advances from the charstring width or the private dictionary's default
width. `Program::Cff(CffProgram)` and `ProgramKind::Cff`. The Type 2
interpreter is its own module: operand stack of 48, the width rule on
the first stack-clearing operator, `hstemhm`/`vstemhm`/`hintmask`/
`cntrmask` counting stems to skip mask bytes, `callsubr`/`callgsubr`
with bias 107/1131/32768 by count, the four flex forms as two curves,
`endchar` with four arguments composing base and accent through the
standard encoding, and the arithmetic escapes accepted (`abs`, `add`,
`sub`, `div`, `neg`, `random` as 0, `mul`, `sqrt`, `drop`, `exch`,
`index`, `roll`, `dup`, `put`/`get` on a 32-entry transient array,
`and`/`or`/`not`/`eq`/`ifelse`) since fonts in the wild use them.
*Alternative:* two readers — the CID-keyed layout differs only in where
the private data lives, and one reader with an `enum Privates` keeps
the interpreter identical.

**D2. Loading through `FontSetInit`.** Resource categories gain
`ProcSet` (a table of built-in dictionaries; `FontSetInit` holds one
operator, `StartData`) and `FontSet` (defined resources only).
`StartData` pops a count and a name, reads that many bytes from the
innermost file frame's file (the run file or an `eexec`-free file; a
short read is `invalidfont`), parses the CFF, and for each name-keyed
font in it builds a read-only FontType 2 dictionary: `FontName`,
`FontMatrix` (the top dictionary's, default `[0.001 0 0 0.001 0 0]`),
`FontBBox`, `PaintType 0`, `Encoding` (the CFF encoding rendered to
names, or `StandardEncoding`), `CharStrings` mapping every charset name
to its glyph index (as Type 42 does), and `FID` through `definefont`.
The parsed program is cached on the interpreter under the `FID` before
`definefont` returns, so the snapshot path finds it without re-parsing;
a FontType 2 dictionary that reaches a glyph without a cached program
(one a job built by hand) is `invalidfont`, like a Type 1 dictionary
without charstrings. The `FontSet` resource is an array of the font
names defined. A CID-keyed font in the data is parsed and cached but
not defined as a font (the composite change defines it as a CIDFont).
*Alternative:* store the CFF bytes in the dictionary as a string — the
program is immutable data the job never needs to see, and Type 42's
`sfnts` precedent exists only because the reference specifies it.

**D3. Snapshot and show.** `snapshot` handles `FontType 2` by taking
the cached program; `FontKind::Embedded` needs no change: advances are
in charstring units under the font's own matrix, so `scale` is 1 (a
non-standard CFF `FontMatrix` is already in the dictionary's
`FontMatrix`). `charpath` follows.

**D4. Writer and subset.** `cff::write` emits a name-keyed CFF: header,
name index, top dictionary (charset and charstrings offsets, private
size and offset, `FontBBox`, `FontMatrix` when non-default, `Notice`
and `FullName` copied when present), string index for the names used,
global subroutines kept and renumbered, charset format 0 listing the
kept glyphs in order, no encoding, charstrings for kept glyphs with
`callsubr`/`callgsubr` operands re-encoded against the new biases, a
private dictionary carrying the numeric hinting entries and default
and nominal widths verbatim with its pruned local subroutines. Pruning
traces both subroutine kinds through the Type 2 interpreter (`hintmask`
inside a subroutine keeps working because the trace runs the real
interpreter). Offsets are computed in two passes with fixed-size
operand encodings for the offset fields. The round-trip test parses
the written program and compares every kept glyph's outline and
advance. *Alternative:* embed the original CFF whole — correct and
simple, but CJK-sized CFFs are megabytes and the composite change
would need the writer anyway.

**D5. PDF.** `ProgramKind::Cff` routes to a `Type1` dictionary with
`FontFile3 /Subtype /Type1C`; widths via the existing width path (the
font matrix is honoured); the descriptor's stem width from the private
dictionary's `StdVW`, italic angle and fixed pitch from the top
dictionary; flags as for Type 1.

**D6. Test assets.** `testing::CffFont` builds name-keyed and CID-keyed
programs from named outlines with a Type 2 encoder (including a
`hintmask` and an `hflex`); the corpus generator writes FontSet corpus
files with binary data after `StartData` — the corpus harness reads
files as bytes and parse-survival tokenises them, so the binary section
must not break either: the generator emits the CFF, and parse-survival
learns that a `StartData` count skips that many bytes (a small, honest
change recorded in the notes). `cargo xtask fetch-fonts --test-assets`
extracts one TeX Gyre OpenType file into `target/test-fonts/`; a test
parses its CFF table and interprets every glyph when the file is
present.

## Risks / Trade-offs

- [Type 2 width parsing is easy to get subtly wrong (odd argument
  counts per operator)] → unit tests per operator with and without a
  width, and the TeX Gyre OpenType check exercising a real font's every
  glyph.
- [Binary bytes in corpus files] → the difftest and parse-survival
  changes above; the files are generated, never hand-edited.
- [Renumbering with bias] → the same round-trip guard as Type 1, plus
  a test where a subroutine count crosses a bias boundary after
  pruning.

## Open Questions

- Whether `StartData` should also accept the hexadecimal form some
  tools emit. Not seen in the corpus; can be added without spec change.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

### Part 1

Covers the engine (1.x) and loading (2.x): the CFF reader, the Type 2
interpreter, `Program::Cff`, the test builder and the OpenType check,
the `ProcSet` and `FontSet` categories with `StartData`, and the corpus
files. The writer, `FontFile3`, and the PDF goldens of text scenarios
are part 2.

- **Reader API (D1) as built.** `ps_fonts::cff::{CffProgram,
  parse_fonts, Dict, DictOp, esc, op, PrivateDict, Privates,
  CffEncoding, Ros, Reached, STANDARD_STRINGS, ISO_ADOBE_COUNT,
  DEFAULT_FONT_MATRIX}`. `parse_fonts(bytes)` yields every font of the
  name index (the string and global-subroutine indexes are shared), and
  `CffProgram::parse` the first. Eagerly parsed: `Dict` (operators in
  order with `f64` operands, `get`/`number`), the charset as glyph index
  → string id or CID (`charset`, `charset_names`, `glyph_name`), the
  encoding (`CffEncoding::Standard | Custom`, `encoding()` resolving
  the standard one through the glyph names, `has_standard_encoding`),
  `Privates::Single | Cid { dicts, select }` (`private`,
  `private_for(gid)`, `fd_index`), `PrivateDict { dict, subrs,
  default_width_x, nominal_width_x }` with every entry but `Subrs`
  kept as numbers (`number(op)` for `StdVW`), and the lookups `gid`,
  `gid_of_cid`, `glyph_by_index` (cached by index), `glyph(name)`,
  `glyph_by_cid`, `glyph_names` (sorted), `glyph_count`, `charstring`,
  `global_subrs`, `font_matrix` (`has_font_matrix` tells the default
  apart), `font_bbox`, `italic_angle`, `is_fixed_pitch`, `paint_type`,
  `top_string(op)` (`Notice`, `FullName`, …), `ros`, `cid_count`,
  `reached_subrs(gid) -> Reached { local, global }`, and
  `components(gid)`. `Program::Cff` routes `glyph`, `has_glyph`,
  `glyph_count` (the glyph count, names or not), `glyph_names`, and
  `units_per_em` (`None`: charstring units under the font matrix, so
  `FontKind::Embedded { scale }` is 1, as for Type 1 — D3 holds).
  Predefined charsets: ISOAdobe is the identity on string ids up to the
  glyph count; the two expert charsets and the expert encoding raise
  the new `FontError::Unsupported`, recorded rather than guessed. A
  charstring type other than 2 is `Unsupported` too. Encoding format 0
  and 1 with the high-bit supplements (code → string id, resolved
  through the charset); select formats 0 and 3; a CID-keyed program
  without a select table puts every glyph in dictionary 0. Structural
  faults are `Truncated(<structure>)` or `Malformed(<what>)`; the VM
  maps every `FontError` to `invalidfont`.
- **Width rule (D1) as built.** The interpreter's `take_width` runs at
  the first of `hstem`/`vstem`/`hstemhm`/`vstemhm`/`hintmask`/
  `cntrmask`/`rmoveto`/`hmoveto`/`vmoveto`/`endchar`, with the operator's
  own parity rule deciding whether a leading width is present: an odd
  argument count for the four stem operators and for a mask (whose
  arguments are an implicit `vstem`), more than two for `rmoveto`, more
  than one for `hmoveto`/`vmoveto`, exactly one or five for `endchar`.
  Present, the width is `nominalWidthX + first argument`; absent,
  `defaultWidthX`; later operators never take one (the test `second`
  pins an odd-count `rmoveto` after a stem). Mask bytes are
  `(stems + 7) / 8` with stems counted from every stem operator and from
  the arguments a mask consumes. Bias: 107 below 1240 entries, 1131
  below 33900, else 32768 (`charstring::bias`); depth over ten
  subroutine levels is `CallDepth`; a stack past 48 is `Malformed`.
  Subpaths close implicitly at every `rmoveto`/`hmoveto`/`vmoveto` and
  at `endchar`, which is what the outline model's `Close` expects; a
  subroutine may fall off its end (a return), the glyph's own charstring
  may not. The accent form of `endchar` places the accent's origin at
  `(adx, ady)` — no sidebearing term, unlike `seac` — and the advance is
  the composite's own. `random` pushes 0. `charstring::{tokens, encode,
  encode_number, encode_fixed, Token}` are the token-level API the
  renumbering in part 2 needs; masks are read as `Token::Mask` by the
  running stem count. `encode_number` holds the sixteen-bit range only
  (an assertion, since the encoding has no wider integer).
- **Builder and assets (D6) as built.** `testing::{Type2Builder,
  charstring_type2, CffFont, CffFd, CidLayout, corpus_cff,
  encode_type2_number}`. `CffFont::new(name)` (name-keyed, a `.notdef`
  of the default width), `cid_keyed(name, registry, ordering,
  supplement)`, `widths(default, nominal)`, `std_vw`, `notice`,
  `font_matrix`, `bbox`, `glyph(name, wx, &Outline)`, `charstring`,
  `subr`, `gsubr`, `encode(code, name)` (a custom encoding written as
  format 0 with one supplement per code, so glyphs need not sit in code
  order), `fd(CffFd)`, `cid_glyph(cid, fd, code)`, `build()`,
  `program()`, and `font_set(set_name)` — the FontSet file: `/FontSetInit
  /ProcSet findresource begin`, `/<set> <count> StartData`, one newline,
  the binary program, then `\nend\n`. Layout: header, name index, top
  dictionary index, string index, global subroutines, charset (format
  0), encoding, select table (format 3), charstrings, then either the
  private dictionary with its subroutines or the font dictionary array
  followed by each dictionary's private data; every offset is written
  in the five-byte form (`cff::write::DictWriter::fixed`) so sizes are
  known before offsets are. `cff::write::{index, offset_size, dict_int,
  dict_int5, dict_real, dict_number, dict_op, DictWriter,
  charset_format0, encoding_supplements, fd_select_format3, header}` is
  what part 2's subset writer assembles from. `corpus_cff()`: `SynCFF`,
  nominal width 500, `a` the 500-unit square of advance 600 (width
  delta 100), `b` (advance 400, through local subroutine 0 and global
  0), `c` (global 0 only), `f` (two stems, a `hintmask`, an `hflex`
  whose control box is 0..600 by −50..200, advance 600), local
  subroutine 1 reached by nothing, `StdVW 80`, the standard encoding.
  `cargo xtask fetch-fonts --test-assets` downloads (or reuses) the
  TeX Gyre release and copies `tex-gyre/opentype/texgyrepagella-
  regular.otf` to `target/test-fonts/`; `tests/cff_opentype.rs` finds
  the `CFF ` table through the new `truetype::table_directory` (the
  table-directory reader split out of `TrueTypeProgram::parse`),
  interprets every glyph, and compares every advance with the committed
  `qplr.metrics` table of the resident Palatino face, which was derived
  from the same family's Type 1 program: over a thousand glyphs agree
  to the unit, so the width rule and the hint handling hold on a
  production program. Absent, the test prints a skip line.
- **Loading (D2) as built.** `ops::fontset::OPS` declares `StartData`
  with the new `Visibility::ProcSet`, which `populate` enters in no
  dictionary; `fontset::init_dict` builds the read-only global
  `FontSetInit` dictionary at construction and `Interp::font_set_init`
  holds it. `StartData` (`[Name, Int]`) pops nothing until it succeeds:
  it reads exactly `count` bytes with `Memory::file_read` from
  `Interp::current_file()` — the innermost file frame's object, so the
  run file, a file the job opened and is executing, and an `eexec` layer
  all serve (a short read is `invalidfont`, a negative count
  `rangecheck`); the scanner has consumed exactly the one whitespace
  byte after the token, so the data starts at the cursor. With a
  streaming source the run buffer holds only what has arrived, the same
  limit the Type 1 `RD` procedures have. For each name-keyed font: a
  dictionary with `FontType 2`, `FontName`, `FontMatrix` (the top
  dictionary's, default `[0.001 0 0 0.001 0 0]`, integral entries as
  integers), `FontBBox`, `PaintType`, `Encoding` (the shared
  `StandardEncoding` object when the CFF uses the standard encoding,
  else a read-only 256-name array with `.notdef` at unassigned codes),
  `CharStrings` (read-only, every charset name → glyph index), and a
  fresh `FID`; the program is cached under that `FID`
  (`Interp::cache_font_program`) before `font::define` registers the
  dictionary, so `font_program` never snapshots it. The `FontSet`
  resource is a read-only array of the names, entered in the category
  dictionary of the current allocation mode. CID-keyed fonts are held
  in `Interp::cid_programs` by the CFF's own name, reachable through
  `Interp::cid_program(name) -> Option<Rc<Program>>`; they are not in
  any font directory and not in the FontSet array. `definefont`
  accepts `FontType 2` with the structural checks of 1 and 42; a
  hand-built FontType 2 dictionary defines, and its first glyph need
  is `invalidfont` from the show operator (the snapshot has nothing to
  build from). `show::font_kind` treats 2 like 1 and 42, so the show
  family, `stringwidth`, and `charpath` need no other change; the
  backend sees `FontSource::Embedded { kind: ProgramKind::Cff }`, and
  the dump prints `embedded cff`. `remelt` gained a placeholder arm for
  a CFF program: a `Type1` font dictionary with a descriptor and no
  `FontFile` stream, which part 2 replaces with the `Type1C` subset;
  no corpus file reaches it (the page-producing scenario is a
  `charpath` fill).
- **Resource categories (D2).** `ops::resource::Kind` gained `ProcSet`
  and `FontSet`, each with local and global instance dictionaries on
  the interpreter (`procset_category`, `fontset_category`).
  `resourcestatus` answers status 2 for `FontSetInit` (held outside VM,
  like the resident fonts) and 0 for defined instances; `findresource`
  returns the procedure-set dictionary or the FontSet array;
  `defineresource` into `ProcSet` is allowed for a dictionary instance
  (a program may define its own procedure sets; `typecheck` otherwise)
  and into `FontSet` for an array; `undefineresource` and
  `resourceforall` follow, the built-in list being `FontSetInit` for
  `ProcSet` and empty for `FontSet`.
- **Corpus and tooling (D6).** `corpus/unit/fonts/`: `cff-width`
  (`6.0 0.0`), `cff-charpath-bbox` (`0.0 -0.5 6.0 2.0`, then `fill
  showpage`, with `.ir` and `.pdf` goldens), `fontset-defines-fonts`
  (`true`, `2`), `fontset-short-data` (`invalidfont`), all headed
  `%!PS-Adobe-3.0 Resource-FontSet` and generated by
  `tests/corpus_fonts.rs`; `corpus/unit/text/procset-status.ps` holds
  the ProcSet scenario. The difftest harness needed no change: it reads
  files as bytes, and its header parser stops at the first line that is
  not a comment, before the binary section. `cargo xtask parse-survival`
  now scans token by token through its own cursor source and, on the
  executable name `StartData` preceded by an integer, moves the cursor
  that many bytes forward (clamped to the end), exactly what the
  interpreter's operator consumes; a bare `StartData` without a count is
  scanned as a name. Every pre-existing golden is byte-identical.
- **Deviations and readings.** Expert charsets and the expert encoding
  are unsupported (the corpus has none; a program using them raises
  `invalidfont`). The design's "encoding (code → glyph index,
  name-keyed only)" holds; the standard encoding is not stored as a
  table but resolved through the glyph names, which is also how the VM
  decides to share the `StandardEncoding` object. `Program::glyph_count`
  for a CID-keyed program counts glyphs, not names. D2 said the
  category holds "defined resources only" for `FontSet`, and
  `defineresource` is allowed for both new categories, a small addition.
- **Verification.** `cargo test --workspace`: 588 passed, 2 ignored
  (554 before; thirty-four new tests); `cargo test -p ps-fonts --no-default-features` passes;
  clippy clean on all targets with and without the feature; `cargo fmt
  --check` clean; `difftest run` 120 of 120; `parse-survival` 120 files,
  no failures; `openspec validate cff-fonts` valid.
- **For part 2.** The subset writer reads `CffProgram::{charset,
  charset_names, charstring, global_subrs, private (dict, subrs,
  default_width_x, nominal_width_x), font_bbox, has_font_matrix,
  font_matrix, top_string(op::NOTICE / op::FULL_NAME), reached_subrs,
  components, gid}` and assembles with `cff::write`; renumbering
  rewrites `Token::Num` before `Op(10)`/`Op(29)` against the new
  biases (`charstring::bias`) and re-encodes with `charstring::encode`;
  the descriptor takes `italic_angle`, `is_fixed_pitch`, and
  `private().number(op::STD_VW)` (the placeholder arm in
  `remelt::embedded` already does). The composite change finds a
  CID-keyed program through `Interp::cid_program(name)` and its glyphs
  through `glyph_by_cid`, `private_for`, and `fd_index`.

### Part 2

Covers the writer and subset (3.1), the `FontFile3` embedding (3.2), and
the verification (4.1).

- **Writer API (D4) as built.** `cff::write::{subset_names, subset}`.
  `subset_names(program, used)` is the names the program has among
  `used`, `.notdef`, and the components of every accented glyph among
  them (through `components`). `subset(program, font_name, &names) ->
  Result<Vec<u8>, FontError>` writes, in order: the header (its offset
  size from the total length), the name index with the caller's tagged
  name, the top dictionary, the string index, the global subroutine
  index, a format 0 charset, the charstrings index, the private
  dictionary, and its local subroutine index directly after it. Kept
  glyphs are ordered by their original index with `.notdef` first (it is
  always glyph 0 of a name-keyed program). The top dictionary carries the
  string-valued entries `version`, `Notice`, `Copyright`, `FullName`,
  `FamilyName`, and `Weight` re-interned when present (D4 named `Notice`
  and `FullName`; the others cost nothing and keep the program's
  identification and copyright intact), then `FontBBox`, `isFixedPitch`,
  `ItalicAngle`, `UnderlinePosition`, `UnderlineThickness`, `PaintType`,
  `StrokeWidth`, and `FontMatrix` copied as the parsed operands when
  present (`PaintType` and `StrokeWidth` so a stroked font stays one; the
  default matrix is never written), then the three offset entries in
  the five-byte form. No `Encoding` entry: the subset carries the
  standard encoding, and the PDF `Differences` are computed against the
  standard encoding, so the two agree. The private dictionary is every
  entry the reader kept (`Subrs` excluded), operands verbatim — the
  hinting values, `defaultWidthX`, `nominalWidthX` — with a five-byte
  `Subrs` offset when local subroutines survive. Strings are interned in
  order of first use, standard strings by position. A CID-keyed program
  is `Unsupported("CID-keyed subset")`.
- **Pruning and renumbering as built.** The trace is the union of
  `reached_subrs` over the kept glyphs (an empty charstring reaches
  nothing and is not interpreted); the reached local and global indices
  are renumbered densely in index order, the biases before and after
  come from `charstring::bias` over the old and new counts, and every
  kept charstring and subroutine is rewritten token by token: the
  literal before `callsubr`/`callgsubr` becomes `new − new bias`, other
  numbers are re-encoded in canonical form, masks and operators pass
  through. Three tiers, as the Type 1 writer has them: renumbered when
  every call's operand is a literal in the kept set; otherwise the
  original numbering with every unreached subroutine written as the
  one-byte `return` stub (the count, and so the bias, unchanged); and
  when a kept glyph cannot be interpreted every subroutine and
  charstring is copied as it is. The design's "hintmask inside a
  subroutine keeps working because the trace runs the real interpreter"
  was true of the trace but not of the rewrite: a tokeniser counts the
  stems declared in the charstring it reads, and a mask in a subroutine
  is sized by the stems its caller declared. So `Reached` gained
  `masks`, the set of `(Site, offset, byte count)` for every mask the
  interpreter executed, `Site` being `Glyph(gid) | Local(k) |
  Global(k)`, and `charstring::tokens_with_masks(code, mask_len)` takes
  the count from the trace by offset, falling back to the static count
  only for a mask the run never reached (dead code, whose bytes cannot
  matter). Two runs disagreeing on one mask's length is treated as an
  operand that cannot be rewritten. The unit test pins a subroutine
  whose mask byte is the `callsubr` operator's value.
- **Embedding (D5) as built.** The placeholder arm in `remelt::embedded`
  now subsets through `subset_names` and `subset`, writes the bytes as a
  stream with `/Subtype /Type1C` (no lengths), and names it `FontFile3`
  in the descriptor. The subset tag, the `Type1` dictionary, widths
  through the font matrix, `Differences`, ToUnicode, and the descriptor
  (`Flags` non-symbolic only for the standard encoding, fixed pitch and
  `ItalicAngle` from the top dictionary, `StemV` from `StdVW` else 80,
  `FontBBox` through the matrix) are the part 1 placeholder's, which
  already followed D5. Should `subset` fail — only a CID-keyed program
  does, and the VM defines no font over one — the descriptor is written
  without a program stream, as the placeholder did.
- **Sizes.** The synthesised `SynCFF` program is 179 bytes; the subset
  of `a` alone is 95 (no subroutine survives: `a` reaches none, and
  local 1 was unreached by design). TeX Gyre Pagella's `CFF ` table is
  149,993 bytes; the subset of `P` and `a` is 740 bytes, keeping 7 of
  423 local subroutines (the face has no global ones), every kept glyph
  measuring and outlining as the original; the whole face rewritten
  through `subset` round-trips every one of its glyphs.
- **Corpus and goldens.** `corpus/unit/fonts/cff-embedded-type1c.ps`
  (`(a) show` on a page, generated by `tests/corpus_fonts.rs`) with
  `.ir` and `.pdf` goldens. The `.ir` line is `font 0 embedded cff
  SynCFF glyphs=5 enc=[…]` over the standard encoding; the font object
  reads `/Type /Font /Subtype /Type1 /BaseFont /ZUDKKT+SynCFF /FirstChar
  97 /LastChar 97 /Widths [ 600 ]` with `/FontDescriptor … /Flags 32
  /FontBBox [ 0 -50 600 500 ] /ItalicAngle 0 /Ascent 500 /Descent -50
  /StemV 80 /FontFile3 5 0 R` and the stream `<< /Length 95 /Subtype
  /Type1C >>`. Every pre-existing `.ir` and `.pdf` golden is
  byte-identical. `pdffonts` lists `ZUDKKT+SynCFF Type 1C Builtin emb
  yes sub yes uni yes`, `pdfinfo` accepts it, `pdftotext` extracts `a`,
  and `pdftoppm` renders the glyph. The round-trip scenario is
  `a_type1c_subset_round_trips_through_the_engine` in
  `crates/remelt/tests/scenarios.rs`: it extracts the `FontFile3`,
  parses it with `CffProgram::parse`, and checks exactly `.notdef` and
  `a`, no subroutines, and `a`'s advance and outline against
  `corpus_cff()`'s program.
- **Verification.** `cargo test --workspace`: 596 passed, 2 ignored
  (588 before; eight new tests); `cargo test -p ps-fonts
  --no-default-features`: 105 passed, 1 ignored; clippy clean on all
  targets with and without the feature; `cargo fmt --check` clean;
  `difftest run` 121 of 121 with `EFTERSCRIPT_PDF_CHECK=pdfinfo`;
  `parse-survival` 121 files, no failures; `openspec validate cff-fonts`
  valid.
- **Known limits.** A Type 2 charstring whose call operand is computed
  (`div`, `index`, …) takes the stub tier; a glyph reached only through
  such a call is still correct since nothing is renumbered then. Masks
  in dead code are sized statically. A `glyphshow` of a name outside the
  encoding is recorded under code 0 (text-core's limit), as for Type 1.

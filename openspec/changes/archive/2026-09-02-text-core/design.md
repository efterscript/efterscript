# Design: Text core

See proposal.md for motivation and the three spec deltas for the
contract. This document fixes the shapes: where the font lives, how a
glyph is shown, how Type 3 glyphs are captured, what the IR and PDF
carry.

## Context

- The VM/graphics boundary passes numbers and small value types. The
  graphics state (line width, colour, clip, current path) lives in
  `ps-graphics` behind the trait; the VM does not see it. Font
  dictionaries are VM objects.
- `image` already runs a PostScript procedure from inside an operator via
  a `LoopFrame` variant, and errors, `stopped`, and limits flow through
  it. Type 3 glyph procedures need exactly that mechanism.
- The IR transforms path points through the CTM as they are built, so a
  glyph procedure's paths would land in default user space unless capture
  says otherwise.
- The Core 14 AFM files (widths, bounding boxes, built-in encodings,
  kerning) and the Adobe Glyph List are redistributable under their own
  grants and move into `crates/ps-fonts/data/` with those grants and a
  provenance note.

## Goals / Non-Goals

**Goals:**
- Text correct at the metric level for the fourteen fonts and exact for
  Type 3 fonts; text runs, not glyph-by-glyph fills, in the IR and PDF.
- One capture mechanism for glyphs that later serves forms and
  transparency groups.
- Jobs that reference fonts they do not embed keep running.

**Non-Goals:**
- Outlines of any kind; `charpath` waits for `font-programs`.
- Type 0/CID fonts, CMaps, `cshow`; kerning application; font caching for
  speed (there is nothing to rasterise).
- Any use of the AFM kerning pairs (kept in the data, unparsed until
  needed).

## Decisions

**D1. The font is in the graphics state as `FontRef { instance: u32,
matrix: Matrix }`.** The backend stores it beside the line width and
`gsave`/`grestore`/`grestore_to` handle it for free; `save`/`restore`
follow through the existing depth plumbing. The VM keeps a table
`instance → font dictionary object` and hands the backend the instance
id and the dictionary's `FontMatrix`. `currentfont` looks the instance
up; the table is append-only per job and entries are never dereferenced
after a `restore` invalidates the dictionary, because the graphics state
that pointed at them has been restored too. *Alternative:* the VM keeps
a parallel font stack synchronised with `gsave` — two stacks to keep
aligned, and `grestore_to` would need mirroring.

**D2. Resident fonts are ordinary dictionaries with a marker.** On first
`findfont` of a standard name the VM materialises a read-only Type 1
dictionary: `FontType 1`, `FontName`, `FontMatrix [0.001 0 0 0.001 0
0]`, `FontBBox`, `PaintType 0`, `Encoding` (the shared `StandardEncoding`
array, or the font's own for Symbol and ZapfDingbats), `FID`, and
`/ResidentFont <index>` naming the metrics. Programs that copy and
re-encode a font (`dup length dict copy`, new `Encoding`, `definefont`)
carry the marker along, so the copy still measures with the right widths
and serialises to the same base font. `definefont` on a dictionary
without the marker and with `FontType` 1 or 42 succeeds structurally,
but `show` with it raises `invalidfont` — its glyphs cannot be measured
or drawn until `font-programs`. *Alternative:* a fourth object type for
resident fonts — invisible to the program's dictionary operators, which
real jobs rely on.

**D3. Showing is a loop frame.** `LoopFrame::Show { font, string, next,
variant, pending }` where `variant` is the show operator and its extra
operands (`ashow` deltas, `widthshow` char and delta, `kshow` procedure,
`xshow`/`yshow`/`xyshow` displacement array with its index) and
`pending` is the glyph run being assembled. Resident fonts consume the
whole string in one step; Type 3 fonts take one glyph per step:
`gsave`, set the CTM to `FontMatrix × CTM` translated to the current
point, tell the backend to begin capturing (D4), push the font
dictionary and glyph name (or code for `BuildChar`), execute the
procedure, and on its return read the width recorded by
`setcachedevice`/`setcharwidth`, end capture, `grestore`, advance, and
continue. `stringwidth` runs the same frame with capture in measuring
mode (nothing recorded) and no page marks. `kshow` flushes the pending
run before its procedure runs, since the procedure may change colour or
position. A `Show` frame counts toward the execution-stack limit like
any loop. *Alternative:* a Rust loop calling the interpreter re-entrantly
— the interpreter-core design forbids native recursion.

**D4. Glyph capture in the backend.** New trait methods
`begin_glyph(font: FontRef, code: u8, name: &[u8], measure: bool)` and
`end_glyph(width: (f32, f32), bbox: Option<Bounds>)`. Between them the
backend redirects emission into a glyph procedure: paths are transformed
by `CTM × base⁻¹` where `base` is the CTM at `begin_glyph`, so captured
coordinates are in glyph space; the lazy-emission state starts fresh (a
CharProc is self-contained); nested text inside a glyph is allowed and
captured as a nested `Text` op. `end_glyph` stores the procedure in the
page's Type 3 font resource under the glyph name, deduplicating by
structural equality, and the glyph joins the pending run. In measuring
mode nothing is stored. A `setcachedevice` glyph is by definition
colour-independent; a `setcharwidth` glyph may set its own colour, and
its captured procedure keeps those settings, so PDF's `d0` form is used
for it and `d1` with the bounding box for cached glyphs. Capture happens
on every occurrence (PostScript semantics, no glyph cache) and
deduplication makes the result one procedure. *Alternative:* cache
glyphs per font by name after the first `setcachedevice` — faster, but a
program that changes the font dictionary's behaviour between shows
would be misrepresented, and nothing here is slow yet.

**D5. The `Text` operation.** `IrOp::Text { font: FontRef(resource
index), matrix: Matrix, glyphs: Vec<Glyph { code: u8, dx: f32, dy: f32
}> }`. `matrix` maps glyph space to default user space at the first
glyph (`FontMatrix × CTM` translated to the current point); `dx`/`dy`
are the displacement applied after each glyph in glyph units — the
glyph's width plus whatever `ashow`/`widthshow`/`xshow` added, taken
back through the font matrix. One `show` operator call is one `Text` op
(kshow: one per segment). The run is self-describing: a reader can
position every glyph from the matrix and the displacements alone.
Colour and clip are emitted lazily before the op as for a fill.

**D6. Font resources.** `FontSpec::Resident { base: StdFont, encoding:
Box<[Option<GlyphName>; 256]> }` and `FontSpec::Type3 { font_matrix,
font_bbox, encoding, glyphs: BTreeMap<GlyphName, GlyphProc { ops:
Vec<Op>, width: (f32, f32), bbox: Option<Bounds> }> }`. Resident fonts
intern per page by base and encoding, so a scaled font is not a new
resource (the scale is in the `Text` matrix). A Type 3 font interns per
page by the VM font's `FID` (its instance family) together with its
encoding, since a re-encoded copy keeps the `FID`, and its glyph map
grows as glyphs are captured; the dump and the serializer see the final
map.
`StdFont` is an enum of the fourteen; widths come from `ps-fonts` by
glyph name, so the IR carries no width tables.

**D7. Dump lines, `ir/1` unchanged.** Resources: `font <n> <BaseName>`
followed by `diff=[<code> /<name> …]` only for codes differing from the
built-in encoding; `font <n> type3 <a> <b> <c> <d> <tx> <ty> bbox=[…]`
followed by `glyph /<name> <wx> <wy> [bbox …] {` … indented op lines …
`}`. Ops: `text <n> <a> <b> <c> <d> <tx> <ty> (<bytes>) <dx> <dy>…`. The
grammar is additive, so existing goldens are byte-identical and the
version stays `ir/1`. *Alternative:* `ir/2` — signals a change to readers
that need none.

**D8. Metrics from the AFMs at first use.** `ps-fonts` includes the
fourteen AFM files with `include_str!` and parses each on first access
(`OnceLock`), yielding widths by glyph name, the bounding box, and the
built-in encoding. The files are unmodified so the grant's condition
holds; the licence file sits beside them and a `PROVENANCE.md` records
source, commit, and grant. `stringwidth` sums widths of
`Encoding[code]`, treating a name absent from the font as width 0 (the
notdef). *Alternative:* a build script generating tables — a build
dependency and generated code for a 700 KB text corpus that parses in
microseconds.

**D9. Substitution is a pure function `substitute(name) -> StdFont`.**
Strip a subset tag (`ABCDEF+`), split the family from the style at the
first `-` or `,`, then: exact and alias matches first (Arial → Helvetica,
TimesNewRoman → Times, CourierNew → Courier, with `MT`/`PS`/`PSMT`
suffixes and the LaserWriter 35 families mapped by class: Palatino,
Bookman, NewCenturySchlbk, Garamond → Times; AvantGarde, Optima,
Univers → Helvetica; ZapfChancery → Times-Italic); then hints in the
whole name: Symbol → Symbol, Dingbat → ZapfDingbats, Mono/Courier/
Typewriter/Console → Courier, Serif/Roman/Times/Book/Georgia/Century →
Times, otherwise Helvetica; style from Bold/Black/Heavy/Semibold and
Italic/Oblique (Symbol and ZapfDingbats have one style). Controlled by
`Config::fonts.substitute` (default `true`); `false` makes `findfont`
raise `invalidfont` as the reference specifies. Recorded as an expected
divergence in the spec.

**D10. Resource machinery, minimal.** A `Category` is a name with three
operations (find, status, forall) plus a per-category dictionary for
`defineresource`. `Font` reads `FontDirectory` and `GlobalFontDirectory`
first and the resident set second (status 2 for resident, 0 for
defined); `Encoding` serves the two arrays. `findresource` on `Font`
never substitutes — substitution is `findfont`'s behaviour. Other
categories raise `undefined`. *Alternative:* the full generic
`/Category` resource model with implicit resources and
`resourceforall` templates — needed later for `Pattern`, `Form`, and
CID resources, and easy to grow into from this.

**D11. PDF text.** A `Text` op writes `BT`, `/Fn 1 Tf`, `Tm`, glyphs,
`ET`. For a resident font `Tm` is the IR matrix with its linear part
scaled by 1000 (glyph space is thousandths of text space); for a Type 3
font `Tm` is `FontMatrix⁻¹ × matrix` and the PDF `FontMatrix` is the
font's own, so CharProcs stay in glyph space. Glyphs whose displacement
equals their width go into one `Tj`; a horizontal-only difference
becomes a `TJ` adjustment in thousandths of text space (for Type 3
through the font matrix's x scale); any vertical difference or a
non-axis font matrix ends the run and repositions with `Td`. A font with
a singular `FontMatrix` draws nothing and its text ops are skipped with
a note in the report.

**D12. Font objects once per document.** `PdfSink` keeps a map from a
structural key (resident: base + encoding; Type 3: the whole spec) to
the written font object; page resources reference by id. ToUnicode:
for each code with a glyph name, the name is mapped through the glyph
list (also `uniXXXX`/`uXXXX[XX]` forms and `name.suffix` by its stem);
`bfchar` entries, one CMap stream per font, deterministic order. Widths
for the resident fonts are written (optional for the standard fourteen,
but they make viewers lay out with the metrics the program used), and
with them a `FontDescriptor` built from the AFM header, since ISO
32000-1 Table 111 lets a standard font omit first char, last char,
widths, and descriptor only all together. *(Amended in part 2: the
original text omitted the descriptor.)*

**D13. Encodings.** `StandardEncoding` and `ISOLatin1Encoding` are built
from tables in `ps-fonts` (the standard encoding also from the AFMs'
codes; the two must agree, checked by a test), allocated once in global
VM as read-only arrays and entered in `systemdict`. Symbol and
ZapfDingbats encodings come from their AFM `C` codes.

## Risks / Trade-offs

- [A glyph procedure that changes the page (calls `showpage`) or nests
  deeply] → the backend refuses page operations while capturing
  (`invalidaccess`), matching the reference's prohibition; nesting is
  bounded by the execution-stack limit through the existing frame
  machinery.
- [The AFM widths are for the standard encoding's glyph set; a
  re-encoding to a name outside the font gives width 0 and a notdef in
  PDF] → this is what the real font would do; ToUnicode omits it.
- [Substitution hides wrong output] → the report counts substituted
  fonts and the CLI prints them on standard error, so a missing font is
  visible without failing the job.
- [Binary grows by ~700 KB of metrics] → acceptable for now; a compact
  table can replace `include_str!` without API change if size matters
  for the WASM target.
- [`kshow`/`xshow` runs split into many text ops] → correct first; the
  serializer's `TJ` merging keeps most of them compact.

## Open Questions

- Whether `resourceforall` on `Font` should list job-defined fonts before
  or after the resident set; the reference leaves order to the
  implementation, and the corpus scenario pins sorted order for the
  resident set only.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.

### Part 1

Covers the data promotion and the metrics crate, the boundary additions,
and the VM side: font dictionaries, the show frame, Type 3 execution,
substitution, the resource operators, and the encodings. The graphics
crate carries only the stubs the corpus needs; the IR and PDF are part 2.

- **Data (1.1).** `crates/ps-fonts/data/core14/` (fourteen AFMs and the
  licence) and `data/glyphlist.txt` are byte-identical to the vault; the
  CRLF line endings are kept. `data/PROVENANCE.md` records source,
  commit, retrieval date, the grants' conditions, and every SHA-256.
  `tests/provenance.rs` carries its own SHA-256 and compares against the
  vault's `SHA256SUMS` when `EFTERSCRIPT_HELLBOX` names an existing
  checkout; unset, or set to a path without `SHA256SUMS`, it prints a
  skip message and passes. A second test checks the note lists every
  file with its current checksum.
- **ps-fonts API (1.2–1.4).** `Afm<'a>::parse` reads the header values
  (`FontName`, `FontBBox`, `EncodingScheme`, weight, family, fixed pitch,
  italic angle, heights) and the `C … ; WX … ; N … ; B … ;` lines up to
  `EndCharMetrics`; kerning is left unread. `StdFont` is the fourteen in
  sorted PostScript-name order, with `index()`/`from_index()` (the value
  of the `ResidentFont` marker), `metrics()` (`OnceLock` per font over
  `include_str!`), `width(glyph) -> Option<u16>`, `bbox() -> [f32; 4]`,
  `builtin_encoding()` (`&STANDARD_ENCODING` for the text fonts, the AFM
  codes for Symbol and ZapfDingbats), `family()`, `is_bold()`,
  `is_italic()`, and `styled(family, bold, italic)`. `STANDARD_ENCODING`
  and `ISO_LATIN1_ENCODING` are `[Option<&'static str>; 256]` built in
  `const` context; a test holds the standard table against every text
  font's AFM codes (149 assigned codes; ISOLatin1 has 205). `unicode(name)
  -> Option<Vec<char>>` follows the glyph-list specification: list entry
  (which may be several code points), `uniXXXX…` groups and `uXXXX[XX]`
  in upper-case hex, `_`-joined components each mapped in turn, and a
  `.suffix` dropped; `.notdef`, unknown names, and malformed forms are
  `None`. The `_` ligature rule is beyond D12's list and is recorded here.
- **Substitution (D9) as built.** Exact match of the fourteen names first
  (case-sensitive), then case-insensitive alias lookup on the family part
  (before the first `-` or `,`, with `MT`/`PS`/`PSMT` stripped): Arial,
  ArialNarrow, CourierNew, TimesNewRoman, TimesRoman, Dingbats, and the
  LaserWriter families by class as D9 lists (NewCenturySchoolbook spelled
  out as well); ZapfChancery is Times-Italic outright. Without an alias
  the whole name is classified: `symbol`, `dingbat`, then `mono`/
  `courier`/`typewriter`/`console`, then `sans` (Helvetica, checked
  before `serif` so "SansSerif" is not serif), then `serif`/`roman`/
  `times`/`book`/`georgia`/`century`, else Helvetica. Weight hints are
  `bold`, `black`, `heavy`, `semibold`, and `demi` (added for Bookman);
  slope hints `italic` and `oblique`; hints are read from the style part
  when there is one, otherwise from the whole name.
- **Boundary (2.1).** `FontRef { instance, matrix }` and `Glyph { code,
  dx, dy }` are in `ps_vm::graphics`. The five methods have no default
  bodies: `set_font`/`font` store and return the state's font;
  `show(glyphs)` records the run at the current point and advances the
  current point by `Glyph::total(glyphs)` taken through the font matrix's
  linear part (`Matrix::apply_delta`), a user-space delta — the VM never
  moves the current point itself, so `show … currentpoint` and
  `stringwidth` agree by construction; `nocurrentpoint` without a point,
  `invalidfont` without a font. `begin_glyph(font, code, name, measure)`
  is called inside a `gsave` with the CTM set to `FontMatrix ×
  translation(origin) × CTM` (`origin` the glyph's position in user space)
  and an empty path; `end_glyph(width, bbox)` follows the procedure's
  return, then `grestore_to(depth before the gsave)`. A glyph procedure
  that does not return normally (error, `stop`, `exit`) is abandoned when
  its frame is popped: `end_glyph((0, 0), None)` then the `grestore_to`.
  Part 2 must accept that call as "discard the capture". The recording
  mock (`tests/common/mod.rs`, shared by the boundary tests) logs all five
  and advances its current point on `show`; `ps_graphics::Graphics` keeps
  the font in `GState`, advances the current point on `show` (a `moveto`),
  and ignores `begin_glyph`/`end_glyph` until part 2. `GState::
  reinitialized` now keeps the font, since `initgraphics` and `showpage`
  do not reset it (PLRM3 §8.2).
- **Which names need the backend.** `OPS` in `ops/font.rs` (public,
  always defined): `definefont`, `undefinefont`, `findfont`, `scalefont`,
  `makefont`, `setfont`, `currentfont`, `selectfont`, `stringwidth`,
  `setcachedevice`, `setcachedevice2`, `setcharwidth`; `ops/resource.rs`
  likewise. `PAINT_OPS` (graphics visibility, defined by
  `set_graphics_backend`): `show`, `ashow`, `widthshow`, `awidthshow`,
  `kshow`, `xshow`, `yshow`, `xyshow`, `glyphshow`, `charpath`. Without
  a backend the VM keeps the current font in its own slot
  (`Interp::current_font`), which `save`/`restore` do not touch — there is
  no graphics state to hold it; with a backend the slot is unused and the
  font lives in the graphics state.
- **Font instances (D1).** `Interp::font_instance(dict)` allocates the id
  on first `setfont` of a dictionary (keyed by its composite reference,
  which is never reissued) and `Interp::font_dict(instance)` looks it up;
  the table is never truncated.
- **Resident fonts (D2).** The dictionary has exactly `FontType 1`,
  `FontName`, `FontMatrix [0.001 0 0 0.001 0 0]` (the two scales reals,
  the rest integers), `FontBBox` (integers), `PaintType 0`, `Encoding`
  (the shared `StandardEncoding` object, or a global array from the AFM
  codes for Symbol and ZapfDingbats), `FID`, and `ResidentFont <index>`.
  It is global, read-only, built once per interpreter, and not entered
  in `FontDirectory`, so `resourcestatus` reports 2 before and after
  `findfont`. `definefont` requires `FontType` 1, 3, or 42, a six-number
  `FontMatrix`, a 256-element `Encoding`, and for Type 3 an executable
  `BuildGlyph` or `BuildChar`; anything else is `invalidfont`. An `FID`
  already present is kept (a font redefined under another key), else one
  is added (which needs write access). In global allocation mode the
  font is entered in both `GlobalFontDirectory` and `FontDirectory`, in
  local mode in `FontDirectory` only; `findfont` and the resource
  operators consult `FontDirectory` first, then `GlobalFontDirectory`;
  `undefinefont` removes from both. A local Type 3 font defined in global
  mode is `invalidaccess` from the directory insert.
- **Derived fonts.** `scalefont`/`makefont`/`selectfont` copy every entry
  into a new dictionary in the current VM, read-only, with `FontMatrix' =
  FontMatrix.then(transform)` (glyph space through the original matrix
  first, then the transform — showing with the derived font is showing
  with the original under `transform concat`), stored as six reals; the
  `FID` is shared, which D6's per-family interning relies on. So `10
  scalefont` of a `[1 0 0 1 0 0]` font prints `[10.0 0.0 0.0 10.0 0.0
  0.0]`, the numbers of the spec's scenario in the VM's real syntax.
  `setfont` accepts a dictionary with a readable `FontMatrix` and an
  `FID`, else `invalidfont`; `currentfont` with no font set raises
  `invalidfont` (the reference leaves the initial font unspecified).
- **The show frame (D3) as built.** `LoopFrame::Show(Box<ShowFrame>)`
  with `operator`, `font: FontRef`, `dict`, `kind` (`Resident(StdFont)`
  or `Type3 { build, by_name }`, decided from `FontType` and the
  `ResidentFont` marker when the operator starts — a Type 1/42
  dictionary without the marker is `invalidfont` there), `encoding`,
  `codes`, `variant` (`Show`, `AShow`, `WidthShow`, `AWidthShow`,
  `KShow`, `XShow`/`YShow`/`XYShow` with their numbers, `GlyphShow` with
  the name), `measure`, `next`, `pending`, `total` (glyph-space sum of
  the pending run), `origin` (the run's user-space start, read from the
  backend when a Type 3 glyph first needs it), and `running` (the Type 3
  glyph in progress: code, gstate depth, declared width and box). The
  loop hands a `Show` frame to `ops::show::step`, which takes the frame
  off the stack, advances it, and puts it back under the procedure it
  starts. Resident glyphs are consumed in one step; a Type 3 glyph is one
  step (frame back on the stack, then the font dictionary and the glyph
  name — `.notdef` when the encoding has none — or, for `BuildChar`, the
  code, then the procedure). When the procedure returns the width from
  `setcachedevice`/`setcachedevice2`/`setcharwidth` (found on the
  innermost frame with a running glyph; outside one those operators are
  `undefined`), or `(0, 0)` with no box if none was declared, is read.
  The run is flushed with one `show` at the end of the string and, for
  `kshow`, before the procedure runs after each glyph but the last, with
  the two codes pushed; the next segment starts at whatever current point
  the procedure leaves. `stringwidth` never flushes and pushes
  `FontMatrix.apply_delta(total)`: the width in user space, as the
  reference has it — the spec's "transformed by the font matrix and the
  CTM" reads loosely; `show` advances by the same value. Show frames are
  uncounted loop frames like every other loop (the glyph procedure's
  frame is counted, so nesting stays bounded), and `exit` inside a glyph
  or `kshow` procedure leaves the show.
- **Displacements (D5).** `ashow`/`widthshow`/`awidthshow` additions and
  `xshow`/`yshow`/`xyshow` replacements are user-space distances and are
  taken through the inverse font matrix into glyph units before joining
  the glyph's displacement; a singular font matrix maps them to nothing
  (and draws nothing under D11). So the xshow scenario's displacements
  are 1000, 2000, and 3000 glyph units at size 10 — the spec's "10, 20,
  30" are the user-space numbers. A short number array is `rangecheck`
  before any operand is popped; an encoded number string is `typecheck`.
  `widthshow`'s code is compared with the byte. `glyphshow` finds the
  name's code in the encoding; a name outside it is shown by name under
  code 0 for resident and `BuildGlyph` fonts and is `rangecheck` for a
  `BuildChar` font. A code whose encoding entry is not a name, or a name
  the resident metrics lack, is a notdef of width 0.
- **kshow's operands** are `proc string` (PLRM3 §8.2); the spec scenario
  writes them the other way round, a slip, and the corpus file uses the
  reference order.
- **Single precision.** `0.001 × 10` is not the `f32` nearest `0.01`, so
  scaled widths carry a relative residue of about 5·10⁻⁸ (`9.440001`
  for the re-encoding scenario). The corpus files round to a thousandth
  before printing; the numbers themselves are the scenarios'.
- **Substitution (D9).** `Config::fonts: FontConfig { substitute }`
  (default `true`); `Interp::font_substitutions()` lists every
  substituted `findfont`/`selectfont` as `FontSubstitution { requested,
  substitute }` for part 2's `Report`. Exact resident names are not
  recorded. `findresource` never substitutes.
- **Resources (D10).** `Category { local, global }` per category:
  `Font` is `FontDirectory`/`GlobalFontDirectory`, `Encoding` two
  dictionaries the program reaches only through `defineresource`.
  `defineresource` on `Font` is `definefont` (a non-dictionary instance
  is `typecheck`); on `Encoding` it requires a 256-element array and
  stores it in the dictionary of the current allocation mode.
  `resourcestatus` reports 0 for a defined instance and for the two
  built-in encodings and 2 for a resident font, size 0 throughout;
  `findresource` and an unknown category raise `undefined`, as the spec
  says (no `undefinedresource` error was added). `resourceforall` runs as
  `LoopFrame::ResourceForAll`: the matching names are collected up front
  — the program's definitions in insertion order, local before global,
  then the built-in set in sorted order, which settles the open question
  — and each is written into the scratch string (too short:
  `rangecheck`; read-only: `invalidaccess`) before the body runs with the
  interval. Templates support `*`, `?`, and `\`.
- **Encodings (D13).** Both arrays are global read-only arrays of 256
  names with `.notdef` at unassigned codes, built at construction and
  entered raw into `systemdict` beside `FontDirectory` (local) and
  `GlobalFontDirectory` (global).
- **`invalidfont`** is a new `VmError` with the usual default handler.
- **Corpus.** `corpus/unit/text/` holds the part-1 scenarios that need
  only printed output (widths, advance, re-encoding, aliases,
  heuristics, resident status, encodings, `kshow`, invalid dictionaries,
  derived fonts, a Type 1 dictionary without a program, fonts without a
  backend, Type 3 widths); none produces a page, so none has a golden.
  The Type 3 painting scenarios and the IR scenarios wait for part 2 and
  are covered in part 1 by `crates/ps-vm/tests/text.rs` through the mock.
- **For part 2.** The font resource of a `Text` op comes from
  `FontRef.instance` through the VM's `ResidentFont` marker
  (`StdFont::from_index`) or the Type 3 dictionary's `FID`; the matrix
  D5 wants is `font.matrix.then(translation(current point)).then(ctm)`
  taken at `show` time; `Glyph.dx`/`dy` are already glyph units;
  `begin_glyph` receives the glyph name bytes and the `measure` flag;
  an `end_glyph((0, 0), None)` may follow an abandoned glyph and should
  discard it; `Interp::font_substitutions()` feeds the report.

### Part 2

Covers the graphics crate (font resources, the `Text` operation, glyph
capture, the dump), the PDF side (font objects, text objects, the
report), the corpus goldens, and the command-line substitution line.

- **How the backend learns a font (D1, D6).** The trait gained one
  method with a default body, `define_font(instance, &FontInfo)`, and
  `ps_vm::graphics` two value types: `FontSource::{Resident(StdFont),
  Type3 { family, font_matrix, font_bbox }}` and `FontInfo { source,
  encoding: Vec<Option<Vec<u8>>> }`. `show::begin` sends the description
  once per instance per backend (`Interp::described_fonts`, cleared when
  a backend is installed), reading the encoding at that moment; a
  program that writes into its encoding array after the first show is
  not followed. `Interp::defined_matrices` records the `FontMatrix` each
  `FID` was first defined with (`definefont`; a derived font redefined
  under another key keeps its base), which is the `font_matrix` a Type 3
  `FontInfo` carries — a scaled `FontRef` cannot give the base back, and
  the PDF `FontMatrix` must be the font's own for the spec's `[0.001 0 0
  0.001 0 0]`. The recording mock is untouched. `ps_fonts::Afm` gained
  `std_vw` (`StdVW`), for the font descriptor. These are the only
  changes to `ps-vm` and `ps-fonts`.
- **IR (D5, D6).** `FontIndex(usize)` — `FontRef` is the VM's type —
  `GlyphName = Vec<u8>`, `GlyphNames = Box<[Option<GlyphName>; 256]>`
  built by `glyph_names`, which turns `.notdef` into `None`; `GlyphProc
  { ops, width, bbox }`; `FontSpec::{Resident, Type3}` with
  `width(code)`, `font_matrix()`, `glyph_name(code)`;
  `Resources::{fonts, intern_font, add_font}`; `IrOp::Text { font,
  matrix, glyphs }`. The text matrix is `FontMatrix.then(CTM)` with its
  translation replaced by the device-space current point (plus the
  CTM-mapped font translation), and the advance is applied in device
  space: no inverse CTM, so a run at a translated origin prints exact
  numbers. A per-page `instance → FontIndex` cache keeps repeated shows
  from rebuilding the 256-name encoding.
- **Capture (D4).** `begin_glyph` swaps the page's operation list and
  emitter out; geometry is mapped through the inverse of the CTM at
  `begin_glyph`, computed and applied in double precision (an `f32`
  round trip printed `99.9999`); a stroke's CTM, an image's matrix, and a
  nested run's matrix are composed with the same inverse. The glyph's
  emitter starts from the *inherited* state values — only the glyph's
  own changes are recorded, so a `d0` glyph takes the text colour at
  render time and two shows in different colours capture equal
  procedures — with the colour space marked as not yet named: a colour
  set inside the glyph without a space change names the space first, so
  the CharProc stands alone. The emitter has a clip floor (the clip
  depth at `begin_glyph`); only entries above it open inside the glyph.
  Text in a Type 3 font flushes every setting (line parameters too),
  resident text colour and flatness, so the state PDF has at `Tj` is the
  state the glyphs inherited. `end_glyph((0, 0), None)` discards — an
  abandoned procedure and one that declared nothing are the same call;
  a `0 0 setcharwidth` glyph with marks is lost, a gap to close when the
  boundary grows a distinct abandon signal. Measuring stores and interns
  nothing; a singular glyph CTM stores nothing; a name already captured
  keeps its first procedure. Page operations while capturing
  (`showpage`, `copypage`, `erasepage`, `set_media_box`) are
  `invalidaccess`. A nested run is recorded inside the glyph; its font
  is interned first, so a Type 3 font showing a resident font gets the
  higher index.
- **Dump (D7).** `font <n> <BaseName>[ diff=[<code> /<name> …]]`, with
  `/.notdef` where the program removed a glyph the base has; `font <n>
  type3 <matrix> bbox=[…] enc=[<code> /<name> …]` listing the codes of
  the captured glyphs — an addition to D7, so the code-to-name mapping
  the PDF `Differences` carries is visible — then `glyph /<name> <wx>
  <wy> [<box>] {` and the procedure's lines indented by two spaces;
  `text <n> <matrix> (<bytes>) <dx> <dy>…`. Names escape the bytes that
  would end a token as octal. Every pre-existing golden is byte-identical.
- **Font objects (D12).** Keyed by the `FontSpec` together with the
  values of the colour spaces, images, and fonts its glyph procedures
  name (glyph ops refer to page resources by index, so structural
  equality of the spec alone is not enough across pages; nested
  references are followed one level). Object ids are allocated for all
  of a page's new fonts before any is written, so a glyph's nested run
  can refer to a font written later. Page resources name fonts `/Fn` by
  `FontIndex`. Resident: `/Type1 /BaseFont`, `FirstChar`/`LastChar`/
  `Widths` over the codes with a glyph name (0 for a name the metrics
  lack), `/Encoding << /Differences >>` only when a code differs from
  the base's built-in encoding (runs of consecutive codes), `/ToUnicode`
  for every code whose name maps (bfchar blocks of at most a hundred,
  UTF-16BE), and — the deviation from the original D12 — a
  `FontDescriptor` from the AFM header (`Flags` from fixed pitch, family,
  symbolic, italic; `FontBBox`, `ItalicAngle`, `Ascent`/`Descent` from
  the ascender and descender or the box, `CapHeight` when stated,
  `StemV` from `StdVW`), because the standard fourteen may omit those
  four entries only together. Type 3: `FontMatrix` (the font's own),
  `FontBBox`, `CharProcs` in glyph-name order opening with `wx wy d0`
  or `wx wy llx lly urx ury d1`, `Encoding` `Differences` and
  `FirstChar`/`LastChar`/`Widths` (glyph space) over the captured codes
  (`0 0 [0]` when nothing was captured), `Resources` restricted to what
  the procedures name, `ToUnicode` over the captured codes.
- **Text objects (D11).** `BT`, `/Fn 1 Tf`, `Tm` (resident: the IR
  matrix with its linear part × 1000; Type 3: `FontMatrix⁻¹ × matrix`),
  glyphs, `ET`. Positions are tracked in double-precision text space:
  PDF's own advance (`wx × FontMatrix.a`) against the recorded
  displacement through the font matrix. Before a glyph whose wanted
  position differs horizontally only, a `TJ` number `(pen − wanted) ×
  1000`; a vertical difference (also what a non-axis Type 3 matrix
  produces) ends the piece and moves the line with `Td` relative to the
  line start. The displacement after the last glyph affects nothing.
  Strings are literal when printable ASCII, hexadecimal otherwise. A
  singular Type 3 font matrix skips the run with the note `page N: text
  in font Fn skipped: its font matrix is singular`.
- **Report and CLI.** `Report` gained `substitutions:
  Vec<FontSubstitution>` (from `Interp::font_substitutions()`) and
  `notes: Vec<String>`; `PdfSink::notes()` exposes the latter. The CLI
  prints `efterscript: N font(s) substituted: A -> B, …` as one line on
  standard error when there are any, and each note on its own line.
- **Corpus.** `text-operation-shape`, `type3-square-glyph`,
  `type3-stringwidth-paints-nothing`, `type3-glyph-shown-twice`,
  `type3-glyph-space`, `text-under-clip-and-colour`, `xshow-text-op`,
  `fonts-shared-across-pages`, and `substitution-arial`, each with `.ir`
  and `.pdf` goldens. With the external checker as `EFTERSCRIPT_PDF_CHECK` every
  corpus document is accepted, and its text extraction extracts `Hi` from the
  text-operation-shape golden.
- **Known limits.** `glyphshow` of a name outside the encoding is
  recorded under code 0 with the resource's code-0 name, so it draws a
  notdef in PDF; a Type 3 glyph procedure whose `gsave`/`grestore` are
  unbalanced can pop the clip below the capture's floor, which is
  treated as no clip inside the glyph.

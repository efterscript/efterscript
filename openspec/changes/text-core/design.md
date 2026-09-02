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
page by the VM font's `FID` (its instance family) and its glyph map grows
as glyphs are captured; the dump and the serializer see the final map.
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
but they make viewers lay out with the metrics the program used). The
`FontDescriptor` is omitted, as the standard fourteen allow.

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

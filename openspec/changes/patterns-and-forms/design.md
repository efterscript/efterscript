# Design: Patterns, forms, and user paths

See proposal.md and the spec deltas. This document fixes how a
pattern is a colour across the boundary, how the Type 3 capture
mechanism becomes the general one, where patterns and forms live in
the IR, what the writer emits, and how user paths are interpreted.

## Context

- The backend already captures a Type 3 glyph by redirecting emission
  (`begin_glyph`/`end_glyph`): the page's operation list is swapped
  out, geometry is taken back through the CTM in effect at the start
  so the procedure is in its own space, the emitter starts from the
  inherited state, and page operators are refused. The VM runs the
  glyph procedure as a loop frame and resumes the operator afterwards.
- Colour crosses the boundary as a `SpaceSpec` plus a component
  vector; the IR interns spaces per page; the writer names them
  `/CSn` and selects device spaces directly.
- Paths reach the IR transformed into default user space; `Image` and
  `Text` operations carry the matrix that places them. Resources are
  per page.
- Regular resource categories other than the font ones are not yet
  generic: `CATEGORIES` lists twelve names and `defineresource` knows
  the implemented ones.
- The scanner does not decode binary tokens; user paths in the encoded
  form arrive as ordinary strings the operator must decode itself.

## Goals / Non-Goals

**Goals:** patterns and forms preserved as PDF pattern and form
objects; one capture mechanism for glyphs, cells, and forms; user
paths with no IR change; nothing in the existing goldens changes.

**Non-Goals:** shading patterns and `shfill`; stroke outlining
(`strokepath`, `ustrokepath`); form or pattern reuse across pages in
the PDF (each page carries its own objects, as fonts do today); the
`Implementation` key; halftone or transfer interaction with patterns.

## Decisions

**D1. A pattern is a colour with a reference.** `SpaceSpec` gains
`Pattern { base: Option<Box<SpaceSpec>> }` (`components()` is the
base's arity or 0). The trait gains `fn set_pattern(&mut self, pattern:
&PatternInfo, components: &[f32]) -> Result<(), VmError>` beside
`set_color`, where `PatternInfo { id: u64, matrix: Matrix, bbox:
Bounds, xstep, ystep, paint_type, tiling_type }` is the instance's
value part (the matrix already concatenated with the CTM at
`makepattern`, so it is pattern space to default user space); the
`GState` keeps `Option<PatternInfo>` beside the components;
`current_color` reports the components and the VM keeps the instance
object to return. `set_color` on a pattern space without a pattern
operand is `typecheck`. *Alternative:* pattern as a component-vector
encoding (an id smuggled as a float) — brittle and unreadable in the
dump. *Alternative:* the backend owning the pattern dictionary — it
would need object access, which the boundary forbids.

**D2. The instance.** `makepattern` copies the dictionary (per PLRM3
§4.9.1 the result is a new dictionary) into a new read-only dict with
the same entries plus `Implementation` holding the instance id and the
concatenated matrix; the VM keeps an `id → (dict, matrix)` table in the
interpreter (not the memory pool, so save/restore does not tear it —
ids are never reused, and an instance made inside a `save` and used
after `restore` is `invalidaccess` when its dict is gone, as a font
would be). `setpattern` reads the instance id back from the
dictionary. The `Pattern` and `Form` categories are regular categories
with a type check on `defineresource` (pattern dictionary shape / form
dictionary shape); `CATEGORIES` gains both.

**D3. One capture mechanism.** The backend's `Capture` becomes generic:
`Capture { target: Target, outer_ops, outer_emitter, ctm_at_begin,
… }` with `Target::Glyph { … }`, `Target::Pattern(id)`, and
`Target::Form(id)`. `begin_glyph` is unchanged; two additive trait
pairs are added: `begin_pattern_cell(&mut self, pattern: &PatternInfo)
-> Result<bool>` (returns `false` and captures nothing when the page
already holds this pattern's cell, so the VM skips the procedure) /
`end_pattern_cell()`, and `begin_form(&mut self, form: &FormInfo) ->
Result<bool>` / `end_form()` / `place_form(&mut self, id)`, where
`FormInfo { id: u64, bbox: Bounds, matrix: Matrix }` (`matrix` =
`Matrix` × CTM at the call: form space to default user space). At
`begin_*` the backend does the `gsave`, sets the CTM to the target's
space, resets the clip to the box (a cell's `BBox` in pattern space; a
form's `BBox` in form space), and for a coloured pattern cell resets
the colour to the initial black; the VM does the matching `grestore_to`.
Nested captures push; a pattern set inside a capture has its
`PatternInfo.matrix` taken back through the capture's CTM at begin,
like geometry, so a pattern inside a form or cell is expressed in that
resource's space — the writer's pattern matrix then maps to the
enclosing form space, which is what ISO 32000-1 §8.7.3.1 requires.
*Alternative:* separate mechanisms per target — three copies of the
swap-and-restore logic.

**D4. Capture at first paint, not at `makepattern`.** A pattern's
procedure is run by the first painting operator that uses the colour
on a page (fill, eofill, stroke, the rect operators, show-family text,
imagemask, `ufill`…): the operator asks `begin_pattern_cell`; on
`true` the VM pushes a `LoopFrame::PatternCell { proc, dict, depth }`
that runs `PaintProc` with the dictionary as operand, then `end_
pattern_cell` and `grestore_to(depth)`, and re-dispatches the painting
operator (the same re-run shape as the image and filter data frames).
The procedure for an uncoloured cell runs with the colour operators
answering `undefined` (a VM flag set for the frame, per PLRM3 §4.9.2).
*Alternative:* run at `makepattern` — the page may not exist yet and
the resources are per page. *Alternative:* run at `setpattern` — a
pattern set and never painted would still be captured.

**D5. Forms.** `execform` validates, computes `FormInfo` (id from the
dictionary's identity — its handle — so the same dictionary is one
form; `Implementation` is not consulted), and asks `begin_form`. On
`true` it pushes `LoopFrame::FormBody { proc, dict, depth }` running
`PaintProc` under capture, then `end_form`; in both cases it then
calls `place_form(id)`, which emits `IrOp::Form { form: FormIndex,
matrix }` with the matrix taken back through any enclosing capture.
The IR `Resources` gains `forms: Vec<FormSpec>` (`FormSpec { bbox,
ops: Vec<Op> }`) and `patterns: Vec<PatternSpec>` (`PatternSpec {
matrix, bbox, xstep, ystep, paint_type, tiling_type, ops }`); `IrOp`
gains `Form { form, matrix }` and `SetPattern { pattern: PatternIndex,
components: Vec<f32> }` (emitted in place of `SetColor`; the emitter's
dedup treats the pair space+pattern+components as the colour). Ops
inside resources index the page's tables, so interning is unchanged;
the dump prints the resources after fonts as `pattern n …` / `form n
…` blocks with their content indented, and a paint line with a pattern
colour is preceded by `pattern n [components]` the way `color` is.

**D6. The writer.** Patterns become `/Pn` objects: a stream with
`/PatternType 1 /PaintType /TilingType /BBox /XStep /YStep /Matrix
/Resources`, content rendered by the existing content renderer over
the cell's ops; forms become `/Fmn` XObjects with `/Subtype /Form
/BBox /Matrix [1 0 0 1 0 0] /Resources`; each resource dictionary lists
only what that content references (the writer already computes per-
content reference sets for Type 3 glyph procedures — reuse). The page
content selects `/Pattern cs /Pn scn` (coloured) or `/CSn cs c… /Pn
scn` with `CSn = [/Pattern base]` (uncoloured), and the stroking twins
`CS`/`SCN`; a placement is `q <matrix> cm /Fmn Do Q`, as an image is.
`Flate` container as for every stream.

**D7. User paths.** `ops/upath.rs` interprets a user path with a small
walker over either form: the literal array (numbers and executable
names among the eleven allowed) or the encoded pair (a homogeneous
number array decoded per PLRM3 §3.14.5 — 16/32-bit integers and fixed-
point, IEEE and native reals, both byte orders — and an operator
string with repeat counts). The walker drives the existing backend
path calls (`moveto`, `rmoveto`, `lineto`, `rlineto`, `curveto`,
`rcurveto`, `arc`, `arcn`, `arcto`, `closepath`) and checks each
absolute point against the `setbbox` box (`rangecheck` outside; the
box must be first, else `typecheck`). `ufill`/`ueofill`/`ustroke` are
`gsave newpath uappend <paint> grestore`, `ustroke` with a matrix
concatenating it after `newpath`; `upath` reads the current path back
from the backend (`current_path`) into an array in *current* user
space (the inverse CTM applied — `upath` describes the path in the
program's coordinates) headed by its bounding box and `setbbox`, the
array executable when `true` (cacheable) and literal otherwise —
record the exact choice in the notes after checking against the
reference converter. `setbbox` outside a user path records the box on
the VM side only. `ucachestatus` answers a mark and five zeros
(nothing is cached); `setucacheparams` pops to the mark.

**D8. Corpus.** `corpus/unit/patterns/`, `corpus/unit/forms/`,
`corpus/unit/upath/`, one file per spec scenario plus: a pattern used
for a stroke, a pattern painting text, a pattern used inside a form,
a form with text and an image, a pattern instance across `save`/
`restore`, the errors. IR and PDF goldens; the oracle tier over all
three directories; the DCT and filter goldens untouched.

## Risks / Trade-offs

- [The pattern matrix inside a capture] → covered by the nested
  scenario and an oracle comparison of a pattern inside a translated
  form.
- [Re-dispatching a painting operator after the cell capture] →
  operands are saved on the frame and restored before re-dispatch, as
  the image frame does; a procedure that raises leaves the operator's
  operands popped, which is the reference's behaviour too.
- [The emitter's colour dedup across `SetPattern`] → the dedup key
  includes the pattern index and components; tested by the two-fills
  scenario.
- [Encoded user paths decoded by hand] → all number representations
  get unit vectors; the oracle compares the painted result.
- [Form identity by dictionary handle] → a job that mutates and
  re-executes a form dictionary on one page sees the first body; noted
  as a limit, no known job does it.

## Open Questions

- Whether `upath` should emit `ucache` first for the cacheable form or
  simply mark the array executable; decide by black-box observation
  and record in the notes.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.
Part 1 covered tasks 1.1, 1.2, 4.1, and 4.2: the boundary types, the
two regular categories, and the user-path operators in both forms.
Provenance: PLRM3 §4.6 (user paths, all of it), §3.14.1 and §3.14.5
(homogeneous number arrays and encoded number strings), §4.9.1–4.9.2
and §4.7.1 (the dictionary shapes), and the §8.2 entries for `setbbox`,
`uappend`, `ucache`, `ucachestatus`, `ufill`, `ueofill`, `upath`,
`ustroke`, `setucacheparams`, `setcolorspace`, `setcolor`,
`currentcolor`, `currentcolorspace`, and `defineresource`; and black-box
observation of the reference converter through `difftest oracle` with
scratch programs that never entered the repository ("observed" below).

- **D1 as built.** `SpaceSpec::Pattern { base: Option<Box<SpaceSpec>> }`
  with `components()` the base's arity or 0, `family()` `"Pattern"`,
  `initial_color()` the base's or empty, and a new `component_space()`
  (the base, or the space itself) that the graphics state uses to clamp
  components. `PatternInfo` and `FormInfo` are as D1 and D3 give them;
  the six hooks (`set_pattern`, `begin_pattern_cell`/`end_pattern_cell`,
  `begin_form`/`end_form`/`place_form`) have defaults that accept and
  capture nothing, so the recording backend and the real one compile
  unchanged. `setcolorspace` accepts `/Pattern`, `[/Pattern]`, and
  `[/Pattern base]`; the base is any space the parser already accepts
  except a pattern space, which is `typecheck` (observed; an Indexed base
  is accepted, as observed — whether it can paint is the instance's
  concern), three elements `rangecheck`, a non-space base `typecheck`.
  `setcolor` in a pattern space: an operand that is not a dictionary is
  `typecheck`, a dictionary without `PatternType` `undefined` (observed),
  and any dictionary `typecheck` until part 2 makes instances.
  *Deviation from the part brief:* `currentcolor` in a pattern space
  pushes a single `null`, not the base's components — PLRM3 §4.9.1 makes
  the initial colour a null object standing for an empty pattern, and
  the reference prints exactly one `null` with nothing under it for both
  `[/Pattern]` and `[/Pattern /DeviceRGB]`; part 2 pushes the components
  and the instance once `setcolor`/`setpattern` set one. The device
  queries (`currentgray` and friends) read black, as observed. The
  `ColorSpaceFamily` category lists `Pattern` (seven names).
- **The interim downstream.** The graphics crate stores the space like
  any other (the emitter names it and records an empty colour); the
  dump prints `Pattern` or `Pattern base=<space>`. `remelt` writes a
  pattern space as its base's colour-space form (`DeviceGray` for a
  coloured pattern), selects the base in the content stream, writes
  `0 g`/`0 G` for an empty component list, and notes once per content
  "pattern colour not yet written: painted in the underlying colour";
  part 3 replaces both. No golden carries a pattern space yet, since
  nothing can paint in one before part 2.
- **D2 as built.** `Kind::Pattern` and `Kind::Form` in `ops/resource.rs`
  over `Interp::pattern_category` and `form_category` (a local and a
  global dictionary each, like the other regular categories); no
  built-in instances; `defineresource` runs `pattern::check_dict` or
  `form::check_dict` (new modules `ops/pattern.rs` and `ops/form.rs`,
  which part 2's `makepattern` and `execform` reuse), makes the instance
  read-only as the `defineresource` entry says, and stores it in the
  dictionary of the allocation mode; `undefineresource`,
  `resourcestatus`, `resourceforall`, and `findresource` share the
  generic paths. The shape check: `PatternType`/`FormType` 1,
  `PaintType` 1–2, `TilingType` 1–3, steps non-zero and finite, `BBox`
  four numbers with the upper corner strictly above and right of the
  lower (else `rangecheck`, the "empty box" of the spec deltas),
  `Matrix` six numbers (a wrong length is `typecheck` here, a shape
  error, where the matrix operators say `rangecheck`), `PaintProc` an
  executable array; a wrong type is `typecheck`, a value outside its
  range `rangecheck`, and an absent entry the error the caller passes —
  `typecheck` from `defineresource` (the dictionary is not of the
  category's type, per the entry) and `undefined` from part 2's
  operators, per the spec deltas. Observed: the reference's
  `defineresource` accepts any dictionary in both categories (a
  `PatternType` of 2, a string, a five-element `Matrix`, all pass), so
  the check is this interpreter's and `pattern-category-shape.ps` and
  `form-category-shape.ps` carry `% oracle: skip` saying so. The
  reference reports size −1 for a defined instance where every regular
  category here reports 0; the category files print the boolean only.
  `CATEGORIES` has fourteen names and the identity test counts them.
- **D7 as built: the walker.** `ops/upath.rs` reads a user path into
  steps first (`parse`: shape and types, no graphics state touched) and
  then runs a `Walker` over the backend. Shape: an array of at least
  five elements is the literal form, of exactly two the encoded form,
  anything else `typecheck` (observed for `[1 2 3]` and `[]`). Literal
  elements are numbers, executable names, or operator objects (a bound
  user path) among the twelve operators of §4.6.1 — `arct`, not `arcto`
  — and every other element is `typecheck`, including a literal name
  and `arcto`; an operator with the wrong number of numbers before it,
  or numbers left at the end, is `typecheck` (observed in both
  directions; `stackunderflow` never arises because nothing is
  executed). Structure: `ucache` only first and once, `setbbox` exactly
  once before any construction (a second one `typecheck`, observed; an
  inverted box `rangecheck`, observed), and the first construction
  operator one of `moveto`, `arc`, `arcn`, else `typecheck` per §4.6.1
  — the reference answers `rangecheck` for a leading `lineto`; the unit
  test pins the manual's `typecheck` and no corpus file covers it.
  `arct` is also defined as an ordinary graphics operator (`arcto`
  without its results), which the reference has too.
- **The bounding-box rule.** From the `setbbox` entry: the box's corners
  go through the CTM in effect and the check is against the axis-aligned
  envelope in default user space, with a tolerance of 1e-3 units for the
  trip through the matrix. Every coordinate the path reaches is checked
  there: the point of `moveto`/`lineto`, the current point plus the
  offset for the relative forms, all three points of a curve, the two
  corners given to `arct` (the tangent points and the arc between them
  lie in the corners' hull with the current point), and for `arc`/`arcn`
  the figure of the arc — its two ends and the point at every quarter
  turn the sweep passes — rather than the control points of the curves
  it becomes, whose overshoot depends on how finely a backend cuts the
  sweep. Observed: the reference accepts `{0 0 10 10 setbbox 5 5 5 45
  135 arc}`, whose single 90° piece would put a control point at y ≈
  10.5. The centre of an arc is not checked, not being on the figure.
- **Painting and appending.** `ufill`/`ueofill`/`ustroke` are `gsave`,
  `newpath`, the walk, the matrix concatenated for `ustroke`'s second
  form, the paint, and `grestore_to` the depth before the `gsave`,
  whether or not the walk succeeds; a failing walk leaves the operands
  on the stack. The matrix operand is recognised by shape — an array of
  exactly six numbers — so a five-element array or one with a string is
  taken for the user path and fails as one (observed). No IR changes:
  the goldens under `corpus/golden/ir/upath` are page-for-page identical
  to the pages the same files build by hand. The §4.6.4 note about
  rounding the CTM's translation to whole device pixels is a
  scan-conversion measure and is not applied; the distiller keeps the
  geometry exact.
- **The `upath` shape (the open question).** Settled by the entry and
  observation: an executable array; `ucache` first when the operand is
  true, as the entry's sketch has it, and nothing else marks
  cacheability; then the bounding box of every point the stored path
  has — curve control points and a trailing `moveto` included, all
  zeros for an empty path, which is not an error (the reference prints
  `{0.0 0.0 0.0 0.0 setbbox}`) — and `setbbox`; then the segments with
  real coordinates in the current user space (the stored path taken
  back through the inverse CTM) and the executable names `moveto`,
  `lineto`, `curveto`, `closepath`. The box is the path's own, not one a
  previous `uappend` declared: after appending a path that declares `0
  0 100 100`, the reference reports `10 10 90 90`, so the round-trip
  scenario in the user-paths spec has been amended to say the path's
  box. `==` prints the reals as `10.0`, and the printed arrays agree
  with the reference byte for byte.
- **`setbbox` outside a user path** checks its operands (an inverted box
  `rangecheck`, a non-number `typecheck`) and records the box in the
  interpreter (`Interp::declared_path_bbox`, cleared by `newpath`) and
  nothing more. Observed: the reference also enforces it on the
  construction that follows (`0 0 1 1 setbbox 5 5 moveto` is
  `rangecheck`) and on `pathbbox`; that needs the box in the graphics
  state and is left for a later change, so no corpus file exercises it.
  `ucache` does nothing; `ucachestatus` leaves a mark and five zeros
  (the reference's numbers differ, so files print the count);
  `setucacheparams` pops to the mark and is `unmatchedmark` without
  one.
- **4.2 as built: encoded user paths.** `crates/ps-vm/src/numbers.rs`
  decodes a homogeneous number array: the type byte 149, the
  representation byte, and a two-byte count; representations 0–31
  (32-bit fixed point, that many fraction bits), 32–47 (16-bit, 32
  fewer), 48 (IEEE single), 49 (native real), and 128 more than each of
  those for the low-order byte first, the count included; scale 0 gives
  integers, anything else reals. *The "native" decision:* a native real
  is an IEEE single with the low-order byte first on every host, under
  either flag — this interpreter has no host-specific number format,
  every target it builds for is little-endian, a job must behave the
  same everywhere, and the reference on this host reads 49 and 177 that
  way (observed with a program that printed the decoded values through
  `upath`). Errors, all observed: a first byte other than 149
  `typecheck`; representations 50–127 and 178–255 `rangecheck`; a
  string whose length is not the header plus exactly the declared
  numbers `rangecheck`, shorter or longer. The operator string: codes
  0–11 in the order of §4.6.2's table, 33–255 a repeat count for the
  next code (32 fewer), 12–32 `typecheck`; a count followed by a count
  is replaced by it, a count with nothing after it is ignored, numbers
  the operators never consume are ignored, and running out of numbers
  is `typecheck` — the last four as observed. The reference aborts on
  an operator code outside the table, so `encoded-bad-opcode.ps` carries
  `% oracle: skip`. The number sequence may also be an ordinary array
  of numbers (§4.6.2); anything else in either position is `typecheck`.
  Unit vectors for every representation and both orders are in the
  module's tests, built from the format description; the corpus paints
  the same square from seven representations with one golden.
- **Corpus, part 1.** `corpus/unit/upath/`: `ufill-round-trip`,
  `ustroke-plain`, `ustroke-matrix`, `ustroke-bad-matrix`,
  `uappend-existing`, `ueofill-star`, `arc-in-path`, `out-of-box`,
  `upath-empty`, `upath-user-space`, `cache-operators`,
  `square-literal`, `square-encoded` (32-bit fixed point, eight
  fraction bits), `encoded-repeat`, `encoded-representations`,
  `encoded-bad-opcode`, `encoded-data-exhausted`,
  `encoded-bad-number-string`, and the four `error-*` files; the IR and
  PDF goldens of `square-encoded` and `encoded-repeat` are byte-identical
  to `square-literal`'s. `corpus/unit/patterns/pattern-category.ps` and
  `corpus/unit/forms/form-category.ps` cover define, find, status,
  enumerate, and undefine (part 2 extends them with `makepattern` and
  `execform`); the two `*-shape.ps` files the checks. Oracle tier
  (`difftest oracle --profile default` over the three directories): 26
  files, 23 pass, 0 fail, 3 skipped (the two shape files and the bad
  opcode, reasons above); output same for every compared file except
  the two category files, where the reference's `findresource` on the
  `Category` category returns its implementation dictionary — the files
  now print `resourcestatus`'s boolean instead.
- **Tests.** `crates/ps-vm/tests/upath.rs` (the call sequences, every
  structural and box error, the encoded form's errors and leftovers,
  `arct`, `setbbox` outside a path, the cache operators, the graphics
  group membership), the pattern-space test in `tests/graphics.rs`, the
  category test in `tests/identity.rs`, the decoder vectors in
  `numbers.rs`, the walker's helpers in `upath.rs`, the space arity and
  default hooks in `graphics.rs`.
- **Owed by parts 2 and 3.** `makepattern`, `setpattern`, `setcolor`
  with an instance (and `currentcolor` pushing components then the
  instance), the id table, `LoopFrame::PatternCell` and the uncoloured
  colour-operator rule, `execform` and `LoopFrame::FormBody`, the
  generic capture in the graphics crate, `IrOp::SetPattern` and
  `IrOp::Form` with their dump lines, the writer's pattern and form
  objects replacing the interim gray paint and its note, the remaining
  D8 corpus, and, if a job needs it, `setbbox` enforced outside user
  paths.

Part 2 covered tasks 2.1, 2.2, 3.1, and 5.1: the instances and the
pattern colour, the cell capture at the first paint, forms, and the
generic capture with its IR and dump. Provenance: PLRM3 §4.7 and §4.7.1
(forms), §4.9.1–4.9.2 (patterns; §4.9.3 only to know what is refused),
§4.3 (what a saved state restores), and the §8.2 entries for
`makepattern`, `setpattern`, `setcolor`, `setcolorspace`,
`currentcolor`, `currentcolorspace`, and `execform`; ISO 32000-1
§8.6.6.2 (the pattern space), §8.7.2–8.7.3 (the pattern matrix and its
relation to the enclosing content's space, the cell's starting state),
and §8.10 (form XObjects); and black-box observation of the reference
converter through `difftest oracle` with scratch programs that never
entered the repository ("observed" below).

- **D2 as built: the instance.** `makepattern` reads the operand matrix
  first (`typecheck`/`rangecheck` as `read_matrix` gives them, observed
  the same), then the prototype through `pattern::shape` (an absent
  entry `undefined`, a wrong type `typecheck`, a value out of range
  `rangecheck` — all observed, except that the reference answers a
  `PatternType` of 2 with `undefined`, since it implements shadings, so
  `shading-out-of-range.ps` carries `% oracle: skip`). The instance is a
  new dictionary in *local* VM whatever the allocation mode (the entry
  says so and the reference's instance answers `gcheck` false in global
  mode, observed), holding the prototype's entries — the composite
  values shared, not copied — plus `Implementation`, an integer that is
  the instance id, and made read-only; the prototype gains nothing
  (observed: length 7 before and after, the instance 8). The reference's
  `Implementation` is an array; the entry leaves the type to the
  implementation. Ids are indices into `Interp::pattern_instances`, a
  table of `(dictionary, PatternInfo)` outside the memory pool, never
  truncated; `PatternInfo.matrix` is the operand matrix followed by the
  CTM at the call — the identity without a graphics backend, which is
  why `makepattern` is a public operator while `setpattern` and
  `execform` belong to the graphics group (`matrix` itself is a graphics
  operator, so backend-less corpus files write the literal identity).
  An operand naming an instance (`setpattern`, `setcolor`): not a
  dictionary `typecheck`; no `Implementation` entry `undefined` (a
  prototype was given; observed); an `Implementation` that is not a
  live id of this interpreter, or one carried by a dictionary other
  than the instance's own, `typecheck` (observed for `setpattern`; the
  reference's `setcolor` says `rangecheck` there). A discarded
  instance: `dict_get` on a dictionary `restore` reclaimed is already
  `invalidaccess`, and the paint-time lookup by id checks the
  dictionary is still there — a guard only, since no program can hold
  such an instance: it is local, `restore` refuses a newer local object
  on the stacks, and the graphics state that held it as a colour is
  restored too.
- **`setpattern` as observed.** The entry's own sketch is followed: with
  a current space that is not a pattern space, `setcolorspace` to
  `[/Pattern current-space]` (which resets the colour, as it does for
  any space), then `setcolor`; a current pattern space stays whatever
  its base (observed: an uncoloured pattern set through
  `[/Pattern /DeviceRGB]` followed by a coloured `setpattern` leaves the
  base). The reference prints the base in the array form it was given —
  `[/Pattern [/DeviceRGB]]` after `setrgbcolor P setpattern` — where
  this interpreter prints `[/Pattern /DeviceRGB]`, since a `SpaceSpec`
  keeps no syntax; the corpus files do not print `currentcolorspace`
  after `setpattern`. The colour operand: a coloured instance takes the
  dictionary alone in any space (numbers under it stay, observed: three
  remain), an uncoloured one the base's components under it (fewer is
  `stackunderflow`, a non-number `typecheck`) and, with no base,
  `rangecheck` per the entry — the reference accepts it (observed) and
  the corpus does not exercise the case with the oracle. Components
  are clamped by the backend as for `setcolor`.
- **`currentcolor` and the trait.** In a pattern space `currentcolor`
  pushes the base's components (integers for an Indexed base) and then
  the instance dictionary, per §4.9.2's colour value; before any
  instance is set, the single `null` of §4.9.1 as part 1 had it. The
  reference's shape differs: it pushes the last n objects of the value
  where n is the number of numeric components, so a coloured instance
  yields nothing and an uncoloured RGB one `g b dict` (observed); the
  manual's shape is implemented and `currentcolor-shape.ps` carries
  `% oracle: skip`. To map the current colour back to its dictionary
  the trait gained `current_pattern(&self) -> Option<PatternInfo>`
  (default `None`), an addition to D1, whose "the VM keeps the instance
  object" would otherwise need a parallel graphics-state stack in the
  VM: the backend's state holds the instance through `gsave`/`grestore`
  and `save`/`restore`, and the VM turns the id into the dictionary.
  The device queries (`currentgray` and friends) still read black.
- **D4 as built: the frames.** `LoopFrame::PatternCell { body, dict,
  depth, operator, uncoloured, started }` and `LoopFrame::FormBody {
  body, dict, depth, info, started }`. Every painting operator that
  uses the current colour calls `pattern::capture_cell` after checking
  its operands and before consuming anything: `fill`, `eofill`,
  `stroke`, `rectfill`, `rectstroke`, the nine show operators (in
  `show::start`, after the font is described; not for `stringwidth` or
  `charpath`), `imagemask` (before any data is read; not `image`), and
  `ufill`/`ueofill`/`ustroke` after the walk is parsed. With a pattern
  colour whose cell the backend does not hold, the backend saves the
  state and prepares the cell, the frame is pushed, and the operator
  returns. The frame's first step pushes the dictionary and the
  procedure; its second clears `started`, pops itself, ends the capture,
  restores the state to `depth`, and pushes the operator object, which
  runs again over the operands — *left on the operand stack the whole
  time*, a deviation from D4's "saved on the frame": observed, the
  reference leaves `rectfill`'s four operands in place when the paint
  procedure raises (nine objects: three of the test's, the failing
  operator's two, the four operands), so nothing is saved and nothing
  restored. The re-run asks `begin_pattern_cell` again and is answered
  `false`. A frame discarded while `started` (an error, `stop`, or
  `exit` inside the procedure) ends the capture and restores the state
  from `pop_frame`, and the operator does not run again. The reference
  runs the procedure at the first paint, not at `setpattern`, and never
  again — not for a second fill, a stroke, text, a mask, nor after
  `showpage` or `erasepage` (observed); here the cell is captured once
  per page and again after `erasepage`, since resources are per page.
- **The uncoloured rule.** `Interp::uncoloured_cells` counts the
  uncoloured cell frames on the execution stack (kept in
  `push_frame_unchecked`/`pop_frame`), and while it is non-zero
  `setgray`, `setrgbcolor`, `sethsbcolor`, `setcmykcolor`,
  `setcolorspace`, `setcolor`, `setpattern`, `image`, and `colorimage`
  are `undefined` (§4.9.2 permits `imagemask` but not `image` or
  `colorimage`; §4.8.1 names the error), a form body executed inside
  the cell included, and a `gsave`d colour change is no exception. The
  reference enforces none of it (observed: every one succeeds), so
  `uncoloured-colour-operator.ps` carries `% oracle: skip`. Likewise
  `Interp::paint_procedures` counts cell and body frames, and
  `showpage`, `copypage`, `erasepage`, and `setpagedevice` are
  `undefined` inside either (the reference allows them, observed; two
  corpus files skip). `nulldevice` is left alone.
- **D5 as built: forms.** A form is its dictionary: the id is the
  handle with the space bit, which the memory pool never reissues
  (`restore` keeps the allocation counter), so two dictionaries with
  equal entries are two forms and one dictionary through two names is
  one. `execform` validates through `form::shape` (`undefined` for an
  absent entry; the reference agrees for `PaintProc`, answers
  `undefined` for `FormType` 2 where this says `rangecheck`, accepts
  `BBox [0 0 0 10]` where part 1's check says `rangecheck`, and says
  `rangecheck` for a five-element `Matrix` where part 1's check says
  `typecheck` — all observed, none exercised by the oracle), then gives
  the dictionary an `Implementation` entry past its access and makes it
  read-only (both observed on the reference), computes `FormInfo` with
  `Matrix` followed by the CTM, asks `begin_form`, and pops its operand;
  on `true` the `FormBody` frame runs the procedure and, when it
  returns, ends the capture, restores the state, and places the form;
  on `false` the form is placed at once. An error in the body ends the
  capture, restores the state, and places nothing (the reference
  restores the state too, observed; its stack afterwards is not
  determinable). Nested forms, forms inside cells, and cells inside
  forms go through the capture stack, and the same form executed inside
  its own body is placed by the outer execution only (the backend
  answers `false` for a form being captured and records no placement of
  it inside itself).
- **D3 as built: the capture.** `Capture { target: Target::{Glyph {
  font, code, name, measure }, Pattern(PatternInfo), Form(FormInfo) },
  to_target, outer_ops, outer_emitter }`; `begin_capture`/`end_capture`
  do the swap for all three, `end_glyph`/`end_pattern_cell`/`end_form`
  refuse (`invalidaccess`) when the innermost capture is another kind.
  `begin_pattern_cell` answers `false` under the null device, when the
  page holds the cell, or when that cell is being captured (a cell
  painting with its own pattern); otherwise it saves the state, sets
  the state to `reinitialized()` with the pattern matrix as CTM (the
  media box, null device, font, screens, and transfers kept; colour
  black, line parameters default, clip and path empty), starts the
  emitter from the initial state, and clips to the box in pattern
  space. That is a departure from D3's "inherit, reset the colour": a
  PDF pattern stream starts from its parent content stream's *initial*
  state (ISO 32000-1 §8.7.3.1), so a cell that inherited the page's
  line width and recorded no `w` would be wrong in the PDF, and PLRM3
  §4.9.2 installs the state at `makepattern` time (observed: line width
  3 and the colour set before `makepattern`, not the 7 and gray set
  after), which no hook carries across the boundary; the defaults are
  the state a cell can rely on, and the emitter starting there makes the
  cell's stream self-contained. `begin_form` answers `false` likewise
  and otherwise saves the state, sets the CTM to the form matrix,
  empties the path, keeps everything else (PLRM3 §4.7's sketch:
  `gsave`, `concat`, `rectclip`, `newpath`), starts the emitter from
  the inherited state as a glyph does, and clips to the box in form
  space. `place_form` flushes every setting (colour, flatness, line
  parameters) before `IrOp::Form`, so a body relying on inherited
  settings finds them set before the `Do` the writer will emit. The
  null pattern — a pattern space with no instance — paints nothing per
  §4.9.1: fills, strokes, text, and masks record nothing (the current
  point still advances), images still paint.
- **The nested pattern matrix.** `PatternInfo.matrix` maps pattern
  space to default user space; inside a capture the resource's matrix
  is that matrix followed by the inverse of the capture's CTM at begin
  (`local_matrix`, the same double-precision inverse geometry goes
  through), so it maps pattern space to the captured form's or cell's
  space — what §8.7.2 requires of a pattern used within a form XObject
  (its matrix maps to the form's space) or within another pattern (to
  the outer pattern's space). Since that matrix depends on the context,
  a pattern resource is made per `(instance id, context)` — page,
  glyph (font instance and code), pattern id, or form id — in
  `placed_patterns`, while the cell captured for the instance
  (`page_patterns`) is one: a second context clones its operations,
  which are context-free, with its own matrix. So one instance filled
  inside a form and on the page is two `PatternSpec`s over one captured
  cell (`pattern-inside-form.ps`), a refinement of D3/D5, which spoke
  of one resource per instance. A paint in a context whose resource
  does not exist yet — only reachable when the backend refused the
  capture — gets an empty cell. All three tables are cleared with the
  page and by `erasepage`.
- **IR and dump.** `Resources { patterns: Vec<PatternSpec>, forms:
  Vec<FormSpec> }` with `PatternSpec { matrix, bbox, xstep, ystep,
  paint_type, tiling_type, ops }` and `FormSpec { bbox, ops }`;
  `IrOp::SetPattern { pattern: PatternIndex, components }` in place of
  `SetColor` (the emitter's colour key is the space, the pattern index,
  and the components; a capture inherits no pattern, so a pattern
  colour in effect when a form begins is set again inside it, in the
  form's own resource) and `IrOp::Form { form: FormIndex, matrix }`
  (the matrix taken through the enclosing capture like an image's).
  The dump lists `pattern n matrix … bbox … step … paint … tiling … {`
  and `form n bbox … {` blocks after the fonts with their operations
  indented, a `pattern n [components]` line where `sc` would be, and
  `form n a b c d tx ty` for a placement; pages without either dump as
  before (`difftest run`: 257 files pass, every pre-existing golden
  byte-identical).
- **The interim writer.** `remelt` writes `SetPattern` as the
  components in the base space the interim colour space selected —
  `0 g`/`0 G` for a coloured pattern — with the part 1 note, and a
  placement as nothing, noting once per content how many were skipped.
  The PDF goldens of the new corpus files under `corpus/golden/pdf/
  {patterns,forms}` are therefore the interim writer's; they are
  regenerated when the writer arrives, and the rendering files carry
  `% oracle: skip` naming the writer until then. That the mismatch is
  the writer's and not the interpreter's: the IR goldens were checked
  against the scenarios by hand, the reference agrees with every
  observable the interpreter has (the `output: same` files), and the
  pattern files compared without the skip fail the raster comparison
  by 0.7–6.9% of the pixels, a solid fill in the base colour where the
  reference tiles the cell. The form files write nothing where the
  reference paints the body; `form-placement.ps`, whose body is the
  unit box the spec names, differs by fewer pixels than the raster
  threshold sees at 36 dpi and would pass either way, so nothing about
  forms follows from its verdict until the writer exists.
- **Corpus, part 2.** `corpus/unit/patterns/`: `instance-made`,
  `shading-out-of-range`, `makepattern-missing-entry`,
  `makepattern-zero-step`, `makepattern-wrong-type`,
  `setpattern-prototype`, `coloured-fill` (also "A pattern in the
  dump"), `uncoloured-components`, `uncoloured-colour-operator`,
  `one-capture-two-fills`, `cell-clipped-to-box`, `currentcolor-shape`,
  `page-operator-in-cell`, `cell-raises-restores`,
  `defined-pattern-paints`; `corpus/unit/forms/`: `form-placement`,
  `bad-form`, `three-placements`, `form-placed-twice` ("A form
  placement in the dump"), `nested-forms`, `pattern-inside-form`
  ("Capture nests"), `defined-form-paints`, `page-operator-in-form`,
  `form-raises-restores`. IR goldens for the eleven that paint. Oracle
  tier over both directories: 28 files, 10 pass (output same), 0 fail,
  18 skipped — nine for the writer, the two shape files from part 1,
  and seven where the reference departs from the manual as recorded
  above.
- **Tests.** `crates/ps-vm/tests/patterns.rs` (the instance, the CTM at
  `makepattern`, every error, the space rule, `setcolor`, the
  `currentcolor` shape, the capture and re-run for every painting
  operator and none for the measuring ones, the uncoloured rule, a
  raising procedure, per-page capture, page operators, the colour
  through `gsave`/`save`) and `tests/forms.rs` (capture once per page,
  identity by dictionary, the alterations, every error, nesting both
  ways, a raising body, page operators); the recording mock captures
  once per page and starts a cell from the initial colour as the real
  backend does; `crates/ps-graphics/tests/backend.rs` (per-page and
  `erasepage` reset, the dedup key, the null pattern, a pattern inside
  a form with the page's second resource, nested forms, refusals under
  the wrong capture and the null device); `state.rs` unit tests for
  `set_pattern`.
- **Owed by part 3.** The writer: `/Pn` objects from `PatternSpec` with
  the resource's matrix as written (it is already relative to the
  content that names it), `/Fmn` XObjects from `FormSpec`, per-content
  resource dictionaries that list the patterns and forms a content
  names — `fonts::collect` gathers spaces, images, and fonts only and
  must learn `SetPattern` and `Form` — and `scn`/`SCN` with the
  components; the remaining D8 corpus (a pattern stroke, pattern text,
  a form with text and an image, a pattern instance across
  `save`/`restore`); regenerating the interim PDF goldens and removing
  the writer skips; and, if a job needs it, `setbbox` enforced outside
  user paths. A Type 3 glyph painting with a pattern goes through the
  same mechanism (context `Glyph`) but has no corpus file.

Part 3 covered tasks 6.1, 7.1, and 7.2: the writer, the remaining
corpus, the divergence slugs, and the verification. Provenance: ISO
32000-1 §7.8.3 (resource dictionaries and which content stream's
dictionary lists what), §8.6.6.2 (the pattern space), §8.6.8 (Table 74:
`cs`/`CS`, `scn`/`SCN` and their operands in a pattern space),
§8.7.3 (Table 75, the tiling pattern stream's entries, the coloured and
uncoloured forms of `scn`), §8.10 (Table 95, the form dictionary and
what `Do` does with it); PLRM3 §3.7.3 for what `restore` does to an
instance; and the oracle tier over the three directories.

- **D6 as built: the objects.** `resources::Objects` gained
  `patterns: Vec<Ref>` and `forms: Vec<Ref>`. A `PatternSpec` becomes a
  stream `/Pn` with `/Type /Pattern /PatternType 1 /PaintType
  /TilingType /BBox /XStep /YStep /Matrix /Resources` (`Type` is
  optional by Table 75 and written so the object names its kind;
  `Resources` is required there and always written, an empty
  dictionary when the cell names nothing); the matrix is the
  resource's as the IR carries it, already relative to the content
  that names it. A `FormSpec` becomes `/Fmn` with `/Type /XObject
  /Subtype /Form /BBox /Matrix [1 0 0 1 0 0] /Resources`, the
  identity because every placement carries its own matrix. Both
  contents are `content::render` over the resource's operations
  against the page's resources, so a cell that shows text, a body
  that places another form, or either painting with a pattern is
  written by the same code as the page. The ids are allocated before
  `write_fonts` runs (a Type 3 glyph procedure may name a pattern or
  place a form) and the streams written after it (a cell may show
  text in a font). Their text streams take the page's filter as every
  other text stream does. `pdf-out` is untouched: `write_stream` with
  its extra-entries closure was the helper D6 allowed for, and nothing
  was missing.
- **Per-content resource dictionaries.** `fonts::Refs` gained
  `patterns` and `forms`; `Refs::of(ops, resources)` gathers what a
  content names directly, and `collect(…, deep)` walks into each
  pattern and form named the first time it is met (a cell that names
  itself, which the backend gives an empty cell, ends the walk). The
  direct set is what a pattern's, a form's, and a Type 3 font's
  `Resources` lists: by §7.8.3 a content stream's dictionary holds the
  resources that stream uses, and by §8.10.2 (Table 95) a form's
  resources are not promoted to the stream that paints it — so a form
  placed inside a form is named by the outer body's dictionary and
  what it uses in turn by its own. The deep set is what the Type 3
  `Key` compares, since the meaning of a glyph that paints with a
  pattern depends on what the cell names in turn; `Key` gained
  `patterns: Vec<PatternSpec>` and `forms: Vec<FormSpec>`. The page's
  own dictionary keeps its existing policy and lists every resource
  the page holds, patterns and forms included, as it lists every
  image and font (restricting it would have changed pre-existing
  goldens with an image used only in a glyph). `resources_dict` writes
  `ColorSpace`, `XObject` (the images, then the forms), `Font`, and
  `Pattern`, each only when it has an entry; the pattern space without
  a base is `Form::Pattern { base: None }`, selected by name and
  listed nowhere (`is_direct`, the renamed `is_device`), while
  `[/Pattern base]` is a `/CSn` resource with the base's function
  streams written as for any space.
- **The content.** `ColorOp::Pattern` takes `scn`/`SCN`; a pattern
  space with a base is selected as `/CSn cs`, one without as
  `/Pattern cs` (Table 74: that name always denotes the space itself).
  `IrOp::SetPattern` is written as `[c1 … cn] /Pn scn` and the same
  with `SCN`, following the rule that the stroking colour is set with
  the non-stroking one, so a stroke and a text run paint with the
  pattern as a fill does. `IrOp::Form` is `q a b c d tx ty cm /Fmn Do
  Q` on one line, as an image is. `IrOp::SetColor` under a pattern
  space is noted (`colour components without a pattern in a pattern
  space: not written`) rather than written, since the operands of
  `scn` there must include a name and the interpreter never delivers
  the case (the null pattern paints nothing, and `setcolor` without an
  instance is `typecheck`). The interim `interim()` mapping, the
  pattern note, and the skipped-placement counter are gone, and with
  them the two interim notes.
- **Downsampling and embed-all.** `downsample::painted` now walks
  the page's operations into the forms they place and the patterns
  they paint with, composing the matrices as the reader does: an
  image at `I` inside a body placed by `M` is painted through `I·M`,
  one inside a cell whose pattern has matrix `P` through `I·P·outer`
  (every tile is a translation of the same resolution), nested to any
  depth with a guard against a resource reaching itself. So an image
  inside a form or cell is measured where it lands and is reduced
  under the parameters like one on the page; only an image painted
  solely inside a glyph procedure keeps its `Unsupported` note.
  `embedded::cids_used` walks the cells and bodies too, so an embedded
  or composite font shown inside one has those glyphs in its subset;
  embed-all needed nothing, since a resident face is deferred by
  `write_fonts` per page resource wherever it is shown.
- **Corpus, part 3.** `corpus/unit/patterns/`: `pattern-stroke` (a
  pattern as the stroke colour), `pattern-text` (a resident font shown
  in a pattern), `instance-across-restore`, `uncoloured-without-base`
  (`rangecheck`, the `uncoloured-pattern-without-base` slug);
  `corpus/unit/forms/`: `form-with-text-and-image` (a body showing a
  resident font and painting a gray image, placed twice; the XObject's
  dictionary names `/Im0` and `/F0`). `forms/pattern-inside-form` from
  part 2 already covers the matrix-through-capture case with a
  translated form (the cell's matrix inside the form is `1 0 0 1 -50
  -50`), so no variant was added. IR and PDF goldens for the four that
  paint. *The save/restore finding:* the brief's "made inside `save`,
  used after `restore` is `invalidaccess`" cannot be written as a
  program. The instance dictionary is made in local VM whatever the
  allocation mode (part 2, following the `makepattern` entry), and
  `restore` discards it together with every local reference to it —
  the name it was defined under, a dictionary it was put in, the
  graphics state that held it as the colour — while a global
  dictionary cannot hold it (`invalidaccess` at `put`, PLRM3 §3.7.3)
  and the operand stack cannot carry it across (`invalidrestore`).
  The guard in `current_instance` therefore stays a defensive check
  on the interpreter's own id table, and the corpus file shows the two
  reachable outcomes: an instance made before `save`, set and painted
  inside it, set again after `restore` paints from the cell the page
  already captured (one resource, two fills, `currentcolor` still the
  instance), and the name of one made inside `save` is `undefined`
  afterwards. The reference agrees on both (`output: same`).
- **The slugs applied.** `% oracle: skip …` became `% divergence:
  <slug>` in `patterns/uncoloured-colour-operator`
  (`uncoloured-cell-colour-operators`), `patterns/page-operator-in-
  cell` and `forms/page-operator-in-form` (`capture-refuses-page-
  operators`), `patterns/currentcolor-shape` (`pattern-currentcolor-
  order`), `patterns/pattern-category-shape` and `forms/form-category-
  shape` (`resource-instance-shape`), and the new
  `patterns/uncoloured-without-base` declares `uncoloured-pattern-
  without-base`. The eleven "PDF writer does not yet emit" skips (five
  pattern files, six form files — part 2's notes counted nine) were
  removed. `Registry::paths` already reads the open change's delta
  beside the living registry, so the slugs resolve; the skip-list
  self-test in `oracle.rs` lists the five new slugs with their counts
  and the shorter skipped list (`patterns/shading-out-of-range` keeps
  its skip).
- **Regenerated goldens.** The eleven interim PDF goldens, and only
  those: `corpus/golden/pdf/patterns/{cell-clipped-to-box,
  coloured-fill, defined-pattern-paints, one-capture-two-fills,
  uncoloured-components}.pdf` and `corpus/golden/pdf/forms/
  {defined-form-paints, form-placed-twice, form-placement,
  nested-forms, pattern-inside-form, three-placements}.pdf`. No IR
  golden changed and no golden that existed at HEAD changed
  (`difftest run` 262 of 262; `git status` shows the pre-existing
  goldens untouched).
- **Tests.** `content.rs` (the three selections — `/Pattern`, `/CSn`
  with a coloured and with an uncoloured pattern — the `scn`/`SCN`
  lines, the placement line, the not-written note), `resources.rs`
  (names, which spaces are direct), `fonts.rs` (direct references
  against the deep set and the `Key`), `downsample.rs` (matrices
  composed through nested forms and a cell), and two sink tests in
  `tests/sink.rs` reading the objects back through the structural
  reader: the pattern dictionaries' entries, the cell's own `Font`
  resource, the uncoloured cell's empty resources, the page's colour
  spaces; the form XObject's entries, `Do` under each placement matrix,
  the inner form's `ColorSpace`, `Pattern`, and image resources.
  `tests/support` gained `pattern`.
- **Oracle tier** (`difftest oracle --profile default` over
  `corpus/unit/patterns`, `corpus/unit/forms`, `corpus/unit/upath`):
  55 files, 46 pass, 0 fail, 5 expected-divergence, 2
  divergence-closed, 2 skipped; output 51 same, 2 differs, 0
  unavailable. Every file that paints a pattern or places a form
  passes the raster comparison, including the nine that were skipped
  for the writer and the four new ones. Expected divergences:
  `page-operator-in-cell` and `page-operator-in-form`
  (`capture-refuses-page-operators`, the reference ends normally),
  `uncoloured-colour-operator` (`uncoloured-cell-colour-operators`,
  the reference paints a page), `pattern-category-shape` and
  `form-category-shape` (`resource-instance-shape`, the output differs
  where the reference defines what this interpreter refuses).
  *Divergence-closed, reported for the registry's owner:*
  `currentcolor-shape` (`pattern-currentcolor-order`) — the reference
  prints the same fifteen lines, so it agrees on components-then-
  instance after all, contrary to part 2's skip reason; and
  `uncoloured-without-base` (`uncoloured-pattern-without-base`) — the
  reference ends in `rangecheck` as the file declares. Both headers
  are left as adjudicated; the two requirements describe a reference
  this tier does not see. Skipped: `patterns/shading-out-of-range`
  and `upath/encoded-bad-opcode`, as declared. The captured driver
  job with its host prelude: pass, output same, one page each.
- **Gates, final.** `cargo test --workspace` 1047 passed, 0 failed, 3
  ignored; clippy clean on all targets; fmt clean; `difftest run` 262
  of 262 (the 257 of part 2, every pre-existing golden byte-identical,
  and the five part-3 files); `parse-survival` 262 files, 0 failed;
  `fuzz-round` 2 600 programs (1 300 core, 1 300 graphics), 0 failed;
  `lint-strings` 947 files clean; `check-wasm` passes; the
  `--no-default-features` build passes; `openspec validate
  patterns-and-forms` valid; the oracle tier and the captured job as
  recorded above. The external checker over the pattern and form PDFs
  is run by the coordinator after this part.
- **Two slugs withdrawn after the oracle run.** `pattern-currentcolor-order`
  and `uncoloured-pattern-without-base` were registered from part 2's
  reading of the reference, but the final oracle run reported both
  `divergence-closed`: the reference returns the same `currentcolor`
  shape and raises `rangecheck` for an uncoloured pattern without a base.
  They are not divergences; the requirements and the two `% divergence:`
  headers were removed and the files compare as plain passes. Three slugs
  remain: `uncoloured-cell-colour-operators`,
  `capture-refuses-page-operators`, `resource-instance-shape`.

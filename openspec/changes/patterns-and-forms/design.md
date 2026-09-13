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

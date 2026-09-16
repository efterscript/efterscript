# Design: Shading patterns, function dictionaries, and reusable streams

See proposal.md and the spec deltas. This document fixes how functions
and shadings cross the boundary as values, how mesh data is
normalised, how a shading pattern fits the pattern colour model, how
reusable streams reuse the file table, and what the writer emits.

## Context

- Colour crosses the boundary as `SpaceSpec` plus components; a
  pattern crosses as `PatternInfo` (tiling fields) with its cell
  captured at first paint; `IrOp::Image`/`Form` carry a placement
  matrix because the IR has no `cm`.
- The file table has `Kind::Bytes` (a one-shot in-memory stream for
  string sources), decode layers with chaining, and no positioning:
  `fileposition`, `setfileposition`, `resetfile`, `bytesavailable` do
  not exist.
- The writer emits calculator functions for tint transforms and
  nothing else of ISO 32000-1 §7.10; streams with dictionaries exist
  for images, forms, and pattern cells.
- PLRM3 §3.10.1 defines function types 0, 2, 3 (Tables 3.12–3.15);
  §4.9.3 the shading dictionaries (Tables 4.10–4.19) and the mesh data
  formats; §3.13.3 the reusable stream (Table 3.24). ISO 32000-1
  §8.7.4 and §7.10 define the identical PDF constructs.

## Goals / Non-Goals

**Goals:** every shading type preserved as the same PDF shading;
functions carried as values, never evaluated; mesh data in one packed
form; a shading pattern as a colour through the existing pattern
model; reusable streams positionable.

**Non-Goals:** evaluating functions in the interpreter; converting
shading colours between spaces; `AsyncRead` laziness; writing
smoothness; cubic `Order` 3 interpolation semantics (carried as the
entry, the viewer interprets it).

## Decisions

**D1. Functions are boundary values.** `FunctionSpec` in
`ps-vm/src/graphics.rs`: `Sampled { domain, range, size, bits, order,
encode, decode, samples: Vec<u8> }`, `Exponential { domain, range, c0,
c1, n }`, `Stitching { domain, range, functions: Vec<FunctionSpec>,
bounds, encode }`, built and validated by `ops/function.rs` from the
dictionaries (defaults from Tables 3.13–3.15; `Decode` defaults to
`Range`; `Encode` to `[0 Size−1 …]`; the sample source read in full —
a string copied, a file read from position 0 to the byte count `Size`
× outputs × bits / 8 rounded up, `rangecheck` if short). Dimensional
checks: `Domain` even length ≥ 2; type 0 `Size` length = inputs,
`Range` required; type 2 one input, `C0`/`C1` equal length = outputs;
type 3 one input, `Functions` k ≥ 1 with equal output counts,
`Bounds` k−1 ascending within `Domain`, `Encode` 2k. *Alternative:*
capture the dictionary as PostScript source like a tint transform —
PDF functions are structured objects, not code.

**D2. Shadings are boundary values with normalised mesh data.**
`ShadingSpec { kind, space: SpaceSpec, background: Option<Vec<f32>>,
bbox: Option<Bounds>, antialias: bool }`; `ShadingKind::{Function {
domain, matrix, function }, Axial { coords, domain, function, extend },
Radial { coords, domain, function, extend }, Mesh { ty: 4..=7, bits_per_
coordinate, bits_per_component, bits_per_flag, decode, vertices_per_row,
function: Option<FunctionSpec>, data: Vec<u8> } }`. A `DataSource`
array is *re-encoded* into the packed form with 32-bit coordinates,
16-bit components, 8-bit flags, and a `Decode` built from the array's
own minimum and maximum per coordinate and component (so the writer
has one form; recorded) — an array is bounded by the string limit
anyway; string and file sources are copied as given with their
declared depths. The VM walks the data once (`ops/shading.rs`) to
check whole vertices and triangles/patches and edge-flag values per
Tables 4.15–4.19 (types 4 and 6/7 need flag 0 at the first and every
unconnected element; type 5 needs a multiple of `VerticesPerRow` with
≥ 2 rows), raising `rangecheck`. `ColorSpace` is parsed by the existing
space parser (any but `Pattern`; `Indexed` refused with a `Function`
or for types 1–3); a CIE space is collapsed by the `cie` module and
its non-collapsing case raises `limitcheck` (registered). *Alternative:*
carry arrays as numbers — two writer paths for one construct.

**D3. `shfill` is a backend call carrying the CTM.** Trait gains
`fn shade(&mut self, shading: &ShadingSpec) -> Result<(), VmError>`
(default `Ok`); the backend interns the shading in `Resources.shadings`
and emits `IrOp::Shade { shading: ShadingIndex, matrix: ctm }` (taken
through any enclosing capture like an image). Inside a captured cell
or form it lands in that content. The operator ignores the current
path and colour and leaves them alone; `Background` is dropped for
`shfill` in the VM (the spec ignores it there) — the resource keeps
it for pattern use, so the same shading used both ways is interned
once with the background and the writer writes it in both (PDF's `sh`
ignores it too).

**D4. Shading patterns reuse the pattern colour model.** `PatternInfo`
gains `kind: PatternKind { Tiling { bbox, xstep, ystep, paint_type,
tiling_type }, Shading(Box<ShadingSpec>) }`; `makepattern` with type
2 validates `Shading` (D2) and builds the instance with the
concatenated matrix as today; `begin_pattern_cell` answers `false`
for a shading pattern (nothing to capture) and `set_pattern` interns
it as `PatternSpec::Shading { matrix, shading: ShadingIndex }`. The IR
`PatternSpec` becomes an enum `{ Tiling { … existing fields … },
Shading { matrix, shading } }`; every match updated; the dump prints
`pattern n shading k matrix …`. Painting with it is unchanged
(`SetPattern`), so text, strokes, image masks, and user paths get it
for free. *Alternative:* a separate colour kind for shadings — the
PDF treats both as `/Pattern`, so should the IR.

**D5. Reusable streams are positionable `Bytes` entries.** `filter`
with `ReusableStreamDecode` builds the pre-filter chain from
`Filter`/`DecodeParms` over the source (reusing `open_decoder`), reads
it to end-of-data into a `Vec<u8>`, closes the chain (closing the
source only with `CloseSource`), and opens a `Kind::Bytes` entry marked
`positionable` that does not auto-close at EOF. `fileposition`,
`setfileposition` (`rangecheck` past the length), `resetfile`,
`bytesavailable`, `flushfile` operate on it; on other entries
`fileposition`/`setfileposition` raise `ioerror`, `bytesavailable`
answers −1, `resetfile` is a no-op, per the entries in PLRM3 §8.2.
`AsyncRead` is read and ignored (eager; recorded), `Intent` type-
checked. A shading or function `DataSource` that is a file is read
from its current position without requiring positionability (leniency
where the manual demands a positionable file for patterns; recorded).

**D6. Writer.** Functions: type 2 and 3 as dictionaries, type 0 as a
Flate stream with `/Domain /Range /Size /BitsPerSample /Order /Encode
/Decode` (defaults omitted); nested stitching functions as separate
objects referenced from `/Functions`. Shadings: `/Shn` objects — a
dictionary for types 1–3 (`/ShadingType /ColorSpace /Background /BBox
/AntiAlias` plus `/Domain /Matrix /Function` or `/Coords /Domain
/Function /Extend`), a Flate stream for 4–7 with `/BitsPerCoordinate
/BitsPerComponent /BitsPerFlag /Decode /VerticesPerRow /Function`;
the colour space written through the existing space writer (a `/CSn`
reference or a device name). A shade operation: `q a b c d e f cm /Shn
sh Q`. A shading pattern: `/Pn << /PatternType 2 /Shading Shn-ref
/Matrix [...] >>` and the page's `/Pattern` and `/Shading` resource
entries; per-content resource walks include shadings. `pdf-out` needs
nothing new if `write_stream` and the dictionary builders suffice.

**D7. Smoothness.** `setsmoothness`/`currentsmoothness` on the VM-side
graphics state, clamped, default 0.02 (record what the reference
answers and match it), saved/restored, not written.

**D8. Corpus.** `corpus/unit/shadings/`: every type through `shfill`
(type 1 with a sampled function from a string and from a reusable
stream; axial and radial with type 2 and type 3 functions and with a
`Function` array; meshes 4–7 from arrays and from hex strings, a
`Function`-driven mesh, an `Indexed` mesh); `Extend`, `BBox`,
`Background` in both uses; a shading pattern filling text, a stroke,
an image mask, and a user path; a pattern inside a form; the errors;
the CIE limit file with its slug. `corpus/unit/filters/reusable-*.ps`
for the stream and positioning scenarios. IR and PDF goldens; the
external checker; the oracle in colour (`% oracle: colour`) — the
reference and our PDF are rasterised by the same engine so gradients
should agree closely; a systematic difference is registered only after
inspection.

## Risks / Trade-offs

- [Mesh re-encoding changes numeric precision] → 32-bit coordinates
  and 16-bit components are finer than any array input; a round-trip
  unit test decodes the packed data back within 1e-4.
- [Function dictionaries carry arbitrary sizes] → sample sources are
  copied once; a bound on total shading data (recorded as a limit,
  `limitcheck`) keeps a hostile job from exhausting memory.
- [The reference's rasteriser and ours draw a gradient differently] →
  colour comparison with the existing threshold; inspect any
  difference before registering a slug.
- [`PatternSpec` becoming an enum churns the pattern change's code] →
  mechanical, with every existing pattern golden byte-identical.

## Open Questions

- The default smoothness value: match what the reference answers
  after observation; does not change the specs or tasks.

## Implementation notes

Recorded where the code departs from, or pins down, the text above.
Part 1 covered tasks 1.1 and 1.2 and the ps-vm halves of 2.1 and 2.2:
the positionable file entries and the positioning operators, the
`ReusableStreamDecode` filter, the function and shading boundary values
with their readers and unit tests. Provenance: PLRM3 §3.10.1 (Tables
3.12–3.15 and the surrounding text on the sample layout and the
Encode/Decode relationship), §3.13.1–3.13.3 (data sources, end of data,
Table 3.24), §4.9.3 (Tables 4.11–4.19, the vertex and patch data layouts
and the implicit-value tables), §4.10.5 "Sample Decoding", and the §8.2
entries for `bytesavailable`, `closefile`, `fileposition`, `filter`,
`flushfile`, `resetfile`, and `setfileposition`; and black-box
observation of the reference through `difftest oracle` over the
`corpus/unit/filters/reusable-*.ps` files ("observed" below).

- **D5 as built: positionability.** The file table's in-memory entry
  kind (`Kind::Bytes`, until now a boxed stream) carries a
  `positionable` flag; `open_bytes` (a `filter` source over a string)
  leaves it clear and `open_reusable` sets it. Only a reusable stream
  is positionable: a string source under a decode filter stays
  one-shot, since a program never holds that entry as a file, and
  every other kind — the embedder's streams, the job's own source,
  procedure sources, decode and encode layers — has no length to
  position within. `fileposition` and `setfileposition` are `ioerror`
  on those and on a closed file, `setfileposition` `rangecheck` beyond
  the length (a negative position too), `bytesavailable` answers the
  length minus the position on a reusable stream, −1 on any other open
  file, `ioerror` on a closed one, and `flushfile` on a reusable
  stream moves to the end. The position excludes the byte the scanner
  peeked, so `token` followed by `fileposition` reports what the
  scanner consumed, the whitespace that ended the token included. A reusable stream stays open at its end under every
  reader — `readstring`, `readline`, `token`, `exec`, `image`, a later
  `filter` over it — because the automatic close at end of file applies
  to decode layers only (`is_filter`), which a `Bytes` entry never is.
- **`resetfile` on ordinary files.** The entry's rule for an input
  file is to discard characters received but not consumed, never with
  an error. On a reusable stream that is a move to position 0. On every
  other file it is a no-op: the only unconsumed bytes the table holds
  for such a file are the scanner's lookahead, which belongs to the
  token stream (dropping it would eat the delimiter after the
  operator), and the job's buffered chunks are not the program's to
  drop short of `closefile`. On a closed file it does nothing and
  raises nothing, as the entry says.
- **The eager read runs as a loop frame.** `filter` with the name
  checks the parameters, opens the source and the pre-filter chain
  (`open_decoder`, each layer with `close_source` false), pops its
  operands, and pushes `LoopFrame::ReusableRead`; each step reads one
  chunk from the top of the chain into the frame's buffer, and the
  last step closes the chain, closes the original file source only
  with `CloseSource` true, opens the positionable entry, and pushes the
  file object. A frame rather than a read inside the operator because
  of suspension: a read that starves fails the step with `NeedMore`,
  the loop rolls the file table back (undoing the partial chunk and
  restoring the layers' decoder state) and either arranges the
  procedure source's next delivery (`FilterData`, as any filter over a
  procedure) or suspends the run for more job bytes; the frame and the
  entries it opened persist across the wait, which an operator that
  re-runs from scratch could not offer a procedure source (its entry
  would be reopened empty). The frame's entries are closed when the
  frame is popped, so an error inside the procedure leaves nothing
  open. The chunked-feed test splits a job making a reusable stream
  from `currentfile` at every byte. The read is bounded at 256 MiB
  (`limitcheck`). Data follows the operator directly: the read starts
  at the byte after `filter`'s delimiter, so `currentfile … filter`
  must be the last token on its line, as with `exec` over a filter.
- **The pre-filter chain.** `Filter` is a name or an array of names,
  each a decode filter of this interpreter's (an unknown name is
  `undefined`; an encode filter, or the reusable filter itself, is
  `rangecheck`); `DecodeParms` is the one dictionary for a single
  filter or an array as long as `Filter` with `null` where a filter
  takes nothing (a length mismatch `rangecheck`, another type
  `typecheck`); each filter's parameters are read by the ordinary
  `Params::read` with the same checks as `filter` itself, and any
  `CloseSource` inside them is not used. When the top of the chain
  ends before a lower layer has consumed its own marker (run-length
  data ending in its `80` inside a base-85 layer), the lower layers
  are read through their markers as `closefile` on a chain does, so
  the source continues after the encoded section. `AsyncRead` is
  type-checked and ignored (the read is always eager; recorded),
  `Intent` type-checked only (any integer). The `Filter` category
  lists the name (fourteen entries, seven decoders).
- **D1 as built.** `FunctionSpec` as written, with two refinements:
  `range` is a `Vec<f32>` that is empty when a type 2 or 3 dictionary
  gave none (a type 0 dictionary always has one — absent it is
  `undefined`), and `size` is `Vec<u32>`. `inputs()` is the `Size`
  count or 1, `outputs()` the range's pair count, `C0`'s length, or
  the first part's outputs. A shading's `Function` is carried as
  `Vec<FunctionSpec>`: one entry for a single n-output function, n
  entries of one-output functions for an array (PDF takes the same two
  shapes), empty only for a mesh without a function — D2's `function`
  and `Option<FunctionSpec>` became this one vector. Readers:
  `ops::function::read_function(i, object, inputs, outputs)` and
  `read_function_or_array`, both public so the boundary tests (and
  part 2's `shfill`) call them with an object a program left on the
  operand stack. Checks beyond the delta's list: `Size` entries
  positive integers, `BitsPerSample` in the table's set, `Order` 1 or
  3, `Encode`/`Decode`/`C0`/`C1`/`Bounds`/`Encode` of the lengths the
  tables give (`rangecheck`), stitching `Bounds` strictly increasing
  and strictly inside the domain (the manual's inequality chain, taken
  literally), a type 2 or 3 dictionary with more than one input
  `rangecheck`, nesting beyond eight levels `limitcheck`, sample data
  beyond 64 MiB `limitcheck`. A type 0 source that is a positionable
  file is read from position 0 (the manual's model); any other file
  from its current position (leniency for a filter over the job,
  recorded); a string is copied for the byte count only. *Leniencies:*
  the type 2 constraints tying `Domain` to a non-integer or negative
  `N`, and the rule that a shading function's domain contain the
  shading's, are not enforced — the viewer clips inputs to the
  domain either way.
- **D2 as built.** `ShadingSpec` and `ShadingKind` as written, with
  the vector-valued `function` above, `bits_per_flag` 0 for type 5,
  `vertices_per_row: Option<u32>` present for type 5 only, fixed-size
  arrays for `domain`, `coords`, and `extend`. `ops::shading::
  read_shading(i, dict)` is public for the same reason as the function
  reader. `ColorSpace` goes through `parse_space`, so a CIE-based
  array is parsed and collapsed by the `cie` module as everywhere
  else; a result carried as `Lab` — which only a non-collapsing space
  produces — is `limitcheck`, as the space itself or as an Indexed
  base; a pattern space `rangecheck`; `Indexed` `rangecheck` for
  types 1–3 or with a `Function`. `Background` is an array of the
  space's arity, `BBox` four numbers enclosing an area (the shared
  reader: a degenerate box `rangecheck`), `AntiAlias` a boolean. Type
  1 `Domain` must have x0 ≤ x1 and y0 ≤ y1; type 3 radii ≥ 0; `Extend`
  exactly two booleans. For a string or file source the bit depths are
  required and checked against the tables' sets (`BitsPerFlag` only
  for 4, 6, 7), `Decode` has 4 + 2 × values entries (values = 1 with a
  function); for an array source none of those entries is read.
- **Re-encoding depths and the constant-column rule.** An array source
  is packed at 32 bits per coordinate, 16 per colour value, 8 per flag,
  each vertex or patch byte-aligned (which at those depths it already
  is); `Decode` is each column's minimum and maximum over the whole
  mesh; a column with no values (an empty type 4 mesh) decodes from
  `[0 1]`; a *constant* column decodes from `[v v+1]` so that its
  all-zero codes give `v` exactly (the "min = max − 1" option of the
  design, turned around so the exact end is the one every code hits);
  a parametric value under a `Function` is clipped to `[0 1]` as the
  tables say for arrays and decodes from `[0 1]`. The round-trip test
  decodes the packed data back within 1e-4.
- **The structural walk.** One grammar over a source of flags and
  numbers — the packed bits with their decode ranges, or the array's
  objects — yields vertices and patches, and `read_shading` walks
  every mesh once with a discarding sink; `mesh_elements` walks a
  stored mesh again to hand the values back. Type 4: a flag of 0
  starts a triangle that two more vertices complete, their own flags
  ignored whatever they are; 1 or 2 continues the previous triangle;
  3, or 1/2 before any triangle, or data ending inside a triangle, is
  `rangecheck`; an empty mesh is accepted as zero triangles. Type 5:
  no flags; the vertex count a multiple of `VerticesPerRow` (≥ 2, an
  integer) and at least two rows. Types 6/7: the first patch's flag 0;
  a flag of 0 takes 12/16 points and 4 corners, 1–3 take 8/12 and 2;
  at least one patch. In every type a partial vertex or patch — the
  packed data ending mid-element, trailing bytes short of an element
  included — is `rangecheck`; packed flags use their low two bits as
  the tables say, array flags must be integers 0 to 3 (`typecheck`
  for a real, `rangecheck` beyond). Mesh data is bounded at 64 MiB.
- **Sources.** A mesh `DataSource` file is read to its end, from
  position 0 when positionable and from its current position
  otherwise (the manual requires a reusable form only for patterns;
  `shfill` may take a one-shot source — the leniency the design
  records); a string is copied whole. A file that is the job's source
  starving mid-read fails the calling operator with `NeedMore`, which
  is part 2's `shfill` to carry (the readers keep no state).
- **Oracle.** The four reusable-stream corpus files agree with the
  reference on output; `reusable-ordinary-file.ps` is skipped because
  the reference reads the job from a real file, where `currentfile`
  can be positioned and the scenario's `ioerror` cannot arise.
- **Part 2** covered tasks 2.3, 2.4, and 3.1 and the corpus halves of
  2.1 and 2.2: the `shade` hook and `shfill`, smoothness, type 2
  patterns, the IR resources and operation with their dump, and the
  interim writer behaviour. Provenance: PLRM3 §4.9.3 (Table 4.10, the
  notes on `Background`, on `BBox`, and on the space a shading is
  painted in for `shfill` against a pattern), the §8.2 entries for
  `shfill`, `setsmoothness`, `currentsmoothness`, `makepattern`, and
  `setpattern`; ISO 32000-1 §8.7.2 (the pattern matrix), §8.7.4.1–
  8.7.4.2 (Tables 76 and 77: the type 2 pattern dictionary, `sh` in
  current user space with `Background` ignored); and black-box
  observation of the reference through `difftest oracle` (the
  smoothness default and clamping, the strictness of the data checks).
- **D3 as built: the `shade` hook.** `GraphicsBackend::shade(&ShadingSpec)`
  with a default of `Ok(())`; the graphics crate synchronises the
  clip, interns the shading in `Resources.shadings` by structural
  equality (`intern_shading`, as spaces are), and records `IrOp::Shade
  { shading, matrix: ctm }`; under the null device nothing. The
  operation carries no colour and flushes none: a colour set before a
  `shfill` reaches the IR only when a later paint needs it. Inside a
  capture the matrix is taken through the capture's inverse CTM like
  an image's (`transformed`), so a shade inside a form body or a cell
  is in that content's space. `shfill` (a graphics-group operator,
  `typecheck` for a non-dictionary operand) calls `read_shading` on the
  operand in place, hands the value over, and pops afterwards, so a
  reader error leaves the operand on the stack; it is `undefined`
  inside an uncoloured cell, as `image` is, since it paints colours of
  its own. The current path and colour are not touched (verified at
  the boundary and in the graphics crate). **`NeedMore`:** the readers
  keep no state, so a mesh or sample source that is the job's file
  starving mid-read fails the operator with `NeedMore`; the loop rolls
  the file table back (the decode layer over the job included) and
  re-dispatches the operator with its operand still on the stack, the
  same path `image` takes — nothing was added for it. The chunked test
  reads a hexadecimal-filtered mesh from `currentfile` at every split,
  through `shfill` and through `makepattern`. **`Background`:** the
  value passed to `shade` carries the background as read, and the
  resource keeps it; the operation is what ignores it (its
  documentation says so), so a shading used both ways is one resource
  (`background-both-uses.ps`: one `shading`, one `sh`, one `pattern`).
- **D7 as built: smoothness.** A `smoothness` field of the graphics
  crate's `GState`, saved and restored with it, reset by
  `initgraphics` (`reinitialized` takes the default) — the reference
  resets it too (observed). `setsmoothness` clamps to the unit
  interval in the operator (`NaN` to 0) and the backend clamps again
  (`rangecheck` for `NaN`, as `set_flatness`); `currentsmoothness`
  answers the backend. Trait methods `set_smoothness`/`smoothness`
  with defaults (accept; answer the default). The default is
  `DEFAULT_SMOOTHNESS = 0.02`, what the reference answers in a fresh
  job (observed: `0.02`, `0.05` after setting it, `1.0` and `0.0` after
  2 and −1, the saved value after `grestore`, `0.02` after
  `initgraphics`). Nothing is written.
- **D4 as built: `PatternKind`.** `PatternInfo { id, matrix, kind }`
  with `PatternKind::{Tiling { bbox, xstep, ystep, paint_type,
  tiling_type }, Shading(Rc<ShadingSpec>)}` and the helpers
  `is_uncoloured` and `is_shading`; `PatternInfo` is `Clone`, no longer
  `Copy`, and the instance table's entries with it. *Deviation:* `Rc`
  rather than the design's `Box`, so the instance table, the graphics
  state, and every saved copy of it share one mesh (a `gsave` would
  otherwise copy up to 64 MiB); the library crates use no threads, and
  the IR already shares font programs the same way. `makepattern`
  reads `PatternType` as 1 or 2 (`rangecheck` otherwise, so 3 is), a
  type 2 dictionary's `Shading` entry must be a dictionary
  (`typecheck`; absent `undefined`) and is read whole by
  `read_shading` at `makepattern` — where a file source starving fails
  the operator with `NeedMore` as for `shfill` — and the instance is
  the read-only copy with `Implementation` as for type 1, its matrix
  the operand concatenated with the CTM. The `Pattern` category's
  shape check accepts a type 2 dictionary whose `Shading` is a
  dictionary without reading the shading (a file data source would be
  consumed by a check); `pattern-category-shape.ps` now puts
  `PatternType` 3 out of range. `setpattern` and `setcolor` take a
  shading instance with no components (`is_uncoloured` is false);
  `currentcolor` answers the instance. `capture_cell` answers `false`
  before looking for a `PaintProc` — the backend is never asked, which
  the boundary test checks by the absence of `BeginPatternCell` — and
  the graphics crate's `begin_pattern_cell` answers `false` for a
  shading kind on its own. Every painting operator routes through the
  unchanged pattern model; the graphics crate's `pattern_resource`
  makes `PatternSpec::Shading { matrix, shading }` per context with the
  interned shading, the matrix taken through the enclosing capture as
  a tiling pattern's is.
- **3.1 as built: the IR.** `Resources.shadings: Vec<ShadingSpec>`
  (the space inline, as the VM read it; part 3 interns it in
  `color_spaces` or writes it inline as it prefers), `ShadingIndex`,
  `IrOp::Shade { shading, matrix }`, and `PatternSpec` as the enum
  `{ Tiling { matrix, bbox, xstep, ystep, paint_type, tiling_type,
  ops }, Shading { matrix, shading } }` with `matrix()` and `ops()`
  accessors (a shading pattern's operations are empty) that the
  resource walks in `remelt` (`fonts.rs`, `downsample.rs`,
  `embedded.rs`) use. Every match in the graphics crate, `remelt`, and
  the tests was updated; `difftest` matched nothing.
- **The dump layout.** Shading resources follow the `cs` lines (pages
  without shadings dump byte-identically: every existing IR golden is
  unchanged): `shading <n> type <t> space <space> [background [c…]]
  [bbox [llx lly urx ury]] [antialias] <entries> {`, the entries
  `domain [x0 x1 y0 y1] matrix <six>` (type 1), `coords [4 or 6]
  domain [t0 t1] extend [bool bool]` (2, 3), `bits <coord> <component>
  <flag> decode [ranges]` (4, 6, 7), `bits <coord> <component> per-row
  <n> decode [ranges]` (5); inside the block one line per function
  (`function type 0 domain […] range […] size […] bits b order o
  encode […] decode […] samples <hex>`, `function type 2 domain […]
  [range […]] c0 […] c1 […] n <n>`, `function type 3 domain […]
  [range […]] bounds […] encode […] {` with its parts indented two
  more and `}`), then for a mesh `data <hex>`, then `}`. Array-valued
  entries are bracketed so an empty `bounds` or `range` cannot be
  mistaken for the next entry's numbers; an absent `Range` is omitted;
  hexadecimal is one lowercase run in angle brackets, as a text run's
  codes are — a large mesh is one long line, which a golden never
  holds. The operation is `sh <n> <matrix>` (PDF's operator name, as
  `form` and `Do img` follow the convention) and the pattern line
  `pattern <n> shading <k> matrix <six>`; the module documentation
  lists all of it.
- **The interim writer.** `remelt` compiles with the enum and writes
  no shading yet: `IrOp::Shade` writes nothing and adds the note
  `shade operation over shading <k> skipped: shadings are not yet
  written`; a `SetPattern` naming a `PatternSpec::Shading` writes `0
  g`/`0 G` with the note `shading pattern P<n> painted as black:
  shadings are not yet written` (the note mechanism the tiling change
  used for its own interim); no object is allocated for a shading
  pattern and the page's `/Pattern` dictionary does not list it
  (`Objects.patterns` is `Vec<Option<Ref>>`), so the external checker
  sees a consistent file. The PDF goldens under
  `corpus/golden/pdf/shadings/` are this writer's, and part 3
  regenerates them.
- **Corpus and oracle.** `corpus/unit/shadings/`: `axial-shfill`,
  `triangle-mesh-array`, `pattern-text-and-stroke`,
  `background-both-uses`, `pattern-fill-matrix` (the graphics-ir
  pattern scenario, made under a scaled and translated CTM) with IR and
  interim PDF goldens; `smoothness-round-trip`; the error files
  `incomplete-triangle`, `indexed-with-function`,
  `function-short-source`, `function-bounds-order`; under `patterns/`
  the new `shading-instance-made` and the rewritten
  `shading-out-of-range` (`PatternType` 3, no longer skipped by the
  oracle). Oracle verdicts: the five page files are skipped with `%
  oracle: skip the PDF writer does not yet emit shadings` after
  confirming that their PDFs carry only the clip or the black fill
  (they failed on pixels before the header); `function-bounds-order`,
  `indexed-with-function`, `smoothness-round-trip`,
  `shading-instance-made`, and `shading-out-of-range` pass with the
  same output as the reference. **Two divergences registered:** the
  reference paints a sampled function whose source is short
  (`sampled-function-short-source`) and drops an incomplete mesh
  element (`incomplete-mesh-element`), where the delta's SHALL and D1/
  D2 raise `rangecheck`; both files carry the slug and the
  expected-divergences delta gains the two requirements — the strict
  reading stands because the data goes to the PDF as checked, but a
  reviewer preferring the reference's leniency can turn either into a
  tolerant read in `ops/function.rs` or `ops/shading.rs`.
- **Part 3** covered tasks 4.1, 5.1, and 5.2: the writer, the rest of
  the corpus, and the gates. Provenance: ISO 32000-1 §7.10 (Tables
  38–41 and the text of §7.10.2 on the sample stream: bit-packed,
  high-order bit first, no padding, the first input dimension varying
  fastest, the outputs of one sample in `Range` order), §8.7.4 (Tables
  76–86: the type 2 pattern dictionary, `sh`, the common and per-type
  shading entries, and the vertex and patch data layouts of
  §8.7.4.5 with each vertex or patch padded to a byte), §7.8.3 (Table
  33: the `Shading` and `Pattern` sub-dictionaries), and §8.7.2 (the
  pattern matrix against the default space of the page or of the form
  the pattern is used in); and black-box observation of the reference
  through `difftest oracle` in colour over the shading, pattern, and
  filter directories ("observed" below).
- **D6 as built: function objects.** `write_function_spec` in
  `remelt/src/resources.rs` writes one object per `FunctionSpec`. A
  type 0 is a Flate stream of the sample bytes as the IR holds them,
  with `/FunctionType 0 /Domain /Range /Size /BitsPerSample` always and
  `/Order` (when 3), `/Encode` (when not `[0 Size−1 …]`), `/Decode`
  (when not equal to `Range`) only when they differ from Table 39's
  defaults. **Sample layout verified:** part 1 packed the samples as
  PLRM3 §3.10.1 lays them out — most significant bit first, no padding
  between samples, the first input dimension fastest, the outputs of
  one sample adjacent — and §7.10.2 of ISO 32000-1 prescribes the same
  layout in the same words' terms, so the bytes go out verbatim; the
  sink test writes a two-by-two RGB table and a four-bit table and
  reads both streams back byte for byte. A type 2 is a dictionary with
  `/Domain` and `/N` always, `/Range` when the dictionary gave one,
  `/C0` unless it is `[0]`, `/C1` unless it is `[1]` (Table 40's
  defaults). A type 3 writes its parts first as objects of their own
  and refers to them from `/Functions`; `/Bounds` and `/Encode` are
  always written (an empty `Bounds` for one part). A shading's
  `Vec<FunctionSpec>` of length one becomes `/Function ref`, of length
  n `/Function [refs]`, empty (a mesh without) no entry
  (`put_function`). Function objects are not resources: nothing in a
  content stream names them, so they are anonymous, allocated and
  written just before the shading that uses them.
- **Shading objects.** `write_shading` writes `/Shn`, n the
  `ShadingIndex`, named in the `Shading` resource dictionary of the
  page and of any content that paints it. Types 1–3 are dictionaries:
  `/ShadingType /ColorSpace` always, `/Background` when the value
  carries one, `/BBox` when set, `/AntiAlias true` only when true;
  then type 1 `/Domain` unless `[0 1 0 1]`, `/Matrix` unless the
  identity, `/Function`; types 2 and 3 `/Coords`, `/Domain` unless
  `[0 1]`, `/Function`, `/Extend` unless `[false false]`. Types 4–7
  are Flate streams of the packed data with `/BitsPerCoordinate
  /BitsPerComponent`, `/BitsPerFlag` for every type but 5,
  `/VerticesPerRow` for type 5, `/Decode`, and `/Function` when there
  is one. **Mesh layout verified:** §8.7.4.5 reads each vertex as flag,
  x, y, then the colour values at their depths from the high-order
  bits down, each vertex (types 4 and 5; type 5 without the flag) and
  each patch (6 and 7: flag, the 12/16 or 8/12 points, the 4 or 2
  corner colours) padded to a whole byte — the layout part 1's
  `Bits` reader consumes and its `BitWriter` produces (`end_element`
  and `align` both round to the byte), so the data goes out verbatim
  too; the sink test writes an eight-bit triangle and reads the stream
  back through the interpreter's own `mesh_elements`, which yields the
  vertices it was packed from. Numbers go through the writer's
  canonical real form (`fmt_real`).
- **The colour-space decision.** A shading's `ColorSpec` is written
  *inline* in the shading dictionary through the existing `write_space`
  and `Form::put`: a device family as its name, anything else as the
  array form (`[/Indexed /DeviceRGB 3 <…>]`, `[/CalRGB << … >>]`,
  `[/Separation /Spot /DeviceCMYK ref]` with its calculator function
  written as a stream first). Table 78 takes a name or an array there,
  exactly as an image dictionary does, and the alternative — interning
  the space into `Resources.color_spaces` at write time — would have
  changed `/CSn` numbering behind the IR's back and named a resource
  that no content stream uses. The cost is that a Separation or
  DeviceN space used both for painting and in a shading has its tint
  function written twice; no corpus file does, and the objects are
  small.
- **Content.** A shade operation is `q a b c d e f cm /Shn sh Q` on
  one line, as an image or form placement is written; under the
  identity it is the bare `/Shn sh` — `sh` neither reads nor changes
  the graphics state (Table 77), so a save around it would restore
  nothing, and the stroke writer already drops its wrapper for an
  identity CTM. A shading pattern is `/Pn << /Type /Pattern
  /PatternType 2 /Shading Shn-ref /Matrix [six] >>` (Table 76; the
  matrix always written, as a tiling pattern's is), selected exactly
  as a tiling pattern: `/Pattern cs /Pn scn` and `SCN` for strokes,
  the pattern space's `/CSn` when it has a base. The two interim notes
  are gone, `Objects.patterns` is `Vec<Ref>` again (every pattern has
  an object), and the content writer no longer looks at the pattern's
  kind. Shading objects are written after the images and before the
  fonts, so a glyph procedure can paint one; a shading pattern's
  object is written with the tiling streams after the fonts and
  refers to the shading's object directly.
- **Resource walks.** `Refs` gains `shadings`; `collect` records a
  `Shade` operation's shading, and — in the deep walk only — the
  shading behind a shading pattern, so a Type 3 font's identity `Key`
  (which gains `shadings: Vec<ShadingSpec>`) tells two pages apart
  whose glyphs paint with the same pattern index over different
  shadings. The direct walk does not list a shading pattern's shading
  in the content's own dictionary: the pattern object refers to it,
  and the content names only the pattern. `resources_dict` writes
  `/Shading` beside `/Pattern` (Table 33), page-wide or restricted to
  the content's `Refs`; `names_anything` counts shadings. The
  downsampling and embedding walks match nothing new (`Shade` paints
  no image and shows no text) and compile unchanged.
- **Corpus (D8 as built).** Fifteen files added under
  `corpus/unit/shadings/`, every painting one with `% oracle: colour`
  and IR and PDF goldens: `function-sampled-string` (type 1, a 2-in
  3-out table from a string, `Matrix` onto a rectangle),
  `function-sampled-reusable` (the same table from `currentfile …
  /ReusableStreamDecode filter` through a hexadecimal pre-filter, the
  stream positioned to 3 before use to show the read starts at 0),
  `radial-stitching` (type 3 with a stitching function of two
  exponential parts, the second reversed by its `Encode`, `Extend
  [false true]`), `axial-function-array` (three one-output functions
  as an array), `lattice-hex-string` (type 5 from a hex string, eight
  bits everywhere, `VerticesPerRow 3`), `coons-patches-array` (type 6,
  two patches, the second attached with flag 2), `tensor-patches-array`
  (type 7, the second attached with flag 1, four interior points each),
  `mesh-with-function` (type 4, one parametric value per vertex),
  `mesh-indexed` (type 4 in an Indexed space, a fourth vertex with
  flag 1), `extend-variants` (`[false false]`, `[true true]`, `[false
  true]` in three clips), `bbox-clips` (a `BBox` confining an extended
  axial blend under `shfill` and as a pattern), `pattern-mask-and-upath`
  (a shading pattern painting an `imagemask` and a `ufill`),
  `pattern-inside-form` (the pattern made inside the form's procedure
  and again on the page under a translation: one shading, two pattern
  resources with different matrices), `calrgb-shading` (a
  `CIEBasedABC` with only a white point, written inline as
  `[/CalRGB …]`), and `cie-limit` (`% divergence:
  shading-colour-conversion-limit`, `% expect-error: limitcheck`: a
  `RangeABC` beyond the unit interval keeps the space from
  collapsing). Part 2's five painting files lost their `% oracle:
  skip` line for `% oracle: colour`; the registry self-test in
  `tools/difftest/src/oracle.rs` lists the new slug and no longer lists
  the five as skipped. **Regenerated goldens** (the only goldens
  changed): `corpus/golden/pdf/shadings/axial-shfill.pdf`,
  `background-both-uses.pdf`, `pattern-fill-matrix.pdf`,
  `pattern-text-and-stroke.pdf`, `triangle-mesh-array.pdf`. Every IR
  golden, including part 2's five, is byte-identical.
- **Oracle (colour, `--profile default`, over `corpus/unit/shadings`,
  `patterns`, and `filters`).** 75 files: 67 pass, 0 fail, 6
  expected-divergence, 0 divergence-closed, 2 skipped; output the same
  on 72, differing on 1. Every painting file under `shadings/` passes
  with a differing-pixel fraction of 0.0000 — `axial-function-array`,
  `axial-shfill`, `background-both-uses`, `bbox-clips`,
  `coons-patches-array`, `extend-variants`, `function-sampled-reusable`,
  `function-sampled-string`, `lattice-hex-string`, `mesh-indexed`,
  `mesh-with-function`, `pattern-fill-matrix`, `pattern-inside-form`,
  `pattern-mask-and-upath`, `radial-stitching`, `tensor-patches-array`,
  `triangle-mesh-array` — except `calrgb-shading` at 0.0017 and
  `pattern-text-and-stroke` at 0.0009 (the calibrated conversion and
  the glyph edges; both under the profile's 0.005 limit). The mesh
  types agree exactly, so no interpolation divergence was registered.
  The three error files with slugs are `expected-divergence` as before
  (`cie-limit` newly: the reference paints the non-collapsing space);
  `function-bounds-order`, `indexed-with-function`, and
  `smoothness-round-trip` pass on output. Under `patterns/` the 17
  files pass or keep their part 2 verdicts (`page-operator-in-cell`
  and `uncoloured-colour-operator` expected-divergence on the error,
  `pattern-category-shape` expected-divergence with differing output
  under `resource-instance-shape`, as part 2 left it). Under
  `filters/` 24 pass and the two declared skips remain
  (`flate-encode`, `reusable-ordinary-file`). Part 2's five files
  passed the oracle on their first run without a skip, so no
  expectation needed revisiting.
- **The captured driver job.** `finder-sys608-lw70-print-directory.ps`
  under the host prelude: pass, output the same, one page.
- **Gate record (final, replacing the per-part ones).** `cargo test
  --workspace`: 65 test binaries, 1138 passed, 0 failed. `cargo clippy
  --workspace --all-targets`: 0 warnings. `cargo fmt --check`: clean.
  `difftest run`: 313 files, 313 passed, 0 failed, 0 skipped; `git
  status --short corpus/golden` shows only added files plus the five
  regenerated PDFs listed above. `parse-survival`: 313 files, 0
  failed, no errors. `fuzz-round`: core 1300 programs, graphics 1300
  programs, 0 failed. `lint-strings`: 1061 files clean of 16 listed
  strings. `check-wasm`: `platen` and its dependencies compile for the
  wasm target. `cargo build --workspace --no-default-features`:
  finished. `openspec validate shading-patterns`: valid. The
  external checker over the PDFs is the coordinator's step after this
  part.

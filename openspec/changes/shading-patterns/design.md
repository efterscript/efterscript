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

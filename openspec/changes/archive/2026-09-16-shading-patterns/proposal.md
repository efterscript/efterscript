# Change: Shading patterns, function dictionaries, and reusable streams

## Why

Gradient fills are the last painting construct of the language the
interpreter refuses: `makepattern` raises `rangecheck` for a type 2
pattern and `shfill` is undefined, so any job with a smooth colour
transition — every drawing application's gradient, the shaded logos
and buttons of driver output from the LanguageLevel 3 era — stops at
its first use. Shadings are the one construct whose appearance a
distillation engine must never flatten: PDF carries the same seven shading
types with the same function model, so a job's gradient survives as a
gradient, resolution-independent, rather than as bands of fills. Three
pieces have to land together, and they shape each other: function
dictionaries (the language's only static function representation,
used nowhere but here), reusable streams (the positionable data
source the language requires for a pattern's mesh or sample data),
and the shading dictionaries themselves. The boundary carries paths
and small values only; a shading is the first resource whose *paint*
is a function, so how functions and mesh data cross the boundary and
reach the writer is the decision that is hard to retrofit, and it
belongs with the resource-carrying colour model the tiling-pattern and
calibrated-colour changes just completed.

## What Changes

- **Function dictionaries** (`ps-vm`): type 0 (sampled: `DataSource`
  string or positionable file, `Size`, `BitsPerSample`, `Order`,
  `Encode`, `Decode`), type 2 (exponential: `C0`, `C1`, `N`), and type
  3 (stitching: `Functions`, `Bounds`, `Encode`) dictionaries of PLRM3
  §3.10.1 validated with their `Domain`/`Range` rules and carried as a
  value across the boundary (samples copied out of their source); no
  evaluation in the interpreter — functions are for shadings, and the
  viewer evaluates them.
- **Reusable streams** (`ps-vm`): the `ReusableStreamDecode` filter of
  PLRM3 §3.13.3 (`Filter`/`DecodeParms` pre-filters, `CloseSource`,
  `AsyncRead` and `Intent` accepted), reading its whole source at
  creation into a positionable in-memory file, with `fileposition`,
  `setfileposition`, `resetfile`, `bytesavailable`, and `flushfile`
  behaving as the section says; the `Filter` category gains the name.
- **Shading dictionaries** (`ps-vm`): types 1–7 of PLRM3 §4.9.3
  validated (common entries `ColorSpace`, `Background`, `BBox`,
  `AntiAlias`; the per-type entries; mesh data from an array, string,
  or file decoded per `BitsPerCoordinate`/`BitsPerComponent`/
  `BitsPerFlag`/`Decode`, with the edge-flag and lattice rules checked
  for whole triangles and patches), with a colour space that is a
  device space, a Separation/DeviceN/Indexed space (Indexed only for
  the mesh types without `Function`), or a CIE-based space that
  collapses to a calibrated one.
- **`shfill`** paints a shading in current user space subject to the
  clip; **type 2 patterns** (`makepattern` with `PatternType` 2 and a
  `Shading` entry) become pattern instances usable as the current
  colour for every painting operator, the shading in pattern space,
  `Background` honoured only in pattern use.
- **IR and writer** (`ps-graphics`, `remelt`, `pdf-out`): shadings and
  functions as page resources, a shading-pattern variant beside the
  tiling one, a shade operation carrying its matrix; the writer emits
  function objects (types 0, 2, 3, with sample streams), shading
  dictionaries and mesh streams (`/ShadingType` 1–7 with their
  entries), `/PatternType 2` pattern objects, and `sh` under the
  matrix.
- **Smoothness**: `setsmoothness`/`currentsmoothness` recorded in the
  graphics state.
- Out of scope, with triggers: shadings in a CIE-based space that does
  not collapse (function outputs cannot be converted without
  evaluating them — when a job needs it, a sampled re-encoding of the
  function); `AsyncRead true` laziness (read eagerly, recorded);
  rasterising a shading (never, by project decision); `Function` evaluation in
  the interpreter; a `Halftone`/`Trapping` category the reusable-stream
  intents mention.

## Capabilities

### New Capabilities
- `shadings`: function dictionaries, shading dictionaries and their
  data, `shfill`, shading patterns, smoothness, and the mapping to the
  PDF.
- `reusable-streams`: the `ReusableStreamDecode` filter and the file
  positioning operators.

### Modified Capabilities
- `patterns`: MODIFIED requirement "Pattern instances" — `PatternType` 2
  accepted with a `Shading` entry (the shading-out-of-range scenario
  replaced).
- `graphics-ir`: ADDED requirement for shading and function resources,
  the shading-pattern variant, and the shade operation with dump.
- `remelt`: ADDED requirement for function objects, shading
  dictionaries and streams, shading patterns, and `sh`.

## Impact

- Code: `crates/ps-vm` (`ops/function.rs`, `ops/shading.rs`, the
  reusable stream in `files.rs`/`decoders.rs`, positioning operators
  in `ops/file.rs`, `makepattern` for type 2, `shfill`, smoothness),
  `crates/ps-graphics` (`ShadingSpec`, `FunctionSpec`, the pattern
  enum, `IrOp::Shade`, dump), `crates/remelt` (function and shading
  objects, `sh`, pattern objects), `pdf-out` (a stream-with-dictionary
  helper if missing), corpus under `corpus/unit/shadings/` and
  `corpus/unit/filters/` (reusable streams) with goldens, the `Filter`
  category table.
- No new dependencies.
- Depends on `patterns-and-forms`, `cie-color-spaces`, `filters`,
  archived.

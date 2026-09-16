# Tasks: shading-patterns

## 1. Reusable streams and positioning (ps-vm)

- [x] 1.1 Positionable `Bytes` entries and `fileposition`, `setfileposition`, `resetfile`, `bytesavailable`, `flushfile` semantics per the entries; verified by the positioning scenarios as corpus files and unit tests for the errors on ordinary files
- [x] 1.2 `ReusableStreamDecode` with `Filter`/`DecodeParms` pre-filters, `CloseSource`, `AsyncRead`/`Intent` accepted; the `Filter` category name; verified by the read-twice and pre-filter scenarios as corpus files, a chunked-feed test, and the category test

## 2. Functions and shadings (ps-vm)

- [x] 2.1 `FunctionSpec` boundary value and `ops/function.rs` validation of types 0, 2, 3 with sample sources read in full; verified by unit tests for every default and error and the two function scenarios as corpus files — *part 1 delivered the value, the reader, and the unit tests (`tests/functions.rs`); the two corpus files came with `shfill` in part 2*
- [x] 2.2 `ShadingSpec` boundary value and `ops/shading.rs`: common entries, per-type entries, mesh data decoded from arrays (re-encoded) and strings/files, the structural walk with edge-flag rules, colour-space rules including the CIE collapse and the registered limit; verified by unit tests (a hand-packed mesh decodes, re-encoding round-trips within 1e-4, each error) and the accepted/incomplete/Indexed scenarios as corpus files — *part 1 delivered the value, the reader, and the unit tests (`tests/shadings.rs`); the accepted/incomplete/Indexed corpus files came with `shfill` in part 2*
- [x] 2.3 `shfill` through the new `shade` hook with the CTM; `setsmoothness`/`currentsmoothness`; verified by the axial and mesh scenarios with IR goldens and the smoothness scenario
- [x] 2.4 `makepattern` type 2 with `PatternKind::Shading`, `begin_pattern_cell` answering false for it, `Background` kept for pattern use; verified by the modified patterns scenarios and the text-and-stroke and background scenarios with IR goldens

## 3. IR and dump (ps-graphics)

- [x] 3.1 `Resources.shadings`, `IrOp::Shade`, `PatternSpec` as an enum with the shading variant, matrices through captures, dump lines for shadings, shade operations, and shading patterns; verified by the two graphics-ir scenarios as IR goldens and every existing golden byte-identical

## 4. Writer (remelt)

- [x] 4.1 Function objects (types 0, 2, 3), shading dictionaries and mesh streams, `/Shading` resources, `sh` under the matrix, `/PatternType 2` objects, per-content resource walks; verified by the two remelt scenarios as PDF goldens, a sink test reading the objects back, and every existing PDF golden byte-identical

## 5. Corpus and verification

- [x] 5.1 The remaining D8 corpus (every type via `shfill`, function arrays, `Extend`, `BBox`, pattern uses, pattern inside a form, errors, the CIE-limit file with its slug) with goldens; verified by `difftest run`
- [x] 5.2 `cargo test --workspace`, clippy, fmt, `difftest run` (pre-existing goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate shading-patterns`; the colour oracle over `corpus/unit/shadings/` and the reusable-stream files with each verdict inspected; the external checker on the PDFs; the captured driver job still passes; design.md gains "## Implementation notes" with provenance and the smoothness default

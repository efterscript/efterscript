# Tasks: triage-fixes-4

## 1. Interpreter (ps-vm)

- [x] 1.1 `rand`/`srand`/`rrand` with the project's generator and fixed seed; `usertime` from the execution counter; the `Clock` capability and `realtime`; wrap-around; verified by unit tests (full period of the generator on a reduced modulus, sequence reproduction, wrap) and the two interpreter-core scenarios as `% backend: none` corpus files
- [x] 1.2 `languagelevel` 3, `serialnumber`, `version`/`product`/`revision` in `systemdict` tied to the identity configuration, `FontType` 9 and 11; verified by the two identity scenarios and the modified languagelevel scenario as corpus files and the category tests
- [x] 1.3 `setstrokeadjust`/`currentstrokeadjust` (not reset by `initgraphics`, `false` at glyph start) and `setoverprint`/`currentoverprint` in the VM graphics state with the `set_overprint` hook; verified by unit tests and the overprint scenario's getter half
- [x] 1.4 The page device as a graphics-state field with `gsave`/`grestore`/`grestoreall`/`restore` and `currentpagedevice`; verified by the page-size scenario as a corpus file with an IR golden and a unit test across `save`/`restore`

## 2. Graphics (ps-graphics, ps-vm)

- [x] 2.1 `pathbbox` rules (control points, trailing `moveto`, `setbbox`, empty path) and double-precision readings for `pathbbox`, `currentpoint`, `arcto`; verified by the two reading scenarios as corpus files and unit tests on the generator's recorded cases, every existing IR golden byte-identical
- [x] 2.2 `outline.rs`: flattening by flatness, dashes, offsets under the CTM, joins, caps, degenerate subpaths; the `stroke_outline` hook and `strokepath`; `ustrokepath` both forms; verified by unit tests on each join/cap and a dashed curve, and the two stroke-outline scenarios as corpus files with IR and PDF goldens
- [x] 2.3 `IrOp::Overprint` through the emitter with dedup and the dump line; verified by the overprint scenario's IR golden and every existing IR golden byte-identical

## 3. Writer (remelt)

- [x] 3.1 `ExtGState` resources with `OP`/`op` and `gs` selection; verified by the remelt scenario as a PDF golden, a sink test reading the resource back, every existing PDF golden byte-identical

## 4. Corpus and verification

- [x] 4.1 The remaining D7 corpus files (each cap and join, dashed curve, closed rectangle, degenerate subpath, `pathbbox` variants, page device across `save`/`restore`, `rrand` round trip) with goldens; verified by `difftest run`
- [x] 4.2 `cargo test --workspace`, clippy, fmt, `difftest run` (pre-existing goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate triage-fixes-4`; the oracle over the new files (colour for the overprint file) with each verdict inspected; the external checker; the captured driver job still passes; design.md gains "## Implementation notes" with provenance, the generator parameters, the `usertime` scale, and the LanguageLevel 3 gap list

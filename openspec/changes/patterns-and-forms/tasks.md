# Tasks: patterns-and-forms

## 1. Boundary and colour model (ps-vm)

- [ ] 1.1 `SpaceSpec::Pattern { base }`, `PatternInfo`, `FormInfo`, `set_pattern`, `begin_pattern_cell`/`end_pattern_cell`, `begin_form`/`end_form`/`place_form` on the trait with mock defaults; `[/Pattern]`/`[/Pattern base]` accepted by `setcolorspace`; verified by ps-vm unit tests on the space arity and the mock, workspace compiling
- [ ] 1.2 `Pattern` and `Form` as regular categories with instance shape checks; `CATEGORIES` gains both; verified by the two category scenarios as corpus files and the sorted-table test

## 2. Patterns (ps-vm)

- [ ] 2.1 `makepattern` (validation, the instance dictionary, the id table, the concatenated matrix), `setpattern`, `setcolor` with a pattern operand, `currentcolor`/`currentcolorspace`, save/restore of the pattern colour; verified by the instance and shading-out-of-range scenarios and unit tests for the errors
- [ ] 2.2 `LoopFrame::PatternCell`: first paint asks `begin_pattern_cell`, runs `PaintProc` under capture with the uncoloured colour-operator rule, re-dispatches the operator; every painting operator (fills, strokes, rect forms, text, `imagemask`, user-path paints) routed through it; verified by the coloured, uncoloured, and one-capture-two-fills scenarios with IR goldens and a unit test that a raising procedure restores the state

## 3. Forms (ps-vm)

- [ ] 3.1 `execform` with validation, `FormInfo` from the dictionary handle, `LoopFrame::FormBody` capture on first use, `place_form` every time, nested forms; verified by the placement, bad-form, three-placements, and nested scenarios with IR goldens

## 4. User paths (ps-vm)

- [ ] 4.1 `ops/upath.rs`: the literal walker, `setbbox` with the range and ordering checks, `uappend`, `ufill`, `ueofill`, `ustroke` with and without matrix, `upath`, `ucache`, `ucachestatus`, `setucacheparams`; verified by the round-trip, matrix-stroke, and out-of-box scenarios and unit tests for the errors
- [ ] 4.2 Encoded user paths: the homogeneous number array decoder for every representation and byte order, the operator string with repeat counts; verified by decoder unit vectors and the encoded-equals-literal scenario

## 5. IR and dump (ps-graphics)

- [ ] 5.1 Generic `Capture` with `Target::{Glyph, Pattern, Form}`; `PatternSpec`/`FormSpec` resources; `IrOp::Form` and `IrOp::SetPattern` with the emitter's dedup key; pattern matrices taken through enclosing captures; the dump blocks and colour line; verified by the three graphics-ir scenarios as IR goldens, `tests/eexec`-style backend unit tests for nesting, and every existing IR golden byte-identical

## 6. Writer (remelt, pdf-out)

- [ ] 6.1 Pattern objects, form XObjects, per-content resource dictionaries, `Pattern` colour-space selection for fill and stroke with components, `Do` under a matrix; verified by the three remelt scenarios as PDF goldens, a remelt sink test reading back the objects, and every existing PDF golden byte-identical

## 7. Corpus and verification

- [ ] 7.1 The remaining D8 corpus files (pattern stroke, pattern text, pattern inside a form, form with text and image, pattern across save/restore, error cases) with goldens; verified by `difftest run`
- [ ] 7.2 `cargo test --workspace`, clippy, fmt, `difftest run` (pre-existing goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate patterns-and-forms`; the oracle tier over the three new directories with each verdict recorded; the external checker on the pattern and form goldens; the captured driver job still passes; design.md gains "## Implementation notes" including the `upath` cacheable-form decision

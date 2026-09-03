# Tasks: cff-fonts

## 1. Engine (ps-fonts)

- [x] 1.1 CFF reader: header, indexes, dictionaries, charsets, encodings, private data, CID-keyed structures; verified by unit tests on synthesised fonts of both keyings
- [x] 1.2 Type 2 charstring interpreter with width rule, hints and masks, subroutines with bias, flex forms, accent endchar, arithmetic escapes, trace mode; verified by per-operator unit tests
- [x] 1.3 `Program::Cff`, `ProgramKind::Cff`, glyph lookup by name, index, and CID; verified by engine tests and the CID-keyed scenario
- [x] 1.4 `testing::CffFont` builder and Type 2 encoder; optional TeX Gyre OpenType test with `fetch-fonts --test-assets`; verified by round-trip tests and, when present, every glyph of the OpenType file interpreting

## 2. Loading (ps-vm)

- [x] 2.1 `ProcSet` and `FontSet` resource categories, `FontSetInit` with `StartData`, FontType 2 dictionaries with cached programs, `definefont` accepting FontType 2, snapshot path; verified by the FontSet scenarios as corpus files and the width and charpath scenarios
- [x] 2.2 Corpus generator for FontSet files; difftest and parse-survival tolerate the binary section; verified by `difftest run` and `parse-survival` green

## 3. Embedding (ps-fonts, remelt)

- [x] 3.1 CFF writer with subset, charset, pruned and renumbered subroutines; verified by the round-trip scenario and a bias-boundary test
- [x] 3.2 `FontFile3`/`Type1C` embedding with descriptor fields; verified by the Type1C scenario with goldens and a checker

## 4. Verification

- [x] 4.1 `cargo test --workspace`, feature-off ps-fonts, clippy clean, fmt, `difftest run`, `parse-survival`, `openspec validate cff-fonts`; every pre-existing golden byte-identical; design.md gains "## Implementation notes"

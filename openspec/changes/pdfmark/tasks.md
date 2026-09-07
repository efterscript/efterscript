# Tasks: pdfmark

## 1. Operator and boundary (ps-vm)

- [x] 1.1 `MarkValue`, the `pdfmark` operator (defined with a backend), value conversion and errors, the additive trait method with a default; mock records calls; verified by unit tests and the guarded-job and typecheck scenarios as corpus files

## 2. IR (ps-graphics)

- [x] 2.1 `Annot`, `DocMark`, `Target`, `View`; `Page.annots`; `PageSink::document` and `finish` defaults; backend parsing of the honoured kinds, page-index resolution, rectangle transform, unknown kinds counted; dump lines and `doc:` section; verified by backend tests and every pre-existing golden byte-identical
- [x] 2.2 `moveto` replacement rule; verified by the two-moves scenario and existing tests

## 3. Writer (remelt) and CLI

- [x] 3.1 Outlines, named destinations, annotations, info merge, catalog view entries, page attributes, written deterministically; `Report` counts; CLI line; verified by sink tests on hand-built pages and marks, the no-marks byte-identity check, and the scenarios with goldens
- [x] 3.2 External-checker verification of outlines, page mode, and title on the goldens (private tier or `EFTERSCRIPT_PDF_CHECK`), recorded

## 4. Corpus and verification

- [x] 4.1 Corpus files under `corpus/unit/pdfmark/` and the moveto file with goldens; `difftest run` green; private oracle run over the corpus recorded (raster verdicts unchanged)
- [x] 4.2 `cargo test --workspace`, clippy, fmt, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate pdfmark`; design.md gains "## Implementation notes"

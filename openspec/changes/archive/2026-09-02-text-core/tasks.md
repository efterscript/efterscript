# Tasks: text-core

## 1. Metrics and data (ps-fonts)

- [x] 1.1 Promote `core14/` (14 AFMs + Adobe licence, unmodified) and `glyphlist.txt` into `crates/ps-fonts/data/` with `PROVENANCE.md`; verified by a test that each file's SHA-256 matches the vault's `SHA256SUMS` entry when `EFTERSCRIPT_HELLBOX` is set (skips cleanly otherwise)
- [x] 1.2 AFM parser and `StdFont` enum: widths by glyph name, bounding box, built-in encoding, lazily parsed; verified by unit tests on Helvetica `H`=722, Symbol's code 97 = `alpha`, and all fourteen parsing
- [x] 1.3 `StandardEncoding`/`ISOLatin1Encoding` tables and `substitute(name) -> StdFont`; verified by tests for the alias, heuristic, subset-tag, and style cases in the spec and by the standard table agreeing with the text fonts' AFM codes
- [x] 1.4 Glyph-name → Unicode mapping (glyph list, `uniXXXX`, `uXXXX`, suffix stems); verified by unit tests including an unmapped name returning `None`

## 2. Boundary (ps-vm ↔ ps-graphics)

- [x] 2.1 `FontRef`, `Glyph` value types; `GraphicsBackend` gains `set_font`/`font`, `show(glyphs)`, `begin_glyph`/`end_glyph`, mock backend updated; verified by the existing ps-vm tests still passing with the extended mock

## 3. VM font semantics (ps-vm)

- [x] 3.1 `ops/font.rs`: `definefont` validation and `FID`, `FontDirectory`/`GlobalFontDirectory`, `findfont` with resident materialisation (D2) and substitution (D9, `Config::fonts.substitute`), `scalefont`/`makefont`/`setfont`/`selectfont`/`currentfont`; verified by the font-dictionary scenarios as corpus files
- [x] 3.2 `LoopFrame::Show` and `stringwidth` for resident fonts, all show variants and `glyphshow`; `charpath` raising `invalidfont`; verified by the width, advance, re-encoding, widthshow, xshow, and kshow scenarios as corpus files
- [x] 3.3 Type 3 execution: per-glyph steps with gsave, CTM setup, `BuildGlyph`/`BuildChar` dispatch, `setcachedevice`/`setcachedevice2`/`setcharwidth`, measuring mode for `stringwidth`; verified by the Type 3 scenarios through the mock backend (glyph begin/end and widths observed)
- [x] 3.4 `ops/resource.rs`: `findresource`/`resourcestatus`/`defineresource`/`undefineresource`/`resourceforall` over `Font` and `Encoding`, `StandardEncoding`/`ISOLatin1Encoding` in systemdict; verified by the encoding and resource scenarios as corpus files
- [x] 3.5 Font in save/restore: `restore` invalidating font dictionaries while the graphics state is restored past them; verified by a test defining a font inside `save`, setting it, and restoring

## 4. Graphics and IR (ps-graphics)

- [x] 4.1 Font in `GState`; `FontSpec` resources interned per page (D6); `IrOp::Text` with glyph displacements (D5); lazy colour before text; verified by backend tests building runs by hand
- [x] 4.2 Glyph capture (D4): redirection into a glyph procedure with base-matrix-relative coordinates, fresh emission state, dedup on `end_glyph`, measuring mode, page ops refused while capturing; verified by backend tests and the Type 3 scenarios through the interpreter
- [x] 4.3 Dump lines for fonts, glyph blocks, and text (D7); verified by every pre-existing `.ir` golden unchanged and new goldens for the text scenarios

## 5. PDF (remelt)

- [x] 5.1 Font objects once per document (D12): resident Type 1 dictionaries with encoding differences, widths, ToUnicode CMap stream; page resource references; verified by sink tests on hand-built pages checking the objects and the CMap text
- [x] 5.2 Type 3 font dictionaries with CharProcs via the content writer (`d0`/`d1`), encoding, widths, bounding box; verified by the square-glyph scenario
- [x] 5.3 Text in the content writer (D11): `BT`/`Tf`/`Tm`/`Tj`/`TJ`/`Td`/`ET`, resident and Type 3 matrices, singular font matrix skipped; verified by the "Hi" scenario and an xshow scenario checking `TJ` adjustments
- [x] 5.4 Substitution reporting in `Report` and on the CLI's standard error; verified by a CLI test on a corpus file referencing `Arial`

## 6. Corpus and verification

- [x] 6.1 Corpus files under `corpus/unit/text/` for every scenario with `.ir` and `.pdf` goldens; `difftest run` green; text extraction of the "Hi" golden yields `Hi` under `EFTERSCRIPT_PDF_CHECK` when a checker is available
- [x] 6.2 `cargo test --workspace`, `cargo clippy --workspace --all-targets` clean, `cargo fmt --check`, `cargo xtask parse-survival`, `openspec validate text-core`; design.md gains "## Implementation notes"

# Tasks: composite-fonts

## 1. CMaps (ps-fonts, ps-vm)

- [x] 1.1 `cmap` module: model, decoding rule, chaining; verified by unit tests for one- to four-byte codespaces, partial matches, notdef ranges, and `usecmap`
- [x] 1.2 `CIDInit` procedure set operators building a CMap; `CMap` resource category; `Identity-H`/`Identity-V` assets with licence and provenance loaded through the interpreter; verified by the CMap scenarios as corpus files and the provenance test

## 2. CID-keyed fonts and Type 0 (ps-vm, ps-fonts)

- [x] 2.1 `CIDFont` category; CID-keyed CFF from FontSets defines the resource; `CIDFontType 2` from `sfnts` + `CIDMap`; `Program::glyph_by_cid` across variants; verified by the CFF-from-FontSet and CIDFontType 2 scenarios
- [x] 2.2 `CIDInit` `StartData` form with Type 1 charstrings (`GlyphData`, `CIDMap`, `FDArray` privates) into a CID program; `testing::CidType1Font`; verified by the Type 1 charstring scenario
- [x] 2.3 Type 0 dictionaries and `composefont`; `definefont` validation for map type 9 and rejection of others; verified by the Type 0 scenarios as corpus files
- [x] 2.4 Composite decoding in the show frame (all show variants, `stringwidth`, `charpath`, vertical mode); the widened `Glyph` in ps-vm and the mock backend; verified by the composite-text scenarios through the mock and as corpus files

## 3. IR (ps-graphics)

- [x] 3.1 Widened `Glyph`, `wmode` on text, `FontSpec::Composite`, vertical positioning, dump forms; verified by backend tests, the dump scenario, and every pre-existing golden byte-identical

## 4. PDF (ps-fonts, remelt)

- [x] 4.1 CID-keyed CFF subset writer; verified by the round-trip test over every kept CID and a two-dictionary font
- [x] 4.2 Type 0 output: descendants, `W`, `CIDToGIDMap`, `FontFile3`/`FontFile2`, ToUnicode sources, two-byte content strings; verified by the three embedding scenarios with goldens and poppler checks
- [x] 4.3 Type 3 fallback for the Type 1 charstring form; verified by its scenario with goldens and a checker

## 5. Corpus and verification

- [x] 5.1 Corpus files for every scenario under `corpus/unit/fonts/` (CMaps, FontSets, CIDFonts, composite text) with `.ir` and `.pdf` goldens; `difftest run` and `parse-survival` green
- [x] 5.2 `cargo test --workspace`, feature-off ps-fonts, clippy clean, fmt, `openspec validate composite-fonts`; design.md gains "## Implementation notes"

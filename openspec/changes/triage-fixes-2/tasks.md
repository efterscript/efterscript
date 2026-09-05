# Tasks: triage-fixes-2

## 1. Interpreter fixes (ps-vm, ps-fonts)

- [ ] 1.1 Loaded flag and `resourcestatus` 1 for materialised predefined resources; corpus expectations updated (`cmap-identity-predefined`, `resident-set-reported`, and any file printing a status after loading); verified by the two status scenarios as corpus files
- [ ] 1.2 `setpagedevice` type table; `pagedevice-merges.ps` corrected; verified by the ill-typed-key scenario as a corpus file and `difftest run`
- [ ] 1.3 `definefont` program presence for Type 1 and Type 42; `type1-without-program.ps` expectation moved to `definefont`; verified by the scenario and existing font tests
- [ ] 1.4 `.notdef` width lookup order; `resident-charpath-missing-glyph.ps` declares `resident-inventory`; new scenario file with a TeX Gyre face; verified by unit tests and `difftest run`

## 2. Harness (tools/difftest)

- [ ] 2.1 Text "not comparable" when EfterScript's page fonts lack ToUnicode (oracle copy distilled uncompressed); `error_marker` profile key with truncation and the ended-in-error flag; fake-profile tests; verified by the two new scenarios

## 3. Records and probe

- [ ] 3.1 `procedure-nesting-limit` header on `scanner/procedure-depth-limit.ps`; the registry requirement resolves; verified by `difftest run` and slug resolution
- [ ] 3.2 Vertical-writing probe in the private tier per D7; generator/loader fix or divergence record (added to this change's delta if a record) with the variant matrix in the notes; verified by the oracle run on `composite-vertical-width.ps`

## 4. Verification

- [ ] 4.1 Private oracle run over the whole corpus (the vault profile gains `error_marker`); totals and any remaining named items recorded; `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `lint-strings`, `openspec validate triage-fixes-2`; design.md gains "## Implementation notes"

# Tasks: resident-outlines

## 1. Assets and provenance

- [ ] 1.1 `cargo xtask fetch-fonts` (download, archive checksum, extract listed files, compare with provenance, `--check`/`--force`); verified by running it with `--check` against the committed files
- [ ] 1.2 Commit the twelve Liberation `.ttf`, the twenty-one TeX Gyre `.pfb` and `.afm`, both licence texts beside the files, `LICENSES/OFL-1.1.txt` and `LICENSES/LPPL-1.3c.txt`, and the provenance entries with SHA-256s; verified by the provenance test hashing every file

## 2. Type 1 file parser (ps-fonts)

- [ ] 2.1 `type1::parse_file` for PFB and PFA producing `Type1Program` + `Type1Dict`; verified by round-tripping the synthesised corpus fonts and by parsing every TeX Gyre file with all glyphs interpretable

## 3. Resident set (ps-fonts, ps-vm)

- [ ] 3.1 `ResidentFace` (35) with metrics, outline asset, `std_font`, names, `from_index`; TeX Gyre AFMs wired; marker index over 35; verified by unit tests and the extra-face width scenario
- [ ] 3.2 Outline lookup with post-name and Unicode fallback, per-face cache, feature gate; verified by tests that every Core 14 glyph name resolves in each matching Liberation face and by the fallback and missing-glyph scenarios
- [ ] 3.3 Alias table and style mapping for the LaserWriter families; resource operators report 35; verified by the alias and resource scenarios as corpus files
- [ ] 3.4 `charpath` resident branch through the outline lookup; Symbol/ZapfDingbats/Type 3 `invalidfont`; `% requires:` header honoured by difftest; verified by the resident charpath scenarios with goldens

## 4. PDF (ps-graphics, remelt)

- [ ] 4.1 Extra faces described as `FontSource::Embedded` from the parsed asset; fourteen unchanged; verified by the Palatino embedding scenario and the Helvetica-unembedded scenario with goldens, and `pdffonts` showing the embedded subset

## 5. Verification

- [ ] 5.1 `cargo test --workspace` with the feature on and off, `cargo clippy --workspace --all-targets` clean, `cargo fmt --check`, `difftest run`, `cargo xtask parse-survival`, `openspec validate resident-outlines`; every pre-existing golden byte-identical; design.md gains "## Implementation notes"

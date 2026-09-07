# Tasks: distillation-policy

## 1. Encoder (pdf-out)

- [ ] 1.1 DEFLATE encoder with LZ77 and fixed Huffman, stored fallback, test inflater, round-trip proptests, size scenarios; `Document::set_version` header patch for seekable sinks; verified by the two encoder scenarios and every pdf-out test

## 2. Parameters (ps-vm, ps-graphics)

- [ ] 2.1 `setdistillerparams`/`currentdistillerparams` with the type table and tolerance; the additive backend method; `DocMark::Params`; verified by the round-trip and typecheck scenarios as corpus files

## 3. Writer (remelt) and CLI

- [ ] 3.1 `Params` model, precedence with locks, per-page and finish-time application, compatibility header, report fields, CLI flags; verified by the compression-off, locked-key, and header scenarios with goldens
- [ ] 3.2 Embed-all through the existing embedding path with the outline assets; verified by the Helvetica scenario with goldens and the external checker showing the font embedded
- [ ] 3.3 Downsampling for the supported classes; verified by the 300-to-72 scenario with goldens and a rendered tolerance check, and reporting for unsupported images

## 4. Corpus and verification

- [ ] 4.1 Corpus files under `corpus/unit/policy/` with goldens; compressed goldens re-pinned once and listed; `difftest run` green; private oracle run recorded (verdicts unchanged)
- [ ] 4.2 `cargo test --workspace`, clippy, fmt, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate distillation-policy`; design.md gains "## Implementation notes"

# Tasks: divergence-registry-2

## 1. Harness

- [x] 1.1 `% oracle: skip <reason>` header, `skipped` verdict, summary count, JSON field; fake-profile test; `difftest run` ignores it; verified by the skip scenario

## 2. Registry and headers

- [x] 2.1 The thirteen registry requirements in the change's delta; slug resolution test against the delta; verified by `openspec validate` and the harness accepting each slug
- [x] 2.2 Headers placed: divergence slugs on every file the triage listed for T7, T8b, T9, T10, T11 and `derived-fonts.ps`; `% oracle: skip` on the three `% backend: none` files and `interp/deep-recursion.ps`; verified by `difftest run` unchanged (141/141, goldens byte-identical)

## 3. Showpage endings

- [x] 3.1 `showpage` appended to the six text files, `.ir`/`.pdf` goldens created and reviewed; `% divergence: unshown-marks-not-flushed` not needed on them any more; verified by `difftest run` green

## 4. Private run and verification

- [x] 4.1 Oracle run over the whole corpus with the private profile; totals and the named open items recorded in the notes; `cargo test --workspace`, clippy, fmt, `parse-survival`, `lint-strings` clean, `openspec validate divergence-registry-2`; design.md gains "## Implementation notes"

# Tasks: deflate-dynamic-huffman

## 1. Encoder

- [ ] 1.1 Package-merge length-limited code builder with canonical assignment; verified by proptests (Kraft inequality, length limit, determinism) and the one-symbol edge cases
- [ ] 1.2 Dynamic block emission: run-length coded code lengths, code-length code, header counts; three-way per-block choice; verified by round trips through the test inflater over random, repetitive, and mixed inputs and by the three size scenarios

## 2. Verification

- [ ] 2.1 Re-pin the compressed goldens once (listed), run an independent inflater over every regenerated golden from the shell, `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate deflate-dynamic-huffman`; design.md gains "## Implementation notes" with measured ratios before and after

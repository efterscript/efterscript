# Tasks: psgen-v0

## 1. Interpreter

- [x] 1.1 `Limits::steps` and the budget check in the execution loop, `limitcheck` on exceed, re-checked after a handler; unit tests incl. the `{ } loop` scenario; corpus unaffected

## 2. Generator (tools/psgen)

- [x] 2.1 Random source, stack model, dictionary model, profiles `core` and `graphics`, statement grammar with bounded loops, procedures with signatures, `stopped` share, ill-typed share; `psgen gen`; verified by the reproducibility test and the well-typed scenario (100 programs, no errors)
- [x] 2.2 Runner: in-process execution with capture, sink, budget; `psgen check` with the determinism, budget, panic properties and the PDF structural check; verified by unit tests on hand-written programs and a planted failure
- [x] 2.3 Metamorphic relations: save/restore, gsave/grestore, translation, definition reordering; verified by tests with programs that satisfy and programs that violate each
- [x] 2.4 `psgen shrink` with in-process property and external predicate; verified by the planted-failure scenario

## 3. Seeds and xtask

- [x] 3.1 Seed files for both profiles, `cargo xtask fuzz-round` with `--profile` and `--oracle`, corpus README section; verified by running the round on the committed seeds

## 4. First round and verification

- [x] 4.1 A first fuzz round beyond the seed files (both profiles, thousands of programs) with every failure shrunk, classified, and recorded in the notes; private-tier oracle round over generated output recorded the same way; findings become follow-up suggestions
- [x] 4.2 `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `lint-strings`, `openspec validate psgen-v0`; design.md gains "## Implementation notes"

# Tasks: cli-execution-budget

## 1. Command-line tool

- [ ] 1.1 `--budget <n>|unlimited` parsed once for all modes, the default of 100 million objects in every mode's `Limits`, usage errors for zero and non-integers, help text; verified by the CLI tests for the flag's parsing and by every corpus file still passing `difftest run` with byte-identical goldens
- [ ] 1.2 Exit status 3 and the standard-error report for a run ended by the budget, the PDF still written in `pdf` mode; verified by the runaway and adjustable scenarios as CLI integration tests, plus a test that `pdf` writes the file on exhaustion

## 2. Verification

- [ ] 2.1 `cargo test --workspace`, clippy, fmt, `difftest run` (goldens byte-identical), `parse-survival`, `fuzz-round`, `lint-strings`, `check-wasm`, `openspec validate cli-execution-budget`; both captured driver jobs distil under the default budget with their object counts recorded; the measured wall time of a 100-million-object loop in a release build recorded; design.md gains "## Implementation notes"

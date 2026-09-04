# Tasks: triage-fixes-1

## 1. StartData

- [x] 1.1 `StartData` pops the procedure set's dictionary; unit test through the interpreter with `countdictstack`; verified by the dictionary-stack scenario as a corpus file
- [x] 1.2 Generator drops the trailing `end`; regenerate the thirteen FontSet corpus files; drift check passes; goldens re-pinned only where bytes changed (expected none); verified by `difftest run` green

## 2. CIDInit probe (private tier)

- [x] 2.1 Probe the two CIDInit-form files against the reference converter under the corrected loader with minimal variants; fix the generator or loader if the cause is ours and small, else record; verified by the oracle harness result recorded in the notes

## 3. Output forms

- [x] 3.1 `type` returns an executable name; `==` writes null as `null`; unit tests; corpus expectations updated in the seven and three affected files; verified by `difftest run` green and the two scenarios as corpus files

## 4. Verification

- [x] 4.1 `cargo test --workspace`, clippy clean, fmt, `difftest run`, `parse-survival`, `lint-strings` clean, `openspec validate triage-fixes-1`; private-tier oracle run recorded (before/after totals) in the notes; design.md gains "## Implementation notes"

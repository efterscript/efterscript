# Tasks: printer-identity-mechanism

## 1. Dictionaries, seeding, prelude (ps-vm, cli)

- [ ] 1.1 `statusdict` and `serverdict` at construction with the default identity; `exitserver` with the configured password; verified by the three statusdict scenarios as corpus files
- [ ] 1.2 `Config::identity`, `Config::prelude`, `Config::server_password`; prelude execution and failure reporting; CLI `--identity` and `--prelude`; `Report` fields; verified by the seeding, prelude, and failure scenarios (Rust tests with configured interpreters and a CLI test)

## 2. Implicit resource categories (ps-vm)

- [ ] 2.1 Implicit category table and the five operators' behaviour on it, `invalidaccess` on define; a test that every claimed font type defines and every listed category resolves; verified by the implicit-categories scenario as a corpus file

## 3. Accepted device operators (ps-vm, ps-graphics)

- [ ] 3.1 Screen and transfer setters and getters recorded in the graphics state, `framedevice`, `cexec`; verified by the two scenarios as corpus files with an unchanged `.ir` golden

## 4. Private check and verification

- [ ] 4.1 Private tier: a prelude beside the captured driver job in the emulator's test data (never in repo A); run the job with it; fix interpreter gaps it exposes when they are language semantics, each with a clean-room scenario here; record how far the job renders and what remains; oracle comparison of the page when it renders
- [ ] 4.2 `cargo test --workspace`, clippy, fmt, `difftest run`, `parse-survival`, `fuzz-round`, `lint-strings`, `openspec validate printer-identity-mechanism`; design.md gains "## Implementation notes"

## ADDED Requirements

### Requirement: The C interface as a WebAssembly module

The session library SHALL build for `wasm32-unknown-unknown` as a
`cdylib` whose exports are the C interface's `platen_*` functions and
its memory, and which imports nothing, so a JavaScript host can drive
it without Emscripten; the build SHALL need no change to the crate's
declared crate types and no code beyond the existing C interface.

#### Scenario: A self-contained module

- **WHEN** `cargo xtask npm-package` builds the module
- **THEN** the module's import list is empty and its exports include `platen_job_new`, `platen_job_feed`, `platen_job_finish`, `platen_job_pdf`, and `memory`

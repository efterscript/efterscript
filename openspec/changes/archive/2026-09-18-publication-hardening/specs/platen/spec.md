## ADDED Requirements

### Requirement: The C interface is stable across the crate rename

Renaming the session crate SHALL leave the static library name
`libplaten.a`, the header `platen.h`, and every `platen_*` symbol
unchanged, so an embedder linking the previous build links the new
one without modification.

#### Scenario: The emulator bridge links unchanged

- **WHEN** the session library is rebuilt after the rename and linked by a host that includes `platen.h` and calls `platen_job_new`
- **THEN** the host builds and runs without any source change

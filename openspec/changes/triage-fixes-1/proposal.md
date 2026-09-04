# Change: Triage fixes 1 — StartData ends its dictionary, executable type names, null in syntactic form

## Why

The first oracle triage produced three probable bugs with reference
support and small reproductions; together they account for 23 of the
corpus files that disagree with the reference converter. Each is a
one-place fix, but each changes observable output and goldens, so they
travel as one reviewed change rather than as untracked edits. The
FontSet one also corrects the corpus generator, which encoded the same
mistake, and closes or reclassifies the undetermined CIDInit-form
finding by probing it under the corrected loader.

## What Changes

- **`StartData` ends the procedure set's dictionary.** After reading
  the data and defining the fonts, `StartData` SHALL pop the dictionary
  the enclosing `findresource begin` pushed, matching the canonical
  FontSet file form that carries no trailing `end`. The corpus generator
  stops emitting the compensating `end`; the thirteen FontSet corpus
  files are regenerated and their goldens re-pinned (the IR and PDF
  content is unchanged; only the program text differs).
- **CIDInit-form probe.** With the loader corrected, the two CIDInit-form
  CID font corpus files are probed against the reference converter
  through the oracle harness in the private tier; if the cause is a
  missing or misplaced entry in the synthesised form, the generator is
  fixed and the files regenerated; otherwise the finding is recorded
  with what was learned, for the registry or a later change.
- **`type` returns an executable name** (`1 type xcheck` is `true`;
  `==` prints it without a slash), per the reference's description of
  the operator. Every corpus file printing a type name with `==` changes
  its expectation; goldens that contain such output are regenerated.
- **`==` prints null as `null`**, its syntactic form, rather than
  `-null-`; `=` is unchanged. Corpus expectations follow.
- Out of scope: the other triage findings (their own changes).

## Capabilities

### New Capabilities
- none.

### Modified Capabilities
- `font-programs`: "FontSet resources load CFF fonts" — `StartData`
  ends the procedure set's dictionary.
- `interpreter-core`: ADDED requirements for the executable type name
  and the syntactic form of null in `==`.

## Impact

- Code: `crates/ps-vm` (`ops/fontset.rs`, `ops/types.rs`,
  `ops/output.rs`; possibly `ops/cidinit.rs` if the probe finds a
  loader-side cause), `crates/ps-fonts/tests/corpus_fonts.rs` (the
  generator), thirteen FontSet corpus files and their goldens, seven
  corpus files with `type ==` output and three with `null ==`.
- After the change the oracle harness should report these files as
  pass on the document channel and same on the output channel; the
  private tier confirms and records the new totals.
- Depends on `oracle-testing`, `cff-fonts`, `composite-fonts`
  (archived).

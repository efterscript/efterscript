# Design: version-free-goldens

## D1: An option, not a comparison rule

The producer with its version is a spec requirement and useful in the
field, so it stays the default. Normalising the `Producer` line at
comparison time was rejected: the version's length moves every offset in
the cross-reference table after it, so a byte comparison would need to
parse the file. A sink option that omits the version keeps the goldens
plain bytes.

## D2: Where the option lives

`Options` gains `versioned_producer: bool` (default `true`) and the
builder `unversioned_producer()`. The sink carries the flag and writes
`EfterScript` or `EfterScript <version>` at finish. Users of the corpus
runner's golden writer, the facade's golden test, and the marks crate's
golden test set it; every other test keeps the default, including the
one that asserts the versioned form.

## D3: One regeneration

`difftest run --update-pdf` rewrites every golden. The diff is checked
to touch only the `Producer` line, the cross-reference offsets after it,
and `startxref`.

## Implementation notes

- **As built.** `Options::versioned_producer` (default `true`) with the
  builder `unversioned_producer()`; the sink carries the flag and picks
  `EfterScript` or `EfterScript <version>` at finish. The corpus runner's
  golden writer, the facade's golden test, and the marks crate's golden
  test set it. The sink test asserting the versioned form is untouched
  and still passes, as the default.
- **The regeneration.** `difftest run --update-pdf` rewrote 154 goldens,
  236 lines each way. Every changed line is a `Producer` entry (in two
  files inside a fuller Info dictionary from `pdfmark`), a cross-reference
  entry, or the `startxref` value; a script over `git diff -U0` found
  nothing else.
- **Checked here.** Workspace tests pass, including the three targets
  that failed on the tag; formatting and clippy on the touched crates are
  clean; the string lint passes.

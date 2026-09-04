# Design: Divergence registry 2

See proposal.md. This is corpus and harness bookkeeping; the decisions
are about how headers are assigned and what the private run must show.

## Context

- The harness resolves `% divergence:` slugs against the registry, and
  `difftest run` ignores the header. There is no way to exclude a file
  from the oracle tier; build-specific scenarios (`% backend: none`) and
  the one timeout therefore count as failures on every run.
- Six text files paint text and end without `showpage`; ours emits no
  page, the reference flushes one, so the harness compares nothing for
  them. With a `showpage` they become raster comparisons of text.
- Registry entries are requirements; the harness only needs the name.

## Decisions

**D1. One slug per cause, not per file.** Slugs name the decision;
files declare the slug that explains their difference. A file with two
causes declares the one that produces the mismatch on the channel that
fails; the notes list any file where two would apply.

**D2. `% oracle: skip <reason>`** is parsed by the expectation reader
alongside the other headers; `difftest run` ignores it; the oracle
subcommand reports `skipped: <reason>` without running any command.
Reserved for scenarios that cannot be compared in principle (a build
without a backend, a limit test that never terminates in the
reference), never for inconvenient mismatches — those get a divergence
slug or stay failures. *Alternative:* a `% requires:` feature — that
header means "feature off in this build", a different thing.

**D3. Showpage endings.** Each of the six files gains `showpage` as its
last operator and, where its expected output is a `currentpoint` or
width, keeps that output; `--update-ir`/`--update-pdf` create their
goldens, which are reviewed once by eye (rasterised) before committing.
`type1-malformed-charstring.ps` keeps its error ending and gets the
malformed-font slug instead.

**D4. The private run's acceptance.** After headers: zero `fail` on the
document channel except the open probable bugs (T8a, T12, T13, T5) and
the two probes, which are listed by name in the notes as known open
items; every output-channel `differs` is either covered by a slug or
listed the same way. The notes record the totals.

## Risks / Trade-offs

- [Slugs become a way to silence real bugs] → every entry cites the
  reference's position and is reviewed as a change; the harness reports
  `divergence-closed` when a declared file starts matching, so a stale
  record is visible.
- [Rasterised text comparisons at 36 dpi are coarse] → they catch
  missing or misplaced runs, which is what the six files test.

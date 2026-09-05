# Test corpus — public tier

This directory holds only material we hold redistribution rights to (the commit-time test). Encumbered material — licensed suites, real-world
driver harvests, reference outputs from commercial converters and printer hardware — live exclusively in the
hellbox repo and never enters this history.

| Directory | Contents |
|---|---|
| `unit/` | Tiny hand-written `.ps` files, one operator/behavior each, authored from the published spec. Fully publishable. |
| `generated/` | Generator seeds/specs for `psgen` — not bulk outputs. |
| `golden/` | Reference outputs for regression comparison, stored as deterministic, diffable text (the normalized comparison form `difftest` produces) rather than binary PDFs — no git-LFS. Every generated file carries a `GENERATED-BY` marker so the set is enumerable. |

Rules:

- No test files or examples taken from other implementations or suites,
  whatever their license. No Red Book example programs beyond de minimis
  snippets. No harvested driver output (Apple/vendor
  ProcSets are copyrighted — those captures go to the hellbox; only distilled
  PDFs and clean-room minimal reproductions graduate here, with provenance
  recorded).
- Reference PDFs must not embed encumbered fonts: generate with free fonts
  (Liberation 2.x / URW LGPL-lineage / TeX Gyre releases) or no-embed
  configurations, and document font provenance here.
- A behavior covered only by hellbox data is the signal to author a clean-room
  public-tier equivalent.

## Declarations, verdicts, and the oracle tier

Every `unit/*.ps` file declares its own expectations in its leading
comment block (`% expect-output:`, `% expect-error:`, `% backend:`,
`% requires:`); `difftest run` checks them against the interpreter and
the file's goldens, and that is the whole public tier. Two further
headers speak to the oracle tier only and are ignored by `difftest run`:
`% divergence: <slug>` and `% oracle: skip <reason>`.

`difftest oracle` compares the same files against a reference converter
described by a profile that lives outside this repository (in the vault,
found through `EFTERSCRIPT_HELLBOX` or `EFTERSCRIPT_ORACLE_PROFILE`).
Its verdicts per file:

| Verdict | Meaning |
|---|---|
| `pass` | Page count, media box, pixels within tolerance, and extracted text (when the profile extracts it) agree. |
| `fail` | Something above differs, or the comparison could not be carried out. Only this verdict fails the run. |
| `expected-divergence` | The file declares `% divergence: <slug>` and something differs — a reviewed decision to behave differently, recorded as a requirement named `<slug>` in the `expected-divergences` specification. |
| `divergence-closed` | The file declares a divergence but nothing differs any more: the record may be retired. Informational. |
| `skipped` | The file declares `% oracle: skip <reason>`: nothing is converted or compared, and the reason is reported. Reserved for scenarios that cannot be compared in principle — a build without a graphics backend, a limit the reference never reaches — never for an inconvenient mismatch, which gets a divergence slug or stays a failure. |

The program's standard output is compared separately, after
normalisation, and shown as `output: same` or `differs` beside the
verdict. A `% divergence:` slug that names no requirement aborts the run
before anything is compared; the requirement may live in the living
`expected-divergences` specification or in the delta of an open change
that proposes it.

Nothing the reference converter produces ever lives in this repository:
its documents, renderings, and outputs stay under `target/oracle/`, and
this directory holds only EfterScript's own goldens. The converter is not
named anywhere here either — the profile is the only place that knows it.

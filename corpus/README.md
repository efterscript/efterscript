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

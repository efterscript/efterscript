# OpenSpec — the project's decision registry

This directory is the spec-driven-development registry: change
proposals, delta specs, and the expected-divergence records. It contains the
project's *decisions*, not Adobe's text, so it survives publication.

Initialize with the OpenSpec tool (Node 20+):

```sh
npx openspec init
```

## Division of authority (the anti-transcription rule)

- **The PLRM owns PostScript semantics.** Specs *cite* it ("per PLRM3 §3.7.3");
  they never restate it at length. No Red Book quotations in this repo — close
  paraphrase of the manual is both wasted effort and a copyright problem in a
  future-public repo.
- **These specs own what the PLRM doesn't:** architecture (IR shape, layer
  contracts, capability model), policies (distiller parameters, font-embedding
  rules, quirks-mode tolerances), deviations (expected-divergence records), and
  scope decisions (deferrals: rasterizer, JIT, banding, general job server).
- **The corpus owns behavioral truth.** Where prose would duplicate the spec, a
  corpus file plus golden output *is* the requirement; specs reference corpus
  IDs.

Significant changes start as an OpenSpec proposal; trivial fixes keep a
direct-commit escape hatch.

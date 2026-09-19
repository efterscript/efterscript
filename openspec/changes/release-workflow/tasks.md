# Tasks: release-workflow

## 1. Workflow

- [x] 1.1 `.github/workflows/release.yml` with the `verify` and `publish` jobs per D1–D3, pinned actions, the `release` environment, trusted publishing; verified by YAML parsing, a dry review of every step's command against the local gate chain, and a local run of the publish loop's skip logic against the registry API (all nine at 0.0.1 → nine skips)

## 2. Setup and record

- [ ] 2.1 Owner-side setup recorded (D4) and the first tag-triggered release rehearsed as far as the approval gate; design.md gains "## Implementation notes"

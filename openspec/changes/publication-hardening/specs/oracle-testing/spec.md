## ADDED Requirements

### Requirement: The public tier runs in continuous integration

A workflow SHALL run the public tier on every push and pull request on
the pinned toolchain: the workspace tests, clippy with no warnings,
formatting, the corpus run with byte-identical goldens, parse
survival, the generator round, the no-default-features build, and the
WebAssembly check; the private tiers (the strings lint against the
vault's list and the oracle comparison) SHALL NOT run there and the
workflow SHALL say so.

#### Scenario: A failing golden fails the workflow

- **WHEN** a push changes a corpus golden without updating it deliberately
- **THEN** the corpus run step fails and the workflow reports it

# text

## ADDED Requirements

### Requirement: CMap and CIDFont categories

The resource operators SHALL also operate on the `CMap` category
(`Identity-H` and `Identity-V` predefined with status 2, embedded CMaps
defined with status 0) and the `CIDFont` category (defined by FontSets,
`CIDInit` data, or `defineresource`), and `CIDInit` SHALL be a `ProcSet`
resource.

#### Scenario: CMap category listing

- **GIVEN** `(*) { == } 64 string /CMap resourceforall` before any
  embedded CMap
- **THEN** `Identity-H` and `Identity-V` are printed in sorted order

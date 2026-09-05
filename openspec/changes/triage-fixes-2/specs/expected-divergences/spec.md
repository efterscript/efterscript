# expected-divergences

## ADDED Requirements

### Requirement: procedure-nesting-limit

Procedure bodies nested deeper than the scanner's limit raise
`limitcheck` while scanning, where other interpreters scan arbitrarily
deep nesting; the reference makes such limits implementation-dependent,
and the limit bounds memory during scanning of untrusted input.

#### Scenario: Declared

- **GIVEN** the corpus file nesting procedures beyond the limit
- **THEN** it carries `% divergence: procedure-nesting-limit`

### Requirement: vertical-default-metrics

Through a vertical CMap, a CIDFont carrying no vertical metrics advances
by the default vertical metrics of PLRM3 §5.11 — one em downward, the
glyph placed at its vertical origin — where the reference interpreter
advances by the glyph's horizontal width and moves nothing vertically,
whatever the CIDFont declares (`WMode` 1, `W2`/`DW2` entries, a
`CIDFontType 2` descendant, an explicit Type 0 font with `WMode` 1);
what input would make the reference advance vertically is not known,
and ours follows the specification's default.

#### Scenario: Declared

- **GIVEN** the corpus file showing through `Identity-V` a synthesised
  CIDFont without vertical metrics
- **THEN** it carries `% divergence: vertical-default-metrics`

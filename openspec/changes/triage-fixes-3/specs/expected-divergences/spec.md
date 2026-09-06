# expected-divergences

## ADDED Requirements

### Requirement: bitshift-zero-fill

`bitshift` with a negative shift moves zeros into the vacated high bits
of the 32-bit pattern, per the reference's description, where other
interpreters sign-extend; the operator therefore treats the integer as
a bit pattern rather than as a signed value.

#### Scenario: Declared

- **GIVEN** the corpus file shifting a negative integer right
- **THEN** it carries `% divergence: bitshift-zero-fill`

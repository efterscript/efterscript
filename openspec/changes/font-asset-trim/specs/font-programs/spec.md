# font-programs

## ADDED Requirements

### Requirement: Subroutines are pruned in embedded subsets

An embedded Type 1 subset SHALL keep only the subroutines reachable from
its kept charstrings, transitively, plus the first four; every other
subroutine SHALL be replaced by a stub that returns, so subroutine
indices remain valid. The pruned program SHALL define the same glyph
outlines and advances as the unpruned one.

#### Scenario: Pagella subset shrinks

- **GIVEN** the Palatino scenario embedding two glyphs
- **THEN** the embedded `FontFile` is under 20 KB and the interpreter
  reading it back yields the same outlines and advances as before

#### Scenario: Hint replacement survives

- **GIVEN** a synthesised font whose glyph reaches a subroutine only
  through the hint-replacement other-subroutine
- **THEN** the pruned program keeps that subroutine and the glyph's
  outline is unchanged

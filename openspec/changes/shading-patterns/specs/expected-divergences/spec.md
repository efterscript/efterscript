## ADDED Requirements

### Requirement: shading-colour-conversion-limit

A shading whose `ColorSpace` is a CIE-based space that does not
collapse to a calibrated space SHALL raise `limitcheck`. The reference
paints it. Chosen because the shading's colours come from a function
the viewer evaluates, so converting them to L*a*b* would require
evaluating the function in the interpreter; no configuration restores
the reference's behaviour.

#### Scenario: Declared

- **GIVEN** the corpus file painting an axial shading in a
  non-collapsing CIE-based space
- **THEN** it carries `% divergence: shading-colour-conversion-limit`

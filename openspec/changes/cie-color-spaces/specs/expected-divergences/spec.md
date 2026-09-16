## ADDED Requirements

### Requirement: cie-rendering-path

A colour in a CIE-based space that does not collapse SHALL be written
as the L*a*b* value of the XYZ its dictionary defines, relative to the
dictionary's white point. The reference renders such colour through a
colour rendering dictionary, and its rasteriser renders the written
`Lab` space by a path of its own, so a saturated colour renders
differently on the two sides: a monitor-like blue (L* 31.8, a* 81,
b* −109) comes out with about 90/255 more red from the written `Lab`
than from the reference's own path, while greys, reds, and moderate
colours agree within 25/255. Chosen because the written value is the
exact conversion and the departure lies in the rendering; no
configuration restores the reference's rendering.

#### Scenario: Declared on the saturated painting files

- **GIVEN** the corpus files painting a monitor-like blue through a
  converting space (an image and an Indexed lookup)
- **THEN** each carries `% divergence: cie-rendering-path`

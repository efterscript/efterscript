## ADDED Requirements

### Requirement: Glyph metrics follow the CTM at the call

The operands of `setcachedevice`, `setcachedevice2`, and
`setcharwidth` SHALL be interpreted in the coordinate system in effect
when the operator executes, per PLRM3 §5.4 and the operator entries in
§8.2, and carried into glyph space — the system in effect when the
glyph procedure began: the width vector through the delta transform
of the intervening change, the bounding box through the full
transform with an axis-aligned envelope in glyph space. A procedure
that changes the CTM before declaring its metrics SHALL therefore
advance the current point and declare a box exactly as if the metrics
had been given in glyph space; a procedure that declares them first
SHALL be unaffected.

#### Scenario: A scale before setcachedevice

- **GIVEN** a Type 3 font with `FontMatrix [0.001 0 0 0.001 0 0]` whose `BuildChar` executes `0.5 0.5 scale` and then `1200 0 0 0 1200 1200 setcachedevice` before filling a triangle, shown at size 20 from (100,100)
- **THEN** each glyph advances the current point by 12, the captured glyph's width is 600 and its box `0 0 600 600` in glyph space, and the page's glyphs are spaced 12 apart

#### Scenario: A translated and scaled bitmap glyph

- **GIVEN** a Type 3 font whose `BuildChar` scales by a normalisation factor, declares its metrics, translates to an offset, scales to the bitmap's size, and paints with `imagemask` from a string data source (the shape of a driver's bitmap font)
- **THEN** the text line's glyphs advance by the declared widths through the normalisation, the glyph boxes enclose the painted bitmaps, and a checker accepts the resulting font

### Requirement: findfont enters resident faces in FontDirectory

`findfont` SHALL enter a font it obtains from the resident set, or by
substitution, in `FontDirectory` under the requested key, as
`definefont` would, per the `FontDirectory` and `findfont` entries of
PLRM3 §8.2; a later `findfont` of the key SHALL find it there, and a
font the program defined SHALL take precedence.

#### Scenario: A found face is listed

- **GIVEN** `FontDirectory length /Helvetica findfont pop FontDirectory /Helvetica known FontDirectory length`
- **THEN** the results are `0`, `true`, and a length greater than 0, and `FontDirectory { pop } forall` enumerates the key

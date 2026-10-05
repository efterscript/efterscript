## MODIFIED Requirements

### Requirement: Stroke adjustment and overprint in the state

`setstrokeadjust` and `currentstrokeadjust` SHALL keep a boolean in
the graphics state, default `false`, saved and restored with it, that
`initgraphics` does not reset and a glyph procedure starts with
`false`; `setoverprint` and `currentoverprint` SHALL keep a boolean in
the graphics state, default `false`, saved and restored with it. The IR
SHALL carry the overprint setting in effect where a paint occurs,
dumped as a state line only where it changes. The IR SHALL carry the
stroke adjustment setting in effect where a stroke occurs, stated
before the first stroke of each content (a page, a glyph procedure, a
pattern cell, a form body) whatever its value, and thereafter only
where it changes; the dump SHALL print it as a state line.

#### Scenario: Overprint is recorded

- **WHEN** `true setoverprint` precedes a fill in a Separation space and `false setoverprint` a second fill
- **THEN** the dump shows an overprint setting before the first fill and its reset before the second, and `currentoverprint` after a `gsave … grestore` round trip is unchanged

#### Scenario: The default is stated at the first stroke

- **WHEN** a page fills a rectangle and then strokes two lines without calling `setstrokeadjust`
- **THEN** the dump shows a stroke adjustment setting of `false` once, after the fill and before the first stroke, and none before the second

#### Scenario: A change is recorded and a restore brings it back

- **WHEN** a page strokes a line, then inside `gsave` calls `true setstrokeadjust` and strokes a second, then after `grestore` strokes a third
- **THEN** the dump shows `false` before the first stroke, `true` before the second, and `false` again before the third (the pair holds no clip, so the IR has no `Restore` that would bring the setting back), and `currentstrokeadjust` after the `grestore` answers `false`

#### Scenario: A glyph procedure states its own setting

- **WHEN** a Type 3 glyph procedure strokes a path and is shown on a page that has called `true setstrokeadjust` and stroked before
- **THEN** the glyph procedure's operations state a stroke adjustment setting of `false` before its stroke, and `currentstrokeadjust` after the `show` answers `true`

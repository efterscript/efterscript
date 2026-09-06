# oracle-testing

## MODIFIED Requirements

### Requirement: Comparison

For each corpus file the harness SHALL produce EfterScript's PDF and
the converter's PDF, render both through the profile's rasteriser to
raw PNM at the profile's resolution, and compare page count, media box
(within 0.5 units), and pixels: a pixel differs when any channel differs
by more than the profile's threshold, and a page fails when the
differing fraction exceeds the profile's limit. When the profile
provides a text extractor, extracted text SHALL be compared after
whitespace normalisation, except that a page whose EfterScript PDF
carries no Unicode mapping for its fonts SHALL report its text as not
comparable rather than as a failure, and a page whose rasters agree and
from which the reference extracted no text SHALL report a text
difference as a note (invisible text) rather than as a failure. The
harness SHALL also run the file through the reference interpreter and
compare standard output with EfterScript's after normalising
real-number formatting and trailing whitespace; when the profile gives
an `error_marker`, the reference's output SHALL be cut at the marker's
first occurrence and the presence of the marker recorded as the
reference ending in error. Each file SHALL be given the profile's
timeout.

#### Scenario: Identical pages pass

- **GIVEN** a corpus file whose two renderings differ in no pixel beyond
  the threshold
- **THEN** the verdict is pass

#### Scenario: Missing page fails

- **GIVEN** a corpus file for which the converter produces two pages and
  EfterScript one
- **THEN** the verdict is fail with the page counts in the report

#### Scenario: Output channel compared

- **GIVEN** a corpus file with `% expect-output:` lines
- **THEN** the report shows whether the reference interpreter's output
  equals EfterScript's after normalisation, independently of the raster
  verdict

#### Scenario: Text without a Unicode mapping

- **GIVEN** a page whose EfterScript fonts carry no ToUnicode
- **THEN** the report marks text as not comparable and the verdict
  depends on the other checks only

#### Scenario: Error marker

- **GIVEN** a profile with an `error_marker` and a declared-error corpus
  file
- **THEN** the output comparison covers only the text before the marker
  and the reference is recorded as having ended in error

#### Scenario: Invisible text

- **GIVEN** a page showing text outside the media box, with rasters that
  agree and no text extracted by the reference
- **THEN** the text difference is reported as a note and the verdict is
  pass

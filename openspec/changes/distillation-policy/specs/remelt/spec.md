# remelt

## ADDED Requirements

### Requirement: Embed-all

With `EmbedAllFonts` set, text in the standard fourteen faces SHALL be
written as embedded subset programs from the faces' outline assets,
named by the asset's font name with a subset tag, with widths from the
face's metrics; the two faces without an outline asset SHALL remain
unembedded and be reported.

#### Scenario: Helvetica embedded

- **GIVEN** Helvetica text with `EmbedAllFonts true`
- **THEN** the font dictionary is a TrueType font whose descriptor
  embeds a subset program, and the page's text extracts unchanged

### Requirement: Image downsampling

With downsampling enabled for an image class, an image whose effective
resolution on the page exceeds the class's target SHALL be reduced by
the largest integer factor that keeps it at or above the target, by
averaging or subsampling as set, for 8-bit gray, RGB, and CMYK samples
and 1-bit masks (subsampling only); other images SHALL be left as they
are and reported.

#### Scenario: A 300 dpi gray image at 72

- **GIVEN** a 300×300-sample gray image drawn one inch square with
  `DownsampleGrayImages true` and `GrayImageResolution 72`
- **THEN** the written image is 75×75 samples (factor 4, averaged) and
  the rendered page agrees with the original within tolerance

### Requirement: Compatibility header

The file header SHALL name the `CompatibilityLevel` version, 1.3 to
1.7, defaulting to 1.7; features written SHALL not change with it.

#### Scenario: Header version

- **GIVEN** `<< /CompatibilityLevel 1.4 >> setdistillerparams`
- **THEN** the output begins with the 1.4 header

# filters Specification

## Purpose
The `filter` operator and the standard filters: how a filtered file is
created over a source or target, how each filter decodes and encodes,
its parameters and end-of-data, how filters chain, and the `Filter`
resource category.

## Requirements

### Requirement: The filter operator

`filter` SHALL take a data source (a file, a string, or a
procedure), a filter name, and for some filters a parameter dictionary,
and return a file object. A decode filter's file SHALL read the named
filter's decoding of the source; an encode filter over a target file
SHALL write the filter's encoding through to the target. A filter's
source or target MAY itself be a filter, so filters chain. An unknown
filter name SHALL raise `undefined`; an unsuitable source SHALL raise
`typecheck`. Reading past the filter's end-of-data SHALL return
end-of-data, and `closefile` on a decode filter SHALL leave the source
positioned after the consumed bytes.

#### Scenario: A hexadecimal decode

- **GIVEN** `(4869207e>) /ASCIIHexDecode filter 100 string readstring`
- **THEN** the bytes read are `Hi ~` up to the `>` end-of-data and the
  boolean is `false`

#### Scenario: Chained filters

- **GIVEN** an ASCII85 wrapping of a Flate encoding of `(hello)`,
  read through `… /ASCII85Decode filter /FlateDecode filter`
- **THEN** the decoded bytes are `hello`

#### Scenario: Unknown filter

- **GIVEN** `(x) /NoSuchDecode filter`
- **THEN** the error is `undefined`

### Requirement: The standard decode filters

`ASCIIHexDecode`, `ASCII85Decode`, `RunLengthDecode`, `FlateDecode`,
`LZWDecode`, and `SubFileDecode` SHALL decode per their published
definitions, honouring `FlateDecode` and `LZWDecode` `Predictor`
parameters (PNG and TIFF predictors with `Colors`, `BitsPerComponent`,
`Columns`), `LZWDecode` `EarlyChange`, and `SubFileDecode` `EODCount`
and `EODString`. `DCTDecode` SHALL be recognised and its parameters
parsed, but its samples SHALL NOT be decoded; reading raw samples
through it SHALL raise `undefined`.

#### Scenario: Flate with a PNG predictor

- **GIVEN** a Flate stream of predictor-filtered rows with `<<
  /Predictor 12 /Columns 4 /Colors 1 /BitsPerComponent 8 >>`
- **THEN** the decoded rows match the original unpredicted rows

#### Scenario: LZW round-trips with a decode

- **GIVEN** data encoded with `LZWEncode` and read back through
  `LZWDecode`
- **THEN** the bytes match the original

#### Scenario: SubFileDecode ends at its string

- **GIVEN** `(ab*EOD*cd) /SubFileDecode` with `/EODString (*EOD*)` and
  `/EODCount 0` read to end
- **THEN** the bytes read are `ab`

### Requirement: The standard encode filters

`ASCIIHexEncode`, `ASCII85Encode`, `RunLengthEncode`, `FlateEncode`,
`LZWEncode`, and `NullEncode` SHALL encode to their filters' formats
when writing to a target file, and their output SHALL decode back to
the input through the matching decode filter.

#### Scenario: Encode then decode

- **GIVEN** `(the quick brown fox)` written through each encode filter
  to a string-backed target and read back through its decode filter
- **THEN** the recovered bytes equal the input for every filter

### Requirement: The Filter category

The `Filter` implicit resource category SHALL report the decode and
encode filter names as members: `resourcestatus` leaves `0 0 true` for
a member and `false` otherwise, `findresource` returns the key, and
`resourceforall` enumerates them.

#### Scenario: Filter is resident

- **GIVEN** `/FlateDecode /Filter resourcestatus` and `/NoSuch /Filter
  resourcestatus`
- **THEN** the first leaves `0 0 true` and the second `false`

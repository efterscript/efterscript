# Spec Delta

## Purpose

The image operators' dictionary forms as the interpreter accepts them:
which image types `image` and `imagemask` take, how their sample and mask
data are read, and which errors they raise.

## ADDED Requirements

### Requirement: Image types

`image` SHALL accept image dictionaries of types 1, 3, and 4 and raise
`rangecheck` for any other `ImageType`; `imagemask` SHALL accept type 1
only, as before. Whether masking applies depends on the type alone, not
on the language level a program detects.

#### Scenario: An unknown image type

- **WHEN** `image` is given a dictionary whose `ImageType` is 2
- **THEN** it raises `rangecheck` and paints nothing

#### Scenario: imagemask refuses a masked type

- **WHEN** `imagemask` is given a well-formed type 3 dictionary
- **THEN** it raises `rangecheck`

### Requirement: Explicit masks

A type 3 image (PLRM3 §4.10.6) SHALL paint the samples of its `DataDict`
only where its `MaskDict`'s decoded mask value is 0, leaving the page
unchanged where it is 1. The mask is read as its `InterleaveType`
prescribes (1 by sample, 2 by row, 3 from its own source); the image is
placed by the data dictionary's `ImageMatrix`.

#### Scenario: Interleaved by row, mask taller than the image

- **WHEN** a type 3 image with interleave type 2 has a 4×2 8-bit DeviceGray data dictionary and a 4×4 mask, its source holding blocks of two mask rows then one image row
- **THEN** the image paints only where the mask's decoded value is 0, and the program continues with the token after the data

#### Scenario: Interleaved by row, image taller than the mask

- **WHEN** a type 3 image with interleave type 2 has a 4×4 data dictionary and a 4×2 mask, its source holding blocks of one mask row then two image rows
- **THEN** each mask row governs the two image rows of its block

#### Scenario: Interleaved by sample

- **WHEN** a type 3 image with interleave type 1 has an 8-bit DeviceRGB data dictionary, each sample in its source being a mask byte followed by three colour bytes
- **THEN** samples whose mask byte is 0 are painted under a mask `Decode` of `[0 1]`, and a mask byte other than 0 or 255 counts as 255

#### Scenario: Separate sources

- **WHEN** a type 3 image with interleave type 3 has a 2×2 data dictionary with its own data source and an 8×8 mask with its own data source and an `ImageMatrix` scaled to the same square
- **THEN** the mask is applied at its own resolution over the image's square

#### Scenario: An indexed image with an inverted mask

- **WHEN** a type 3 image in an 8-bit Indexed space has a mask whose `Decode` is `[1 0]`, the data read through a `RunLengthDecode` filter on the current file
- **THEN** mask samples of 1 are painted and samples of 0 are left unchanged, and the program continues after the filter's end-of-data marker

### Requirement: Explicit mask consistency

A type 3 dictionary whose parts do not fit together SHALL raise
`typecheck`, including when a sub-dictionary is missing or not type 1,
data sources are present or absent against the interleave type, sizes,
ratios, or depths break the interleave type's rule, or the mask does not
cover the image's square. An interleave type other than 1, 2, or 3
SHALL raise `rangecheck`.

#### Scenario: Heights not in an integral ratio

- **WHEN** a type 3 image with interleave type 2 has an image height of 3 and a mask height of 2
- **THEN** it raises `typecheck` before reading any data

#### Scenario: A mask data source under interleave type 2

- **WHEN** the mask dictionary of an interleave type 2 image carries a `DataSource`
- **THEN** it raises `typecheck`

#### Scenario: A mask that does not overlay the image

- **WHEN** the mask's `ImageMatrix` maps it to a square offset from the image's
- **THEN** it raises `typecheck`

#### Scenario: Interleave type 4

- **WHEN** a type 3 dictionary has `InterleaveType 4`
- **THEN** it raises `rangecheck`

### Requirement: Colour key masks

A type 4 image (PLRM3 §4.10.6) SHALL leave the page unchanged wherever
every component of a sample, compared before decoding, falls within the
corresponding range of `MaskColor`; a `MaskColor` of n integers SHALL
mean n exact values. A `MaskColor` holding something other than n or 2n
integers SHALL raise `rangecheck`, and one holding a non-integer SHALL
raise `typecheck`.

#### Scenario: Exact colour

- **WHEN** a 4×1 8-bit DeviceRGB type 4 image whose `MaskColor` is `[255 255 255]` has two white samples
- **THEN** only the two other samples are painted

#### Scenario: Ranges compared before decoding

- **WHEN** a 4-bit DeviceGray type 4 image with `Decode [1 0]` has `MaskColor [0 3]`
- **THEN** samples with raw values 0 to 3 are masked, whatever their decoded values

#### Scenario: Wrong length

- **WHEN** a DeviceRGB type 4 image has a `MaskColor` of four integers
- **THEN** it raises `rangecheck`

### Requirement: Short masked data

When a masked image's data or mask ends early, the image SHALL be cut
to the rows both cover, as a type 1 image is cut to the rows its source
delivered. The mask SHALL stay aligned with the remaining rows.

#### Scenario: Data ends after half the rows

- **WHEN** an interleave type 3 image's data procedure returns an empty string after half of its rows while its mask is complete
- **THEN** the painted image is the upper half of the square with the matching half of the mask, and no error is raised

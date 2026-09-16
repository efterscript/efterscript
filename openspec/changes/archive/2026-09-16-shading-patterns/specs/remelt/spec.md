## ADDED Requirements

### Requirement: Shadings in the PDF

A shading resource SHALL be written as a shading dictionary (types 1
to 3) or a shading stream (types 4 to 7 with the packed data) of ISO
32000-1 §8.7.4 with its entries, its function written as a type 0
stream, a type 2 dictionary, or a type 3 dictionary of §7.10, and
named in the page's `Shading` resources; a shade operation SHALL
become `sh` inside a saved state under the matrix; a shading pattern
SHALL be written as a `/PatternType 2` pattern object with its
`Shading` and `Matrix`, selected like a tiling pattern.

#### Scenario: An axial shading object

- **WHEN** a page paints an axial shading
- **THEN** the page's resources name a shading with `/ShadingType 2`, `/Coords`, `/Extend`, and a `/FunctionType 2` function, the content stream paints it with `sh` under the matrix, and a checker accepts the file

#### Scenario: A mesh stream

- **WHEN** a page paints a type 4 mesh given as an array
- **THEN** the shading is a stream with `/ShadingType 4`, `/BitsPerCoordinate`, `/BitsPerComponent`, `/BitsPerFlag`, and `/Decode`, whose data decodes to the vertices given

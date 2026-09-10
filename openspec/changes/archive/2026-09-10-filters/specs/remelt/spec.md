# remelt

## ADDED Requirements

### Requirement: DCT images pass through

An image whose samples reached the interpreter already DCT-encoded (its
`image` dictionary's data source was a `DCTDecode` filter) SHALL be
written to the PDF as an image XObject with the `DCTDecode` filter and
its encoded bytes unchanged, so the viewer decodes the JPEG; images
with raw samples SHALL continue to be written through the Flate
container.

#### Scenario: A JPEG image survives

- **GIVEN** a program that draws an `image` whose data source is
  `currentfile /DCTDecode filter` over a small baseline JPEG
- **THEN** the page's image XObject has `/Filter /DCTDecode`, its stream
  is the original JPEG bytes, and a checker accepts the file

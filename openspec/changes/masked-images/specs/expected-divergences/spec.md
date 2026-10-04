## ADDED Requirements

### Requirement: masked-dct-interleaved

A type 3 image whose data source is a `DCTDecode` filter and whose
interleave type is 1 or 2 SHALL raise `limitcheck` without painting.
The reference decodes the stream and separates the mask from the
colour samples. Chosen because the interpreter carries JPEG data to the
PDF undecoded and has no decoder to split the stream with; a separate
mask source (interleave type 3) keeps the passthrough. No configuration
restores the reference behaviour.

#### Scenario: Declared

- **GIVEN** the corpus file whose interleave type 2 image reads its data through `DCTDecode`
- **THEN** it raises `limitcheck` and carries `% divergence: masked-dct-interleaved`

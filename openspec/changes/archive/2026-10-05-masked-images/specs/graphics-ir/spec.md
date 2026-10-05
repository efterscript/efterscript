## ADDED Requirements

### Requirement: Images carry their masks

An image resource SHALL carry either a stencil mask (its own width,
height, one-bit samples, decode polarity, and interpolation flag,
covering the image's unit square) or a colour key (one range of raw
sample values per component), or neither. The dump's `img` line SHALL
print the mask's size, polarity, and byte count, or the key's ranges.
Images without a mask SHALL dump exactly as before.

#### Scenario: A stencil mask in the dump

- **WHEN** a page paints a type 3 image with a 4×4 mask under a 4×2 image
- **THEN** the dump's image resource line names the image's size and space and adds the mask's 4×4 size, its decode, and its byte count

#### Scenario: A colour key in the dump

- **WHEN** a page paints a type 4 image with `MaskColor [255 255 255]`
- **THEN** the dump's image resource line adds the key `[255 255 255 255 255 255]`

#### Scenario: Unmasked images are unchanged

- **WHEN** the existing image corpus is dumped
- **THEN** every golden is byte-identical

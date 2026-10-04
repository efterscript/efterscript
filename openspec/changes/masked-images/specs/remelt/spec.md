## MODIFIED Requirements

### Requirement: Images become image XObjects

Each image in a page's resources SHALL become an image XObject with its
width, height, bits per component, colour space (or the image-mask flag
for a mask), decode array when it differs from the default, and
interpolation flag, with its sample data in a Flate container; the paint
SHALL be written inside a save/restore that concatenates the image's matrix
and invokes the XObject. An image with a stencil mask SHALL name, in
`/Mask`, an image XObject of its own carrying the mask as an image mask;
an image with a colour key SHALL carry the key as the `/Mask` array.

#### Scenario: A 2×2 gray image

- **GIVEN** a 2×2 8-bit DeviceGray image drawn at `100 100 translate 50 50
  scale`
- **THEN** the page has one image XObject with width 2, height 2, 8 bits,
  DeviceGray, whose decoded data is the four sample bytes, and the content
  stream is `q 50 0 0 50 100 100 cm /Im0 Do Q`

#### Scenario: An image mask paints the current colour

- **GIVEN** an `imagemask` in a Separation colour
- **THEN** the XObject is flagged as an image mask with no colour space, and
  the Separation is selected in the content stream before the mask is drawn

#### Scenario: An explicit mask

- **GIVEN** a type 3 image with a 4×4 mask whose `Decode` is `[1 0]`
- **THEN** the image XObject's `/Mask` refers to a second image XObject with
  `/ImageMask true`, width 4, height 4, `/Decode [1 0]`, which the page's
  `XObject` resources do not list, and the content stream paints only the
  base image

#### Scenario: A colour key

- **GIVEN** a DeviceRGB type 4 image with `MaskColor [250 255 250 255 250 255]`
- **THEN** the image XObject carries `/Mask [250 255 250 255 250 255]`

#### Scenario: A colour key in a converted space

- **GIVEN** a type 4 image in a CIE-based space that is converted to Lab
- **THEN** the image XObject carries a stencil mask computed from the raw
  samples instead of a key, and the rendered page masks the same samples

### Requirement: Image downsampling

With downsampling enabled for an image class, an image whose effective
resolution on the page exceeds the class's target SHALL be reduced by
the largest integer factor that keeps it at or above the target, by
averaging or subsampling as set, for 8-bit gray, RGB, and CMYK samples
and 1-bit masks (subsampling only); other images SHALL be left as they
are and reported. A masked image's base SHALL be reduced by its own
class while its stencil mask keeps its resolution. A colour key on a
reduced image SHALL first become a stencil mask computed from the
unreduced samples.

#### Scenario: A 300 dpi gray image at 72

- **GIVEN** a 300×300-sample gray image drawn one inch square with
  `DownsampleGrayImages true` and `GrayImageResolution 72`
- **THEN** the written image is 75×75 samples (factor 4, averaged) and
  the rendered page agrees with the original within tolerance

#### Scenario: A colour-keyed image averaged

- **GIVEN** a 300×300-sample DeviceRGB type 4 image drawn one inch square
  with `DownsampleColorImages true` and `ColorImageResolution 72`
- **THEN** the written image is 75×75 samples, its `/Mask` is a 300×300
  stencil mask, and no averaged sample is masked by the key

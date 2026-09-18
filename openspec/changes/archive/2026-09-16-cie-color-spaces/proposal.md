# Change: CIE-based colour spaces

## Why

The interpreter carries device, Separation, DeviceN, Indexed, and now
Pattern colour, but `setcolorspace` refuses the four CIE-based
families (`CIEBasedA`, `CIEBasedABC`, `CIEBasedDEF`, `CIEBasedDEFG`)
and the colour-rendering operators are undefined. Calibrated colour is
what the LaserWriter-8-era colour path emits when a Macintosh prints
with colour matching on, and it appears in any job produced with a
colour-managed workflow; such a job stops at its first
`setcolorspace`. PDF has exact device-independent counterparts,
`CalGray`, `CalRGB`, and `Lab`, so a distillation engine preserves calibrated
colour instead of rendering it through a device profile. The decision
that is hard to retrofit is where the mapping happens: a CIE space is
defined by PostScript procedures that only the VM can run, so the
conversion from a job's components to a PDF space must be made on the
VM side of the boundary, and the boundary must carry the three PDF
spaces as first-class resources. Fixing that now, on top of the
resource-carrying colour model the pattern change established, keeps
the boundary free of procedures and leaves shading patterns and any
later profile-based colour extending a finished model.

## What Changes

- **The CIE-based families** (`ps-vm`): `setcolorspace` accepts
  `[/CIEBasedA dict]`, `[/CIEBasedABC dict]`, `[/CIEBasedDEF dict]`,
  and `[/CIEBasedDEFG dict]`, validating the dictionaries of PLRM3
  §4.8.3 (ranges, decode procedures, matrices, tables, `WhitePoint`,
  `BlackPoint`), with the initial colour and the range clamping the
  manual specifies; `currentcolorspace`/`currentcolor` report the
  space and the program's components; `currentgray`/`currentrgbcolor`/
  `currentcmykcolor`/`currenthsbcolor` answer the initial device value
  as PLRM3 §4.8.3 states.
- **Mapping to PDF calibrated spaces** (`ps-vm`): a space whose
  transformation collapses to a single gamma-and-matrix stage becomes
  `CalGray` (from `CIEBasedA`) or `CalRGB` (from `CIEBasedABC`) with the
  same white point, black point, gammas, and matrix, its components
  unchanged; every other CIE-based space becomes `Lab` with the space's
  white and black points, each colour converted through the space's
  own decode procedures, matrices, and lookup tables to XYZ and then to
  L*a*b*. The decode procedures run in the VM when a colour is set;
  image samples in a converted space are converted per sample with
  per-component caching, and images in a collapsed space pass through.
- **Colour in the IR and the PDF** (`ps-graphics`, `remelt`): `CalGray`,
  `CalRGB`, and `Lab` spaces as page resources with their dictionaries,
  dumped and written as `/CSn` colour-space arrays; colours and images
  in them written unconverted.
- **Rendering operators and categories** (`ps-vm`): `setcolorrendering`,
  `currentcolorrendering`, and `findcolorrendering` accept and record a
  colour rendering dictionary without applying it (distillation keeps
  colour device-independent); the `ColorRendering` regular category
  with a minimal default instance; the `ColorSpace` regular category;
  `ColorSpaceFamily` lists the four families.
- Out of scope, with triggers: a profile-based colour space
  (`ICCBased`) for the PostScript language — the vault's manuals define
  profiles only as a source of rendering dictionaries, so it waits for
  a job that uses it and for the defining document; `UseCIEColor`
  remapping of device colours through `Default*` resources (the key is
  recorded, not honoured — when a job depends on it); `CalCMYK`;
  applying a rendering dictionary; shading patterns.

## Capabilities

### New Capabilities
- `cie-color`: the four CIE-based families, their validation and
  initial colour, the mapping policy to the PDF calibrated spaces, the
  conversion, the rendering operators, and the categories.

### Modified Capabilities
- `graphics-ir`: ADDED requirement for calibrated colour-space
  resources and their dump.
- `remelt`: ADDED requirement for writing `CalGray`, `CalRGB`, and `Lab`
  spaces and colours in them.
- `expected-divergences`: ADDED requirement
  `cie-rendering-path` (converted colour is the exact L*a*b*, which the
  reference's rendering path renders differently for saturated blues).

## Impact

- Code: `crates/ps-vm` (`ops/cie.rs`: parsing, the collapse test, the
  evaluation frame, XYZ→Lab, image sample conversion; `graphics.rs`:
  three `SpaceSpec` variants; `ops/graphics.rs`: the current-colour
  getters; `ops/resource.rs`: two categories and the family names;
  `ops/pagedevice.rs`: `UseCIEColor` recorded), `crates/ps-graphics`
  (dump lines), `crates/remelt` (colour-space arrays, image
  `/Decode` for converted samples), corpus under `corpus/unit/cie/`
  with goldens; `psgen` untouched.
- No new dependencies.
- Depends on `patterns-and-forms` (resource-carrying colour, regular
  categories) and `graphics-ir`, archived.

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The masks of type 3 and type 4 images (PLRM3 §4.10.6). A colour key
//! is a list of raw-sample ranges. A type 3 image's mask is collected
//! with its samples (interleave types 1 and 2) or from its own source
//! ahead of them (type 3), then fitted to the image: cut to the rows
//! both parts delivered, reduced to one bit per sample with its polarity
//! as a flag, and turned to run the way the image's rows and columns
//! run.

use crate::error::VmError;
use crate::graphics::{ImageMask, ImageSpec};
use crate::object::Object;

/// A type 3 image's mask while its data is being read.
#[derive(Clone, Debug)]
pub(crate) struct MaskAcquisition {
    width: u32,
    height: u32,
    /// The mask value raw bits 0 and 1 decode to, each end of the mask's
    /// `Decode` rounded to 0 or 1.
    decoded: [bool; 2],
    interpolate: bool,
    reversed: Reversal,
    pub(super) arrival: Arrival,
}

/// Which axes of the mask's square run opposite to the image's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Reversal {
    rows: bool,
    columns: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Arrival {
    /// Interleave type 1: each sample of the image's source starts with
    /// its mask component, at the image's depth.
    BySample,
    /// Interleave type 2: the image's source holds blocks of `mask_rows`
    /// one-bit mask rows followed by `image_rows` image rows.
    ByRow { mask_rows: usize, image_rows: usize },
    /// Interleave type 3: the mask comes from its own source, read
    /// completely before the image's.
    Separate(Box<Separate>),
}

#[derive(Clone, Debug)]
pub(crate) struct Separate {
    pub(super) source: Object,
    pub(super) data_source: Object,
    pub(super) needed: usize,
    pub(super) bytes: Vec<u8>,
    /// The mask's source has delivered all it will.
    pub(super) done: bool,
    /// The image's string or file source has been read.
    pub(super) data_read: bool,
}

impl MaskAcquisition {
    /// Checks the mask dictionary `mask` against the data dictionary
    /// `data` for `interleave` (1 to 3); every inconsistency is
    /// `typecheck`. `mask_source` is the mask dictionary's own source.
    pub(super) fn new(
        data: &ImageSpec,
        mask: &ImageSpec,
        interleave: i32,
        mask_source: Option<Object>,
        data_source: Object,
    ) -> Result<Self, VmError> {
        let arrival = match (interleave, mask_source) {
            (1, None)
                if mask.width == data.width
                    && mask.height == data.height
                    && mask.bits_per_component == data.bits_per_component =>
            {
                Arrival::BySample
            }
            (2, None) if mask.bits_per_component == 1 => {
                let (mask_rows, image_rows) = block_shape(mask.height, data.height)?;
                Arrival::ByRow {
                    mask_rows,
                    image_rows,
                }
            }
            (3, Some(source)) if mask.bits_per_component == 1 => {
                let needed = mask.data_len().ok_or(VmError::LimitCheck)?;
                Arrival::Separate(Box::new(Separate {
                    source,
                    data_source,
                    needed,
                    bytes: Vec::new(),
                    done: needed == 0,
                    data_read: false,
                }))
            }
            (1..=3, _) => return Err(VmError::TypeCheck),
            _ => return Err(VmError::RangeCheck),
        };
        Ok(MaskAcquisition {
            width: mask.width,
            height: mask.height,
            decoded: [mask.decode[0] >= 0.5, mask.decode[1] >= 0.5],
            interpolate: mask.interpolate,
            reversed: alignment(data, mask)?,
            arrival,
        })
    }

    fn row_bytes(&self) -> usize {
        (self.width as usize).div_ceil(8)
    }

    /// The bytes the image's own source must deliver for `spec`.
    pub(super) fn needed(&self, spec: &ImageSpec) -> Option<usize> {
        match &self.arrival {
            Arrival::BySample => combined_row(spec)?.checked_mul(spec.height as usize),
            Arrival::ByRow {
                mask_rows,
                image_rows,
            } => {
                let block = mask_rows
                    .checked_mul(self.row_bytes())?
                    .checked_add(image_rows.checked_mul(spec.row_bytes()?)?)?;
                block.checked_mul(spec.height as usize / image_rows)
            }
            Arrival::Separate(_) => spec.data_len(),
        }
    }

    /// The separate mask stage, while it still wants bytes.
    pub(super) fn pending(&mut self) -> Option<&mut Separate> {
        match &mut self.arrival {
            Arrival::Separate(separate) if !separate.done => Some(separate),
            _ => None,
        }
    }

    pub(super) fn separate(&self) -> Option<&Separate> {
        match &self.arrival {
            Arrival::Separate(separate) => Some(separate),
            _ => None,
        }
    }

    pub(super) fn separate_mut(&mut self) -> Option<&mut Separate> {
        match &mut self.arrival {
            Arrival::Separate(separate) => Some(separate),
            _ => None,
        }
    }

    /// Takes the collected `data` apart, cuts the image (`spec`'s height)
    /// and the mask to the rows both delivered, and attaches the mask to
    /// `spec`; returns the image's samples.
    pub(super) fn resolve(self, spec: &mut ImageSpec, data: Vec<u8>) -> Result<Vec<u8>, VmError> {
        let row = spec.row_bytes().ok_or(VmError::LimitCheck)?;
        let mask_row = self.row_bytes();
        let (mut image, raw) = match &self.arrival {
            Arrival::BySample => self.split_samples(spec, &data)?,
            Arrival::ByRow {
                mask_rows,
                image_rows,
            } => split_blocks(&data, mask_rows * mask_row, image_rows * row),
            Arrival::Separate(separate) => (data, separate.bytes.clone()),
        };
        let height = spec.height as usize;
        let mask_height = self.height as usize;
        let mask_read = rows_in(raw.len(), mask_row, mask_height);
        let (image_rows, mask_rows) = if spec.encoded.is_some() {
            // Encoded samples cannot be cut; rows the mask lacks are
            // kept off the page instead.
            (height, mask_height)
        } else {
            let image_read = rows_in(image.len(), row, height);
            covered(
                image_read,
                height,
                mask_read,
                mask_height,
                self.reversed.rows,
            )
        };
        if spec.encoded.is_none() {
            image.truncate(image_rows * row);
            spec.height = u32::try_from(image_rows).map_err(|_| VmError::LimitCheck)?;
        }
        spec.mask = Some(self.stencil(&raw, mask_read, mask_rows)?);
        Ok(image)
    }

    /// Interleave type 1: the image's samples and one mask bit per
    /// sample, 0 exactly when the mask component's bits are all 0, from
    /// the whole rows of `data`.
    fn split_samples(&self, spec: &ImageSpec, data: &[u8]) -> Result<(Vec<u8>, Vec<u8>), VmError> {
        let width = spec.width as usize;
        let components = spec.components();
        let bits = usize::from(spec.bits_per_component);
        let combined = combined_row(spec).ok_or(VmError::LimitCheck)?;
        let row = spec.row_bytes().ok_or(VmError::LimitCheck)?;
        let mask_row = self.row_bytes();
        let rows = rows_in(data.len(), combined, spec.height as usize);
        let mut image = vec![0u8; rows * row];
        let mut mask = vec![0u8; rows * mask_row];
        for y in 0..rows {
            let source = &data[y * combined..(y + 1) * combined];
            let target = &mut image[y * row..(y + 1) * row];
            for x in 0..width {
                let at = x * (components + 1) * bits;
                if read_bits(source, at, bits) != 0 {
                    mask[y * mask_row + x / 8] |= 0x80 >> (x % 8);
                }
                for k in 0..components {
                    let value = read_bits(source, at + (k + 1) * bits, bits);
                    write_bits(target, (x * components + k) * bits, bits, value);
                }
            }
        }
        Ok((image, mask))
    }

    /// The mask's first `keep` rows in the image's orientation, from the
    /// `read` rows of raw bits in `raw`, as a stencil: the polarity
    /// becomes the flag, or, when both ends of `Decode` agree, every bit
    /// takes the one value under the default polarity. Rows the source
    /// never delivered keep the image off the page.
    fn stencil(&self, raw: &[u8], read: usize, keep: usize) -> Result<ImageMask, VmError> {
        let width = self.width as usize;
        let row = self.row_bytes();
        let constant = (self.decoded[0] == self.decoded[1]).then_some(self.decoded[0]);
        let inverted = constant.is_none() && self.decoded[0];
        let off = u16::from(!inverted);
        let mut rows: Vec<Vec<u8>> = (0..self.height as usize)
            .map(|y| {
                let mut out = vec![0u8; row];
                for x in 0..width {
                    let bit = match (y < read, constant) {
                        (false, _) => off,
                        (true, Some(value)) => u16::from(value),
                        (true, None) => read_bits(&raw[y * row..], x, 1),
                    };
                    let column = if self.reversed.columns {
                        width - 1 - x
                    } else {
                        x
                    };
                    write_bits(&mut out, column, 1, bit);
                }
                out
            })
            .collect();
        if self.reversed.rows {
            rows.reverse();
        }
        rows.truncate(keep);
        Ok(ImageMask::Stencil {
            width: self.width,
            height: u32::try_from(keep).map_err(|_| VmError::LimitCheck)?,
            decode_inverted: inverted,
            interpolate: self.interpolate,
            data: rows.concat(),
        })
    }
}

/// Interleave type 2's block: mask rows and image rows per block, one
/// height an integral multiple of the other.
fn block_shape(mask_height: u32, image_height: u32) -> Result<(usize, usize), VmError> {
    match (mask_height, image_height) {
        (0, 0) => Ok((1, 1)),
        (0, _) | (_, 0) => Err(VmError::TypeCheck),
        (m, h) if m % h == 0 => Ok(((m / h) as usize, 1)),
        (m, h) if h % m == 0 => Ok((1, (h / m) as usize)),
        _ => Err(VmError::TypeCheck),
    }
}

/// Bytes per row of interleave type 1's source: the mask component and
/// the colour components of each sample, at the image's depth.
fn combined_row(spec: &ImageSpec) -> Option<usize> {
    let bits = (spec.width as usize)
        .checked_mul(spec.components() + 1)?
        .checked_mul(usize::from(spec.bits_per_component))?;
    Some(bits.div_ceil(8))
}

/// Whole rows of `row` bytes in `len` bytes, at most `total`.
fn rows_in(len: usize, row: usize, total: usize) -> usize {
    len.checked_div(row).map_or(total, |rows| rows.min(total))
}

/// The image rows and mask rows kept when `image_read` of `height` image
/// rows and `mask_read` of `mask_height` mask rows arrived: the image's
/// leading rows the mask also covers, and the mask rows over them
/// (rounded up). A mask whose rows run opposite to the image's covers
/// its leading rows only when it is complete.
fn covered(
    image_read: usize,
    height: usize,
    mask_read: usize,
    mask_height: usize,
    reversed: bool,
) -> (usize, usize) {
    if height == 0 || mask_height == 0 {
        return (image_read, mask_read);
    }
    let (h, mh) = (height as u64, mask_height as u64);
    let image_rows = if mask_read >= mask_height {
        image_read
    } else if reversed {
        0
    } else {
        image_read.min((mask_read as u64 * h / mh) as usize)
    };
    (image_rows, (image_rows as u64 * mh).div_ceil(h) as usize)
}

/// Interleave type 2's blocks of `data` taken apart into their mask and
/// image rows; a block cut short is dropped.
fn split_blocks(data: &[u8], mask_block: usize, image_block: usize) -> (Vec<u8>, Vec<u8>) {
    let block = mask_block + image_block;
    let blocks = data.len().checked_div(block).unwrap_or(0);
    let mut image = Vec::with_capacity(blocks * image_block);
    let mut mask = Vec::with_capacity(blocks * mask_block);
    for chunk in data.chunks_exact(block.max(1)).take(blocks) {
        mask.extend_from_slice(&chunk[..mask_block]);
        image.extend_from_slice(&chunk[mask_block..]);
    }
    (image, mask)
}

/// Where each dictionary's square lands in user space: the images of
/// image space's origin and of the ends of its first row and column,
/// `None` for a singular matrix.
fn square(spec: &ImageSpec) -> Option<[[f64; 2]; 3]> {
    let m = spec.matrix.inverse64()?;
    let at = |x: f64, y: f64| [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
    let (w, h) = (f64::from(spec.width), f64::from(spec.height));
    Some([at(0.0, 0.0), at(w, 0.0), at(0.0, h)])
}

/// How the mask's square sits on the image's: the same, or with rows,
/// columns, or both reversed, to 1e-4 of the square's extent; anything
/// else is `typecheck`. An image whose square is degenerate takes any
/// mask as it is.
fn alignment(data: &ImageSpec, mask: &ImageSpec) -> Result<Reversal, VmError> {
    let Some([o, x, y]) = square(data) else {
        return Ok(Reversal::default());
    };
    let ex = [x[0] - o[0], x[1] - o[1]];
    let ey = [y[0] - o[0], y[1] - o[1]];
    let extent = ex[0].hypot(ex[1]).max(ey[0].hypot(ey[1]));
    if extent == 0.0 || !extent.is_finite() {
        return Ok(Reversal::default());
    }
    let corners = square(mask).ok_or(VmError::TypeCheck)?;
    let tolerance = 1e-4 * extent;
    let at = |s: f64, t: f64| [o[0] + s * ex[0] + t * ey[0], o[1] + s * ex[1] + t * ey[1]];
    for (columns, rows) in [(false, false), (false, true), (true, false), (true, true)] {
        let s = if columns { 1.0 } else { 0.0 };
        let t = if rows { 1.0 } else { 0.0 };
        let expected = [at(s, t), at(1.0 - s, t), at(s, 1.0 - t)];
        let close = expected
            .iter()
            .zip(&corners)
            .all(|(e, p)| (e[0] - p[0]).abs() <= tolerance && (e[1] - p[1]).abs() <= tolerance);
        if close {
            return Ok(Reversal { rows, columns });
        }
    }
    Err(VmError::TypeCheck)
}

/// A `MaskColor` of `values` as one range per component for an image of
/// `components` components at `bits` per component: `n` values are
/// exact colours and `2n` are ranges, any other count `rangecheck`.
/// Values are clamped to what a sample can hold.
pub(super) fn key_ranges(
    values: &[i32],
    components: usize,
    bits: u8,
) -> Result<Vec<(u16, u16)>, VmError> {
    let max = (1i32 << bits) - 1;
    let clamp = |v: i32| v.clamp(0, max) as u16;
    if values.len() == components {
        Ok(values.iter().map(|&v| (clamp(v), clamp(v))).collect())
    } else if values.len() == 2 * components {
        let (pairs, _) = values.as_chunks::<2>();
        Ok(pairs
            .iter()
            .map(|&[lo, hi]| (clamp(lo), clamp(hi)))
            .collect())
    } else {
        Err(VmError::RangeCheck)
    }
}

/// The `bits`-wide value `at` bits into `row`, most significant first.
fn read_bits(row: &[u8], at: usize, bits: usize) -> u16 {
    (at..at + bits).fold(0, |value, bit| {
        let byte = row.get(bit / 8).copied().unwrap_or(0);
        (value << 1) | u16::from((byte >> (7 - bit % 8)) & 1)
    })
}

/// Sets the `bits`-wide field `at` bits into `row`, which is zero there.
fn write_bits(row: &mut [u8], at: usize, bits: usize, value: u16) {
    for k in 0..bits {
        if (value >> (bits - 1 - k)) & 1 == 1 {
            let bit = at + k;
            row[bit / 8] |= 0x80 >> (bit % 8);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::{Matrix, SpaceSpec};

    fn data(width: u32, height: u32, bits: u8, space: SpaceSpec) -> ImageSpec {
        let components = space.components();
        ImageSpec {
            width,
            height,
            bits_per_component: bits,
            color_space: Some(space),
            decode: [0.0, 1.0].repeat(components),
            matrix: Matrix([width as f32, 0.0, 0.0, -(height as f32), 0.0, height as f32]),
            interpolate: false,
            is_mask: false,
            encoded: None,
            mask: None,
        }
    }

    fn mask(width: u32, height: u32, bits: u8) -> ImageSpec {
        ImageSpec {
            color_space: None,
            decode: vec![0.0, 1.0],
            ..data(width, height, bits, SpaceSpec::DeviceGray)
        }
    }

    fn stencil(spec: &ImageSpec) -> (u32, u32, bool, Vec<u8>) {
        match &spec.mask {
            Some(ImageMask::Stencil {
                width,
                height,
                decode_inverted,
                data,
                ..
            }) => (*width, *height, *decode_inverted, data.clone()),
            other => panic!("not a stencil: {other:?}"),
        }
    }

    const SOURCE: Object = Object::null();

    #[test]
    fn interleave_rules_are_checked() {
        let gray = data(4, 2, 8, SpaceSpec::DeviceGray);
        let new = |m: &ImageSpec, interleave, source| {
            MaskAcquisition::new(&gray, m, interleave, source, SOURCE).map(|_| ())
        };
        let by_sample = ImageSpec {
            matrix: gray.matrix,
            ..mask(4, 2, 8)
        };
        assert_eq!(new(&by_sample, 1, None), Ok(()));
        assert_eq!(new(&by_sample, 1, Some(SOURCE)), Err(VmError::TypeCheck));
        assert_eq!(new(&mask(4, 2, 1), 1, None), Err(VmError::TypeCheck));
        assert_eq!(new(&mask(4, 4, 8), 1, None), Err(VmError::TypeCheck));
        assert_eq!(new(&mask(4, 4, 1), 2, None), Ok(()));
        assert_eq!(new(&mask(8, 1, 1), 2, None), Ok(()));
        assert_eq!(new(&mask(4, 3, 1), 2, None), Err(VmError::TypeCheck));
        assert_eq!(new(&mask(4, 4, 8), 2, None), Err(VmError::TypeCheck));
        assert_eq!(
            new(&mask(4, 4, 1), 2, Some(SOURCE)),
            Err(VmError::TypeCheck)
        );
        assert_eq!(new(&mask(4, 4, 1), 3, None), Err(VmError::TypeCheck));
        assert_eq!(new(&mask(4, 4, 1), 3, Some(SOURCE)), Ok(()));
        assert_eq!(
            new(&mask(4, 4, 1), 4, Some(SOURCE)),
            Err(VmError::RangeCheck)
        );
        assert_eq!(block_shape(6, 2), Ok((3, 1)));
        assert_eq!(block_shape(2, 6), Ok((1, 3)));
        assert_eq!(block_shape(0, 0), Ok((1, 1)));
        assert_eq!(block_shape(0, 2), Err(VmError::TypeCheck));
    }

    #[test]
    fn the_mask_must_overlay_the_image_or_mirror_it() {
        let gray = data(2, 2, 8, SpaceSpec::DeviceGray);
        let with = |a: [f32; 6]| ImageSpec {
            matrix: Matrix(a),
            ..mask(8, 8, 1)
        };
        let check = |m: ImageSpec| alignment(&gray, &m);
        assert_eq!(check(mask(8, 8, 1)), Ok(Reversal::default()));
        // Rows running upwards where the image's run downwards.
        assert_eq!(
            check(with([8.0, 0.0, 0.0, 8.0, 0.0, 0.0])),
            Ok(Reversal {
                rows: true,
                columns: false
            })
        );
        assert_eq!(
            check(with([-8.0, 0.0, 0.0, -8.0, 8.0, 8.0])),
            Ok(Reversal {
                rows: false,
                columns: true
            })
        );
        assert_eq!(
            check(with([-8.0, 0.0, 0.0, 8.0, 8.0, 0.0])),
            Ok(Reversal {
                rows: true,
                columns: true
            })
        );
        // Within the tolerance, and beyond it.
        assert!(check(with([8.0, 0.0, 0.0, -8.0, 0.0004, 8.0])).is_ok());
        assert_eq!(
            check(with([8.0, 0.0, 0.0, -8.0, 4.0, 8.0])),
            Err(VmError::TypeCheck)
        );
        // A transposed mask is no reversal.
        assert_eq!(
            check(with([0.0, -8.0, 8.0, 0.0, 0.0, 8.0])),
            Err(VmError::TypeCheck)
        );
        assert_eq!(
            check(with([0.0, 0.0, 0.0, 0.0, 0.0, 0.0])),
            Err(VmError::TypeCheck)
        );
    }

    #[test]
    fn interleave_by_sample_takes_one_bit_from_each_mask_component() {
        let rgb = data(3, 1, 8, SpaceSpec::DeviceRGB);
        let m = ImageSpec {
            matrix: rgb.matrix,
            ..mask(3, 1, 8)
        };
        let acquisition = MaskAcquisition::new(&rgb, &m, 1, None, SOURCE).unwrap();
        assert_eq!(acquisition.needed(&rgb), Some(12));
        let mut spec = rgb.clone();
        let source = [0, 1, 2, 3, 255, 4, 5, 6, 7, 8, 9, 10];
        let image = acquisition.resolve(&mut spec, source.to_vec()).unwrap();
        assert_eq!(image, [1, 2, 3, 4, 5, 6, 8, 9, 10]);
        assert_eq!(stencil(&spec), (3, 1, false, vec![0b0110_0000]));

        // Four-bit gray: a mask nibble and a sample nibble per sample.
        let gray = data(3, 2, 4, SpaceSpec::DeviceGray);
        let m = ImageSpec {
            matrix: gray.matrix,
            ..mask(3, 2, 4)
        };
        let acquisition = MaskAcquisition::new(&gray, &m, 1, None, SOURCE).unwrap();
        assert_eq!(acquisition.needed(&gray), Some(6));
        let mut spec = gray.clone();
        // The second row is short: the image is cut to the first.
        let image = acquisition
            .resolve(&mut spec, vec![0x0A, 0xFB, 0x1C, 0xF0])
            .unwrap();
        assert_eq!(image, [0xAB, 0xC0]);
        assert_eq!(spec.height, 1);
        assert_eq!(stencil(&spec), (3, 1, false, vec![0b0110_0000]));
    }

    #[test]
    fn interleave_by_row_in_both_height_ratios() {
        // Two mask rows, then one image row of four bytes.
        let gray = data(4, 2, 8, SpaceSpec::DeviceGray);
        let acquisition = MaskAcquisition::new(&gray, &mask(4, 4, 1), 2, None, SOURCE).unwrap();
        assert_eq!(acquisition.needed(&gray), Some(12));
        let mut spec = gray.clone();
        let source = [
            0x10, 0x20, 1, 2, 3, 4, //
            0x30, 0x40, 5, 6, 7, 8,
        ];
        let image = acquisition.resolve(&mut spec, source.to_vec()).unwrap();
        assert_eq!(image, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(stencil(&spec), (4, 4, false, vec![0x10, 0x20, 0x30, 0x40]));

        // One mask row, then two image rows.
        let gray = data(2, 4, 8, SpaceSpec::DeviceGray);
        let acquisition = MaskAcquisition::new(&gray, &mask(2, 2, 1), 2, None, SOURCE).unwrap();
        assert_eq!(acquisition.needed(&gray), Some(10));
        let source = [0x80, 1, 2, 3, 4, 0x40, 5, 6, 7, 8];
        let mut spec = gray.clone();
        let image = acquisition
            .clone()
            .resolve(&mut spec, source.to_vec())
            .unwrap();
        assert_eq!(image, [1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(stencil(&spec), (2, 2, false, vec![0x80, 0x40]));

        // A delivery that ends inside the second block keeps the first.
        let mut spec = gray.clone();
        let image = acquisition
            .resolve(&mut spec, source[..8].to_vec())
            .unwrap();
        assert_eq!(image, [1, 2, 3, 4]);
        assert_eq!(spec.height, 2);
        assert_eq!(stencil(&spec), (2, 1, false, vec![0x80]));
    }

    #[test]
    fn a_separate_mask_is_cut_with_the_image() {
        let gray = data(2, 4, 8, SpaceSpec::DeviceGray);
        let separate = |bytes: &[u8]| {
            let mut acquisition =
                MaskAcquisition::new(&gray, &mask(8, 8, 1), 3, Some(SOURCE), SOURCE).unwrap();
            let s = acquisition.separate_mut().unwrap();
            assert_eq!(s.needed, 8);
            s.bytes = bytes.to_vec();
            acquisition
        };
        let full: Vec<u8> = (1..=8).collect();
        // Half the image: the mask's upper half goes with it.
        let mut spec = gray.clone();
        let image = separate(&full)
            .resolve(&mut spec, vec![9, 9, 9, 9])
            .unwrap();
        assert_eq!((image.len(), spec.height), (4, 2));
        assert_eq!(stencil(&spec), (8, 4, false, vec![1, 2, 3, 4]));
        // Three of eight mask rows cover one image row, with the mask
        // rounded up to two rows.
        let mut spec = gray.clone();
        let image = separate(&full[..3]).resolve(&mut spec, vec![7; 8]).unwrap();
        assert_eq!((image.len(), spec.height), (2, 1));
        assert_eq!(stencil(&spec), (8, 2, false, vec![1, 2]));
        // Complete parts stay whole.
        let mut spec = gray.clone();
        separate(&full).resolve(&mut spec, vec![7; 8]).unwrap();
        assert_eq!(spec.height, 4);
        assert_eq!(stencil(&spec).3, full);
    }

    #[test]
    fn reversed_masks_are_turned_to_the_image() {
        let gray = data(2, 2, 8, SpaceSpec::DeviceGray);
        let flipped = |a: [f32; 6]| ImageSpec {
            matrix: Matrix(a),
            ..mask(4, 2, 1)
        };
        let resolve = |m: ImageSpec, bytes: &[u8], image: Vec<u8>| {
            let mut acquisition = MaskAcquisition::new(&gray, &m, 3, Some(SOURCE), SOURCE).unwrap();
            acquisition.separate_mut().unwrap().bytes = bytes.to_vec();
            let mut spec = gray.clone();
            acquisition.resolve(&mut spec, image).unwrap();
            spec
        };
        let rows = flipped([4.0, 0.0, 0.0, 2.0, 0.0, 0.0]);
        let spec = resolve(rows.clone(), &[0x80, 0x10], vec![0; 4]);
        assert_eq!(stencil(&spec).3, [0x10, 0x80]);
        // A short reversed mask covers none of the image's leading rows.
        let spec = resolve(rows, &[0x80], vec![0; 4]);
        assert_eq!(spec.height, 0);
        assert_eq!(stencil(&spec), (4, 0, false, vec![]));
        let columns = flipped([-4.0, 0.0, 0.0, -2.0, 4.0, 2.0]);
        let spec = resolve(columns, &[0x80, 0x30], vec![0; 4]);
        assert_eq!(stencil(&spec).3, [0x10, 0xC0]);
    }

    #[test]
    fn the_decode_becomes_a_flag_or_a_constant() {
        let gray = data(4, 1, 8, SpaceSpec::DeviceGray);
        let resolve = |decode: [f32; 2]| {
            let m = ImageSpec {
                decode: decode.to_vec(),
                ..mask(4, 1, 1)
            };
            let mut acquisition = MaskAcquisition::new(&gray, &m, 3, Some(SOURCE), SOURCE).unwrap();
            acquisition.separate_mut().unwrap().bytes = vec![0xA0];
            let mut spec = gray.clone();
            acquisition.resolve(&mut spec, vec![0; 4]).unwrap();
            stencil(&spec)
        };
        assert_eq!(resolve([0.0, 1.0]), (4, 1, false, vec![0xA0]));
        assert_eq!(resolve([1.0, 0.0]), (4, 1, true, vec![0xA0]));
        assert_eq!(resolve([0.7, 0.2]), (4, 1, true, vec![0xA0]));
        assert_eq!(resolve([1.0, 0.5]), (4, 1, false, vec![0xF0]));
        assert_eq!(resolve([0.0, 0.25]), (4, 1, false, vec![0x00]));
    }

    #[test]
    fn encoded_samples_keep_their_height_and_the_mask_fills_in() {
        let gray = data(2, 2, 8, SpaceSpec::DeviceGray);
        let mut acquisition =
            MaskAcquisition::new(&gray, &mask(2, 2, 1), 3, Some(SOURCE), SOURCE).unwrap();
        acquisition.separate_mut().unwrap().bytes = vec![0x40];
        let mut spec = ImageSpec {
            encoded: Some(crate::graphics::Encoded::Dct),
            ..gray.clone()
        };
        let image = acquisition.resolve(&mut spec, vec![0xFF, 0xD8]).unwrap();
        assert_eq!(image, [0xFF, 0xD8]);
        assert_eq!(spec.height, 2);
        assert_eq!(stencil(&spec), (2, 2, false, vec![0x40, 0xC0]));
    }

    #[test]
    fn mask_colours_become_ranges() {
        assert_eq!(
            key_ranges(&[255, 0, 7], 3, 8),
            Ok(vec![(255, 255), (0, 0), (7, 7)])
        );
        assert_eq!(key_ranges(&[0, 3], 1, 4), Ok(vec![(0, 3)]));
        assert_eq!(key_ranges(&[-5, 300], 1, 8), Ok(vec![(0, 255)]));
        assert_eq!(key_ranges(&[9, 2], 1, 4), Ok(vec![(9, 2)]));
        assert_eq!(key_ranges(&[1, 2, 3, 4], 3, 8), Err(VmError::RangeCheck));
        assert_eq!(key_ranges(&[], 1, 8), Err(VmError::RangeCheck));
    }
}

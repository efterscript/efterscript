// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Image downsampling, the `Downsample…Images` parameters. An image's
//! resolution on the page is its sample count over the length of the
//! unit square's edges in default user space (ISO 32000-1 §8.9.4: the
//! image matrix maps the unit square), at 72 units per inch; when it
//! exceeds its class's target the image is reduced by the largest
//! integer factor that keeps it at or above the target, the same on
//! both axes. Eight-bit samples are averaged per component over
//! factor×factor blocks (rounded, edge blocks over what exists) or
//! subsampled from each block's top-left sample; one-bit masks and
//! one-bit gray are subsampled whatever the type asks. The matrix maps
//! the unit square and stays as it is; only the sample grid changes.
//! Indexed images, every other depth, and images carried in an encoded
//! form (a DCT stream has no samples to average) are left alone.

use ps_graphics::{Image, IrOp, Page};
use ps_vm::{Encoded, ImageSpec, Matrix, SpaceSpec};

use crate::params::{Downsample, Params};

/// The parameter classes an image falls into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Mono,
    Gray,
    Color,
}

impl Class {
    /// The class of `spec`, or why it has none: encoded data has no
    /// samples to reduce, an Indexed space has no component to average,
    /// and only eight-bit samples and one-bit masks or gray are reduced.
    fn of(spec: &ImageSpec) -> Result<Class, String> {
        if let Some(Encoded::Dct) = spec.encoded {
            return Err("DCT-encoded".to_string());
        }
        if matches!(spec.color_space, Some(SpaceSpec::Indexed { .. })) {
            return Err("Indexed".to_string());
        }
        match (spec.bits_per_component, spec.components()) {
            (1, 1) => Ok(Class::Mono),
            (8, 1) => Ok(Class::Gray),
            (8, _) => Ok(Class::Color),
            (bits, _) => Err(format!("{bits}-bit")),
        }
    }

    /// The class's target resolution and method when it is enabled.
    fn settings(self, params: &Params) -> Option<(u32, Downsample)> {
        match self {
            Class::Mono => params.downsample_mono_images.then_some((
                params.mono_image_resolution,
                params.mono_image_downsample_type,
            )),
            Class::Gray => params.downsample_gray_images.then_some((
                params.gray_image_resolution,
                params.gray_image_downsample_type,
            )),
            Class::Color => params.downsample_color_images.then_some((
                params.color_image_resolution,
                params.color_image_downsample_type,
            )),
        }
    }
}

/// What downsampling did over the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tally {
    /// Images reduced.
    pub images: usize,
    /// A one-bit image was subsampled where averaging was asked.
    pub mono_subsampled: bool,
}

/// What became of one image.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Outcome {
    /// Left as delivered: downsampling off for its class, or already at
    /// or below the target.
    Unchanged,
    /// Reduced; `mono_subsampled` says a one-bit image was subsampled
    /// where averaging was asked.
    Reduced { image: Image, mono_subsampled: bool },
    /// Left as delivered for the reason given, worth a note.
    Unsupported(String),
}

/// The matrices each image of `page` is painted through on the page
/// itself, by resource index; an image painted only inside a glyph
/// procedure has none here.
pub(crate) fn painted(page: &Page) -> Vec<Vec<Matrix>> {
    let mut matrices = vec![Vec::new(); page.resources.images.len()];
    for op in &page.ops {
        if let IrOp::Image { image, matrix } = &op.op
            && let Some(slot) = matrices.get_mut(image.0)
        {
            slot.push(*matrix);
        }
    }
    matrices
}

/// Samples per inch along the image's two axes on the page, from the
/// matrix mapping its unit square into default user space; `None` when
/// the matrix collapses an axis.
pub(crate) fn resolution(spec: &ImageSpec, matrix: Matrix) -> Option<(f64, f64)> {
    let [a, b, c, d, _, _] = matrix.0.map(f64::from);
    let x_extent = a.hypot(b);
    let y_extent = c.hypot(d);
    if !(x_extent > 0.0 && y_extent > 0.0) {
        return None;
    }
    Some((
        f64::from(spec.width) / x_extent * 72.0,
        f64::from(spec.height) / y_extent * 72.0,
    ))
}

/// The largest integer factor that keeps the lesser of the two
/// resolutions at or above `target`; at least 1.
pub(crate) fn factor(resolution: (f64, f64), target: u32) -> u32 {
    let least = resolution.0.min(resolution.1);
    (least / f64::from(target.max(1))).floor().max(1.0) as u32
}

/// Reduces `image` for the parameters when its class is enabled and
/// its resolution through every matrix it is painted with exceeds the
/// class's target.
pub(crate) fn reduce(image: &Image, matrices: &[Matrix], params: &Params) -> Outcome {
    if !(params.downsample_color_images
        || params.downsample_gray_images
        || params.downsample_mono_images)
    {
        return Outcome::Unchanged;
    }
    let spec = &image.spec;
    let class = match Class::of(spec) {
        Ok(class) => class,
        Err(why) => return Outcome::Unsupported(why),
    };
    let Some((target, method)) = class.settings(params) else {
        return Outcome::Unchanged;
    };
    if matrices.is_empty() {
        return Outcome::Unsupported("painted only inside a glyph procedure".to_string());
    }
    // Several paints of one image reduce by what the largest keeps.
    let mut least = (f64::INFINITY, f64::INFINITY);
    for &matrix in matrices {
        let Some((x, y)) = resolution(spec, matrix) else {
            return Outcome::Unchanged;
        };
        least = (least.0.min(x), least.1.min(y));
    }
    let factor = factor(least, target);
    if factor <= 1 {
        return Outcome::Unchanged;
    }
    let Some(len) = spec.data_len() else {
        return Outcome::Unchanged;
    };
    if image.data.len() < len {
        return Outcome::Unchanged;
    }
    let f = factor as usize;
    let (width, height) = (spec.width as usize, spec.height as usize);
    let (data, mono_subsampled) = match (class, method) {
        (Class::Mono, method) => (
            subsample_bits(&image.data, width, height, f),
            method == Downsample::Average,
        ),
        (_, Downsample::Average) => (
            average_bytes(&image.data, width, height, spec.components(), f),
            false,
        ),
        (_, Downsample::Subsample) => (
            subsample_bytes(&image.data, width, height, spec.components(), f),
            false,
        ),
    };
    Outcome::Reduced {
        image: Image {
            spec: ImageSpec {
                width: width.div_ceil(f) as u32,
                height: height.div_ceil(f) as u32,
                ..spec.clone()
            },
            color_space: image.color_space,
            data,
        },
        mono_subsampled,
    }
}

/// The block of source samples behind output position `o` along an
/// axis of `len` samples.
fn block(o: usize, f: usize, len: usize) -> std::ops::Range<usize> {
    (o * f)..((o + 1) * f).min(len)
}

/// Eight-bit samples averaged per component over `f`×`f` blocks.
fn average_bytes(data: &[u8], width: usize, height: usize, comps: usize, f: usize) -> Vec<u8> {
    let row = width * comps;
    let (new_w, new_h) = (width.div_ceil(f), height.div_ceil(f));
    let mut out = Vec::with_capacity(new_w * new_h * comps);
    for oy in 0..new_h {
        let rows = block(oy, f, height);
        for ox in 0..new_w {
            let cols = block(ox, f, width);
            let count = (rows.len() * cols.len()) as u32;
            for c in 0..comps {
                let sum: u32 = rows
                    .clone()
                    .flat_map(|y| cols.clone().map(move |x| (y, x)))
                    .map(|(y, x)| u32::from(data[y * row + x * comps + c]))
                    .sum();
                out.push(((sum + count / 2) / count) as u8);
            }
        }
    }
    out
}

/// Eight-bit samples taken from each block's top-left sample.
fn subsample_bytes(data: &[u8], width: usize, height: usize, comps: usize, f: usize) -> Vec<u8> {
    let row = width * comps;
    let (new_w, new_h) = (width.div_ceil(f), height.div_ceil(f));
    let mut out = Vec::with_capacity(new_w * new_h * comps);
    for oy in 0..new_h {
        for ox in 0..new_w {
            let at = oy * f * row + ox * f * comps;
            out.extend_from_slice(&data[at..at + comps]);
        }
    }
    out
}

/// One-bit samples, rows padded to a byte, taken from each block's
/// top-left sample and packed the same way.
fn subsample_bits(data: &[u8], width: usize, height: usize, f: usize) -> Vec<u8> {
    let row = width.div_ceil(8);
    let (new_w, new_h) = (width.div_ceil(f), height.div_ceil(f));
    let new_row = new_w.div_ceil(8);
    let mut out = vec![0u8; new_row * new_h];
    for oy in 0..new_h {
        for ox in 0..new_w {
            let (x, y) = (ox * f, oy * f);
            let bit = (data[y * row + x / 8] >> (7 - x % 8)) & 1;
            out[oy * new_row + ox / 8] |= bit << (7 - ox % 8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(space: Option<SpaceSpec>, bits: u8, width: u32, height: u32) -> ImageSpec {
        let components = space.as_ref().map_or(1, SpaceSpec::components);
        ImageSpec {
            width,
            height,
            bits_per_component: bits,
            is_mask: space.is_none(),
            color_space: space,
            decode: [0.0, 1.0].repeat(components),
            matrix: Matrix::IDENTITY,
            interpolate: false,
            encoded: None,
        }
    }

    fn image(spec: ImageSpec, data: Vec<u8>) -> Image {
        Image {
            spec,
            color_space: None,
            data,
        }
    }

    fn gray_on(target: u32, method: Downsample) -> Params {
        Params {
            downsample_gray_images: true,
            gray_image_resolution: target,
            gray_image_downsample_type: method,
            ..Params::default()
        }
    }

    /// One inch square at the origin.
    const INCH: Matrix = Matrix([72.0, 0.0, 0.0, 72.0, 0.0, 0.0]);
    /// One point square: four samples across it are 288 per inch.
    const POINT: Matrix = Matrix::IDENTITY;

    #[test]
    fn resolution_comes_from_the_unit_square_extent_and_the_factor_floors() {
        let s = spec(Some(SpaceSpec::DeviceGray), 8, 300, 300);
        assert_eq!(resolution(&s, INCH), Some((300.0, 300.0)));
        // Rotated by 90 degrees the extents are the same.
        let rotated = Matrix([0.0, 72.0, -72.0, 0.0, 0.0, 0.0]);
        assert_eq!(resolution(&s, rotated), Some((300.0, 300.0)));
        // Two inches wide, one high: 150 by 300.
        let wide = Matrix([144.0, 0.0, 0.0, 72.0, 0.0, 0.0]);
        assert_eq!(resolution(&s, wide), Some((150.0, 300.0)));
        assert_eq!(
            resolution(&s, Matrix([72.0, 0.0, 0.0, 0.0, 0.0, 0.0])),
            None
        );
        assert_eq!(factor((300.0, 300.0), 72), 4);
        assert_eq!(factor((150.0, 300.0), 72), 2);
        assert_eq!(factor((100.0, 100.0), 150), 1);
        assert_eq!(factor((144.0, 144.0), 72), 2);
        assert_eq!(factor((143.0, 144.0), 72), 1);
    }

    #[test]
    fn averaging_rounds_per_component_and_edge_blocks_cover_what_exists() {
        // 5 wide, 3 high, factor 4: one full block and one 1×3 block.
        let data: Vec<u8> = (0..15).map(|i| i * 10).collect();
        let out = average_bytes(&data, 5, 3, 1, 4);
        // Block one: rows 0..3, columns 0..4 → the twelve samples
        // 0,10,20,30,50,60,70,80,100,110,120,130 average 65.
        // Block two: column 4 of each row → 40, 90, 140 average 90.
        assert_eq!(out, [65, 90]);
        // Half rounds up: 0 and 1 average 1.
        assert_eq!(average_bytes(&[0, 1], 2, 1, 1, 2), [1]);
        // Three components stay apart.
        let rgb = [
            10, 20, 30, 20, 40, 60, //
            30, 60, 90, 40, 80, 120,
        ];
        assert_eq!(average_bytes(&rgb, 2, 2, 3, 2), [25, 50, 75]);
    }

    #[test]
    fn subsampling_takes_the_top_left_sample_of_each_block() {
        let data: Vec<u8> = (0..15).collect();
        assert_eq!(subsample_bytes(&data, 5, 3, 1, 2), [0, 2, 4, 10, 12, 14]);
        let rgb: Vec<u8> = (0..24).collect();
        assert_eq!(subsample_bytes(&rgb, 4, 2, 3, 2), [0, 1, 2, 6, 7, 8]);
        // Bits: a 10-wide, 4-high mask whose second row is set; factor 2
        // keeps rows 0 and 2 and columns 0, 2, 4, 6, 8.
        let mask = [
            0b1010_1010,
            0b1000_0000, //
            0b1111_1111,
            0b1100_0000, //
            0b0101_0101,
            0b0100_0000, //
            0b0000_0000,
            0b0000_0000,
        ];
        assert_eq!(subsample_bits(&mask, 10, 4, 2), [0b1111_1000, 0b0000_0000]);
        assert_eq!(subsample_bits(&mask, 10, 4, 3), [0b1010_0000, 0b0000_0000]);
    }

    #[test]
    fn reduce_follows_the_class_settings_and_reports_the_rest() {
        let gray = image(spec(Some(SpaceSpec::DeviceGray), 8, 4, 4), vec![100; 16]);
        assert_eq!(
            reduce(&gray, &[POINT], &Params::default()),
            Outcome::Unchanged
        );
        // Four samples over a point is 288 per inch; the target 72 gives
        // factor 4 and one sample. Over an inch it is 4 per inch: left.
        let Outcome::Reduced {
            image: small,
            mono_subsampled,
        } = reduce(&gray, &[POINT], &gray_on(72, Downsample::Average))
        else {
            panic!("reduced")
        };
        assert_eq!((small.spec.width, small.spec.height), (1, 1));
        assert_eq!(small.data, [100]);
        assert!(!mono_subsampled);
        assert_eq!(
            reduce(&gray, &[INCH], &gray_on(72, Downsample::Average)),
            Outcome::Unchanged
        );
        assert_eq!(
            reduce(&gray, &[POINT], &gray_on(150, Downsample::Average)),
            Outcome::Unchanged,
            "288 over 150 floors to 1"
        );
        // The colour flag does not touch a gray image, and a gray image
        // painted twice reduces by what the larger paint allows.
        let colour_only = Params {
            downsample_color_images: true,
            color_image_resolution: 9,
            ..Params::default()
        };
        assert_eq!(reduce(&gray, &[POINT], &colour_only), Outcome::Unchanged);
        let twice = [POINT, Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0])];
        let Outcome::Reduced { image: small, .. } =
            reduce(&gray, &twice, &gray_on(72, Downsample::Subsample))
        else {
            panic!("reduced")
        };
        assert_eq!((small.spec.width, small.spec.height), (2, 2));
        // Unsupported shapes are named; a glyph-procedure image too.
        let indexed = SpaceSpec::Indexed {
            base: Box::new(SpaceSpec::DeviceRGB),
            hival: 1,
            lookup: vec![0; 6],
        };
        assert_eq!(
            reduce(
                &image(spec(Some(indexed), 8, 4, 4), vec![0; 16]),
                &[POINT],
                &gray_on(72, Downsample::Average)
            ),
            Outcome::Unsupported("Indexed".to_string())
        );
        assert_eq!(
            reduce(
                &image(spec(Some(SpaceSpec::DeviceGray), 4, 4, 4), vec![0; 8]),
                &[POINT],
                &gray_on(72, Downsample::Average)
            ),
            Outcome::Unsupported("4-bit".to_string())
        );
        assert_eq!(
            reduce(&gray, &[], &gray_on(72, Downsample::Average)),
            Outcome::Unsupported("painted only inside a glyph procedure".to_string())
        );
        // Short data is left alone.
        let short = image(spec(Some(SpaceSpec::DeviceGray), 8, 4, 4), vec![0; 3]);
        assert_eq!(
            reduce(&short, &[POINT], &gray_on(72, Downsample::Average)),
            Outcome::Unchanged
        );
    }

    #[test]
    fn a_mask_is_subsampled_even_when_averaging_is_asked() {
        let mask = image(spec(None, 1, 8, 8), vec![0b1000_0000; 8]);
        let params = Params {
            downsample_mono_images: true,
            mono_image_resolution: 72,
            mono_image_downsample_type: Downsample::Average,
            ..Params::default()
        };
        // Eight samples over a point: 576 per inch, factor 8 at 72.
        let Outcome::Reduced {
            image: small,
            mono_subsampled,
        } = reduce(&mask, &[POINT], &params)
        else {
            panic!("reduced")
        };
        assert!(mono_subsampled);
        assert_eq!((small.spec.width, small.spec.height), (1, 1));
        assert_eq!(small.data, [0b1000_0000]);
        assert!(small.spec.is_mask);
        let subsample = Params {
            mono_image_downsample_type: Downsample::Subsample,
            ..params
        };
        let Outcome::Reduced {
            mono_subsampled, ..
        } = reduce(&mask, &[POINT], &subsample)
        else {
            panic!("reduced")
        };
        assert!(!mono_subsampled);
    }

    #[test]
    fn painted_matrices_are_gathered_by_resource_index() {
        let mut page = Page::new(ps_vm::Bounds::new(0.0, 0.0, 100.0, 100.0));
        let first = page
            .resources
            .add_image(&spec(Some(SpaceSpec::DeviceGray), 8, 1, 1), &[0]);
        let second = page
            .resources
            .add_image(&spec(Some(SpaceSpec::DeviceGray), 8, 1, 1), &[0]);
        page.ops = vec![
            IrOp::Image {
                image: second,
                matrix: INCH,
            },
            IrOp::Image {
                image: second,
                matrix: Matrix::IDENTITY,
            },
        ]
        .into_iter()
        .map(ps_graphics::Op::from)
        .collect();
        let painted = painted(&page);
        assert!(painted[first.0].is_empty());
        assert_eq!(painted[second.0], [INCH, Matrix::IDENTITY]);
    }
}

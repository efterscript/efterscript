// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `image`, `imagemask`, and `colorimage` (PLRM3 §4.10): the operand and
//! dictionary forms, and sample-data acquisition. A string source is
//! taken as is, a file is read for the required count, and a procedure
//! runs as a loop frame until it has delivered enough or returns an
//! empty string. A `colorimage` with one source per component collects
//! each component's plane from its own source, the procedures called in
//! rotation, and interleaves the planes at the end (8-bit samples only;
//! packed depths are `limitcheck`). The backend receives complete rows
//! only: a source that runs dry truncates the image to the rows it
//! delivered.

use crate::error::VmError;
use crate::graphics::{ImageSpec, SpaceSpec};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::graphics::read_matrix;

/// Sample data being collected for one image.
#[derive(Clone, Debug)]
pub struct ImageAcquisition {
    pub(crate) spec: ImageSpec,
    pub(crate) needed: usize,
    pub(crate) data: Vec<u8>,
    /// Whether the data procedure has been started.
    pub(crate) started: bool,
    operator: &'static str,
    /// One source per component when `colorimage` was given several;
    /// each feeds its own plane, `current` being the one to call next.
    pub(crate) sources: Vec<Object>,
    planes: Vec<Vec<u8>>,
    current: usize,
}

impl ImageAcquisition {
    fn new(spec: ImageSpec, operator: &'static str) -> Result<Self, VmError> {
        let needed = spec.data_len().ok_or(VmError::LimitCheck)?;
        Ok(ImageAcquisition {
            spec,
            needed,
            data: Vec::new(),
            started: false,
            operator,
            sources: Vec::new(),
            planes: Vec::new(),
            current: 0,
        })
    }

    /// As `new`, collecting one plane per source.
    fn planar(spec: ImageSpec, sources: Vec<Object>) -> Result<Self, VmError> {
        let mut acquisition = Self::new(spec, "colorimage")?;
        acquisition.planes = vec![Vec::new(); sources.len()];
        acquisition.sources = sources;
        Ok(acquisition)
    }

    pub(crate) fn operator_name(&self) -> &'static str {
        self.operator
    }

    fn is_planar(&self) -> bool {
        !self.planes.is_empty()
    }

    /// Bytes each plane holds when complete: one per sample.
    fn plane_needed(&self) -> usize {
        self.needed / self.planes.len().max(1)
    }

    pub(crate) fn is_complete(&self) -> bool {
        if self.is_planar() {
            let needed = self.plane_needed();
            self.planes.iter().all(|plane| plane.len() >= needed)
        } else {
            self.data.len() >= self.needed
        }
    }

    /// Appends a chunk — to the current plane when planar, which then
    /// moves on to the next incomplete one; returns whether more is
    /// wanted. An empty chunk ends the acquisition early.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> bool {
        if chunk.is_empty() {
            return false;
        }
        if self.is_planar() {
            let needed = self.plane_needed();
            let plane = &mut self.planes[self.current];
            let room = needed - plane.len().min(needed);
            plane.extend_from_slice(&chunk[..chunk.len().min(room)]);
            let count = self.planes.len();
            for step in 1..=count {
                let next = (self.current + step) % count;
                if self.planes[next].len() < needed {
                    self.current = next;
                    break;
                }
            }
        } else {
            let room = self.needed - self.data.len();
            self.data.extend_from_slice(&chunk[..chunk.len().min(room)]);
        }
        !self.is_complete()
    }

    /// The procedure to call for the next chunk of a planar acquisition.
    pub(crate) fn next_source(&self) -> Option<Object> {
        self.sources.get(self.current).copied()
    }

    /// The planes interleaved sample by sample into `data`, as many
    /// whole rows as every plane delivered.
    fn interleave(&mut self) {
        let width = self.spec.width as usize;
        let rows = self
            .planes
            .iter()
            .map(|plane| plane.len().checked_div(width).unwrap_or(0))
            .min()
            .unwrap_or(0);
        let mut data = Vec::with_capacity(rows * width * self.planes.len());
        for at in 0..rows * width {
            for plane in &self.planes {
                data.push(plane[at]);
            }
        }
        self.data = data;
        self.planes.clear();
    }
}

pub(crate) fn image(i: &mut Interp) -> Result<(), VmError> {
    start(i, false)
}

pub(crate) fn imagemask(i: &mut Interp) -> Result<(), VmError> {
    start(i, true)
}

fn start(i: &mut Interp, is_mask: bool) -> Result<(), VmError> {
    let top = i.peek(0)?;
    let (spec, source, operands) = if top.ty() == Type::Dict {
        let (spec, source) = from_dict(i, top, is_mask)?;
        (spec, source, 1)
    } else {
        let (spec, source) = from_operands(i, is_mask)?;
        (spec, source, 5)
    };
    let operator = if is_mask { "imagemask" } else { "image" };
    let acquisition = ImageAcquisition::new(spec, operator)?;
    acquire(i, acquisition, &[source], operands)
}

/// `width height bits matrix source… multi ncomp colorimage`: samples in
/// the device space of `ncomp` components, from one source or one per
/// component.
pub(crate) fn colorimage(i: &mut Interp) -> Result<(), VmError> {
    let ncomp = i.peek(0)?.as_i32().ok_or(VmError::TypeCheck)?;
    let multi = i.peek(1)?.as_bool().ok_or(VmError::TypeCheck)?;
    let color_space = match ncomp {
        1 => SpaceSpec::DeviceGray,
        3 => SpaceSpec::DeviceRGB,
        4 => SpaceSpec::DeviceCMYK,
        _ => return Err(VmError::RangeCheck),
    };
    let count = if multi { ncomp as usize } else { 1 };
    let sources: Vec<Object> = (0..count)
        .rev()
        .map(|n| i.peek(2 + n))
        .collect::<Result<_, _>>()?;
    let base = 2 + count;
    let matrix = read_matrix(i, i.peek(base)?)?;
    let bits_per_component = bits(i.peek(base + 1)?, false)?;
    let height = dimension(i.peek(base + 2)?)?;
    let width = dimension(i.peek(base + 3)?)?;
    if multi && bits_per_component != 8 {
        return Err(VmError::LimitCheck);
    }
    let components = color_space.components();
    let spec = ImageSpec {
        width,
        height,
        bits_per_component,
        decode: default_decode(Some(&color_space), bits_per_component, components),
        color_space: Some(color_space),
        matrix,
        interpolate: false,
        is_mask: false,
    };
    let acquisition = if multi {
        ImageAcquisition::planar(spec, sources.clone())?
    } else {
        ImageAcquisition::new(spec, "colorimage")?
    };
    acquire(i, acquisition, &sources, base + 4)
}

/// Collects the data from `sources` — strings and files at once,
/// procedures through a loop frame — and hands it on. The sources must
/// all be of one kind.
fn acquire(
    i: &mut Interp,
    mut acquisition: ImageAcquisition,
    sources: &[Object],
    operands: usize,
) -> Result<(), VmError> {
    let kind = |source: &Object| match source.ty() {
        Type::String => Some(0),
        Type::File => Some(1),
        Type::Array | Type::PackedArray if source.is_executable() => Some(2),
        _ => None,
    };
    let kinds: Vec<u8> = sources
        .iter()
        .map(kind)
        .collect::<Option<_>>()
        .ok_or(VmError::TypeCheck)?;
    if kinds.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(VmError::TypeCheck);
    }
    match kinds[0] {
        0 | 1 => {
            for &source in sources {
                let data = if source.ty() == Type::String {
                    bytes(i, source)?
                } else {
                    let wanted = if acquisition.is_planar() {
                        acquisition.plane_needed()
                    } else {
                        acquisition.needed
                    };
                    let mut buffer = vec![0u8; wanted];
                    let mut filled = 0;
                    while filled < buffer.len() {
                        let got = i.mem.file_read(source, &mut buffer[filled..])?;
                        if got == 0 {
                            break;
                        }
                        filled += got;
                    }
                    buffer.truncate(filled);
                    buffer
                };
                acquisition.feed(&data);
            }
            drop_operands(i, operands)?;
            finish(i, acquisition)
        }
        _ => {
            drop_operands(i, operands)?;
            i.push_frame(Frame::Loop(LoopFrame::ImageData {
                body: sources[0],
                acquisition: Box::new(acquisition),
            }))
        }
    }
}

fn drop_operands(i: &mut Interp, count: usize) -> Result<(), VmError> {
    for _ in 0..count {
        i.pop()?;
    }
    Ok(())
}

/// Hands the collected data to the backend, trimmed to whole rows.
pub(crate) fn finish(i: &mut Interp, acquisition: ImageAcquisition) -> Result<(), VmError> {
    let mut acquisition = acquisition;
    if acquisition.is_planar() {
        acquisition.interleave();
    }
    let ImageAcquisition {
        mut spec,
        needed,
        mut data,
        ..
    } = acquisition;
    if data.len() < needed {
        let row = spec.row_bytes().unwrap_or(0);
        let rows = data.len().checked_div(row).unwrap_or(0);
        data.truncate(rows * row);
        spec.height = u32::try_from(rows).map_err(|_| VmError::LimitCheck)?;
    }
    let backend = i.backend()?;
    if spec.is_mask {
        backend.imagemask(&spec, &data)
    } else {
        backend.image(&spec, &data)
    }
}

fn dimension(object: Object) -> Result<u32, VmError> {
    let n = object.as_i32().ok_or(VmError::TypeCheck)?;
    u32::try_from(n).map_err(|_| VmError::RangeCheck)
}

fn bits(object: Object, is_mask: bool) -> Result<u8, VmError> {
    let n = object.as_i32().ok_or(VmError::TypeCheck)?;
    let allowed: &[i32] = if is_mask { &[1] } else { &[1, 2, 4, 8, 12] };
    if !allowed.contains(&n) {
        return Err(VmError::RangeCheck);
    }
    Ok(n as u8)
}

fn default_decode(space: Option<&SpaceSpec>, bits: u8, components: usize) -> Vec<f32> {
    match space {
        Some(SpaceSpec::Indexed { .. }) => vec![0.0, ((1u32 << bits) - 1) as f32],
        _ => [0.0, 1.0].repeat(components),
    }
}

fn decode_array(i: &Interp, object: Object, components: usize) -> Result<Vec<f32>, VmError> {
    if !matches!(object.ty(), Type::Array | Type::PackedArray) {
        return Err(VmError::TypeCheck);
    }
    let values: Vec<f32> = items(i, object)?
        .into_iter()
        .map(|o| o.as_number().ok_or(VmError::TypeCheck))
        .collect::<Result<_, _>>()?;
    if values.len() != 2 * components {
        return Err(VmError::RangeCheck);
    }
    Ok(values)
}

/// The space an `image` paints in: the current colour space.
/// The space the samples are in: none for a mask, DeviceGray for the
/// operand form (PLRM3 §4.10.5), the current colour space for the
/// dictionary form.
fn sample_space(
    i: &mut Interp,
    is_mask: bool,
    from_operands: bool,
) -> Result<Option<SpaceSpec>, VmError> {
    if is_mask {
        Ok(None)
    } else if from_operands {
        Ok(Some(SpaceSpec::DeviceGray))
    } else {
        Ok(Some(i.backend()?.current_color_space()))
    }
}

// `width height bits matrix source image` and
// `width height polarity matrix source imagemask`
fn from_operands(i: &mut Interp, is_mask: bool) -> Result<(ImageSpec, Object), VmError> {
    let source = i.peek(0)?;
    let matrix = read_matrix(i, i.peek(1)?)?;
    let third = i.peek(2)?;
    let height = dimension(i.peek(3)?)?;
    let width = dimension(i.peek(4)?)?;
    let color_space = sample_space(i, is_mask, true)?;
    let components = color_space.as_ref().map_or(1, SpaceSpec::components);
    let (bits_per_component, decode) = if is_mask {
        let polarity = third.as_bool().ok_or(VmError::TypeCheck)?;
        (
            1,
            if polarity {
                vec![1.0, 0.0]
            } else {
                vec![0.0, 1.0]
            },
        )
    } else {
        let bits = bits(third, false)?;
        (bits, default_decode(color_space.as_ref(), bits, components))
    };
    Ok((
        ImageSpec {
            width,
            height,
            bits_per_component,
            color_space,
            decode,
            matrix,
            interpolate: false,
            is_mask,
        },
        source,
    ))
}

fn entry(i: &mut Interp, dict: Object, key: &str) -> Result<Option<Object>, VmError> {
    let key = i.intern(key);
    i.mem.dict_get(dict, key)
}

fn required(i: &mut Interp, dict: Object, key: &str) -> Result<Object, VmError> {
    entry(i, dict, key)?.ok_or(VmError::TypeCheck)
}

// The Level 2 dictionary form. `MultipleDataSources` other than `false`
// and image types other than 1 are outside what the backend accepts.
fn from_dict(i: &mut Interp, dict: Object, is_mask: bool) -> Result<(ImageSpec, Object), VmError> {
    if let Some(kind) = entry(i, dict, "ImageType")?
        && kind.as_i32() != Some(1)
    {
        return Err(VmError::RangeCheck);
    }
    if let Some(multiple) = entry(i, dict, "MultipleDataSources")?
        && multiple.as_bool() != Some(false)
    {
        return Err(VmError::TypeCheck);
    }
    let width = dimension(required(i, dict, "Width")?)?;
    let height = dimension(required(i, dict, "Height")?)?;
    let bits_per_component = bits(required(i, dict, "BitsPerComponent")?, is_mask)?;
    let matrix = required(i, dict, "ImageMatrix")?;
    let matrix = read_matrix(i, matrix)?;
    let source = required(i, dict, "DataSource")?;
    let interpolate = match entry(i, dict, "Interpolate")? {
        Some(flag) => flag.as_bool().ok_or(VmError::TypeCheck)?,
        None => false,
    };
    let color_space = sample_space(i, is_mask, false)?;
    let components = color_space.as_ref().map_or(1, SpaceSpec::components);
    let decode = match entry(i, dict, "Decode")? {
        Some(array) => decode_array(i, array, components)?,
        None => default_decode(color_space.as_ref(), bits_per_component, components),
    };
    Ok((
        ImageSpec {
            width,
            height,
            bits_per_component,
            color_space,
            decode,
            matrix,
            interpolate,
            is_mask,
        },
        source,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::Matrix;

    fn spec(width: u32, height: u32, bits: u8) -> ImageSpec {
        ImageSpec {
            width,
            height,
            bits_per_component: bits,
            color_space: Some(SpaceSpec::DeviceGray),
            decode: vec![0.0, 1.0],
            matrix: Matrix::IDENTITY,
            interpolate: false,
            is_mask: false,
        }
    }

    #[test]
    fn feeding_stops_at_the_required_count_or_an_empty_chunk() {
        let mut a = ImageAcquisition::new(spec(10, 3, 1), "image").unwrap();
        assert_eq!(a.needed, 6);
        assert!(a.feed(&[1, 2, 3, 4]));
        assert!(!a.feed(&[5, 6, 7, 8]));
        assert_eq!(a.data, [1, 2, 3, 4, 5, 6]);
        let mut b = ImageAcquisition::new(spec(10, 3, 1), "image").unwrap();
        assert!(b.feed(&[1]));
        assert!(!b.feed(&[]));
        assert!(!b.is_complete());
        let empty = ImageAcquisition::new(spec(0, 3, 8), "image").unwrap();
        assert!(empty.is_complete());
    }

    #[test]
    fn planes_rotate_and_interleave() {
        let rgb = ImageSpec {
            color_space: Some(SpaceSpec::DeviceRGB),
            decode: vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            ..spec(2, 2, 8)
        };
        let sources = vec![Object::integer(0), Object::integer(1), Object::integer(2)];
        let mut a = ImageAcquisition::planar(rgb, sources).unwrap();
        let next = |a: &ImageAcquisition| a.next_source().and_then(|o| o.as_i32());
        assert_eq!(a.plane_needed(), 4);
        assert_eq!(next(&a), Some(0));
        assert!(a.feed(&[1, 2]));
        assert_eq!(next(&a), Some(1));
        assert!(a.feed(&[11, 12, 13, 14, 15]));
        assert_eq!(next(&a), Some(2));
        assert!(a.feed(&[21, 22, 23, 24]));
        // Back to the first plane, the only incomplete one.
        assert_eq!(next(&a), Some(0));
        assert!(!a.feed(&[3, 4]));
        a.interleave();
        assert_eq!(a.data, [1, 11, 21, 2, 12, 22, 3, 13, 23, 4, 14, 24]);
        // A short plane cuts the image to the rows every plane has.
        let rgb = ImageSpec {
            color_space: Some(SpaceSpec::DeviceRGB),
            decode: vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            ..spec(2, 2, 8)
        };
        let mut b = ImageAcquisition::planar(rgb, vec![Object::null(); 3]).unwrap();
        b.feed(&[1, 2, 3, 4]);
        b.feed(&[5, 6]);
        b.feed(&[7, 8, 9, 10]);
        b.interleave();
        assert_eq!(b.data, [1, 5, 7, 2, 6, 8]);
    }

    #[test]
    fn decode_defaults_follow_the_space() {
        assert_eq!(
            default_decode(Some(&SpaceSpec::DeviceRGB), 8, 3),
            vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0]
        );
        let indexed = SpaceSpec::Indexed {
            base: Box::new(SpaceSpec::DeviceRGB),
            hival: 3,
            lookup: vec![0; 12],
        };
        assert_eq!(default_decode(Some(&indexed), 4, 1), vec![0.0, 15.0]);
        assert_eq!(default_decode(None, 1, 1), vec![0.0, 1.0]);
    }

    #[test]
    fn bit_depths_are_checked() {
        assert_eq!(bits(Object::integer(12), false), Ok(12));
        assert_eq!(bits(Object::integer(3), false), Err(VmError::RangeCheck));
        assert_eq!(bits(Object::integer(8), true), Err(VmError::RangeCheck));
        assert_eq!(bits(Object::real(1.0), true), Err(VmError::TypeCheck));
        assert_eq!(dimension(Object::integer(-1)), Err(VmError::RangeCheck));
    }
}

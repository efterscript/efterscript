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
//! delivered. A file source that is a decode filter is read through its
//! end-of-data marker once the samples are in, so a program that put
//! its data inline after `image` continues after the marker.
//!
//! A file source whose filter is `DCTDecode` is not read for samples
//! (the interpreter decodes no JPEG): the encoded bytes are read from
//! the file under the filter, as far as the stream's own end-of-image
//! marker or the source's end, and handed on flagged as encoded, the
//! dictionary's dimensions and depth describing the samples a decoder
//! would produce.
//!
//! An image in a CIE-based space that does not collapse (see `cie`) has
//! its samples converted to L*a*b* once they are complete, through a
//! conversion job; its default `Decode` is the space's ranges. One whose
//! data arrived encoded cannot be converted and is handed on as it is,
//! in the device space of its component count.
//!
//! Type 3 and type 4 dictionaries (masked images) are checked completely
//! before any data is read; see `masked` for how a type 3 image's mask
//! is collected and fitted. A type 3 image whose mask has its own source
//! reads the mask completely first, then the image's samples.

mod masked;

use std::rc::Rc;

use crate::error::VmError;
use crate::graphics::{Encoded, ImageMask, ImageSpec, SpaceSpec};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::jpeg::{MarkerWalker, Walk};
use crate::object::{Access, Handle, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::cie::{self, CieSpace};
use crate::ops::file::{drain_to_marker, file_operand};
use crate::ops::graphics::{current_cie, read_matrix};
use crate::ops::pattern;

use masked::MaskAcquisition;

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
    /// The CIE space the samples convert through, when the image's space
    /// is one that does not collapse.
    cie: Option<Rc<CieSpace>>,
    /// A type 3 image's mask; `needed` and `data` then count the image's
    /// own source, which for interleave types 1 and 2 carries the mask.
    mask: Option<Box<MaskAcquisition>>,
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
            cie: None,
            mask: None,
        })
    }

    /// As `new`, for a type 3 image whose mask arrives as `mask` says.
    fn masked(spec: ImageSpec, mask: MaskAcquisition) -> Result<Self, VmError> {
        let needed = mask.needed(&spec).ok_or(VmError::LimitCheck)?;
        let mut acquisition = Self::new(spec, "image")?;
        acquisition.needed = needed;
        acquisition.mask = Some(Box::new(mask));
        Ok(acquisition)
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
        if self.mask_pending() {
            return false;
        }
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
    /// While a separate mask is being read, the chunk goes to the mask,
    /// and an empty chunk ends the mask's stage only.
    pub(crate) fn feed(&mut self, chunk: &[u8]) -> bool {
        if let Some(separate) = self.mask.as_mut().and_then(|m| m.pending()) {
            let room = separate.needed - separate.bytes.len();
            separate
                .bytes
                .extend_from_slice(&chunk[..chunk.len().min(room)]);
            separate.done = chunk.is_empty() || separate.bytes.len() >= separate.needed;
            return !self.is_complete();
        }
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

    /// Whether a separate mask's source still has to deliver.
    fn mask_pending(&self) -> bool {
        self.mask
            .as_ref()
            .and_then(|m| m.separate())
            .is_some_and(|separate| !separate.done)
    }

    /// The source of the next chunk: the current plane's for a planar
    /// acquisition, the mask's and then the image's for a separate mask.
    pub(crate) fn next_source(&self) -> Option<Object> {
        if let Some(separate) = self.mask.as_ref().and_then(|m| m.separate()) {
            return Some(if separate.done {
                separate.data_source
            } else {
                separate.source
            });
        }
        self.sources.get(self.current).copied()
    }

    /// Whether the next chunk comes from a procedure; when not, the
    /// image's string or file source is read as the image finishes.
    pub(crate) fn awaits_procedure(&self) -> bool {
        self.next_source()
            .is_none_or(|source| source_kind(source) == Some(SourceKind::Procedure))
    }

    /// Every source the acquisition still refers to.
    pub(crate) fn references(&self) -> Vec<Object> {
        let mut objects = self.sources.clone();
        if let Some(separate) = self.mask.as_ref().and_then(|m| m.separate()) {
            objects.extend([separate.source, separate.data_source]);
        }
        objects
    }

    /// A separate mask's image source when it is a string or file not
    /// yet read.
    fn unread_data_source(&self) -> Option<Object> {
        let separate = self.mask.as_ref()?.separate()?;
        let immediate = source_kind(separate.data_source) != Some(SourceKind::Procedure);
        (separate.done && immediate && !separate.data_read).then_some(separate.data_source)
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
    let (prepared, operands) = if top.ty() == Type::Dict {
        (from_dict(i, top, is_mask)?, 1)
    } else {
        let (spec, source) = from_operands(i, is_mask)?;
        let prepared = Prepared {
            spec,
            source,
            converting: None,
            mask: None,
        };
        (prepared, 5)
    };
    let operator = if is_mask { "imagemask" } else { "image" };
    // A mask is painted with the current colour, so a pattern's cell is
    // captured before any data is read; `image` and `colorimage` carry
    // their own colours and are undefined inside an uncoloured cell
    // (PLRM3 §4.9.2).
    if is_mask {
        if pattern::capture_cell(i, operator)? {
            return Ok(());
        }
    } else {
        pattern::colour_allowed(i)?;
    }
    let Prepared {
        spec,
        source,
        converting,
        mask,
    } = prepared;
    let mut acquisition = match mask {
        Some(mask) => ImageAcquisition::masked(spec, mask)?,
        None => ImageAcquisition::new(spec, operator)?,
    };
    acquisition.cie = converting;
    if acquisition
        .mask
        .as_ref()
        .is_some_and(|m| m.separate().is_some())
    {
        return acquire_separate(i, acquisition, source, operands);
    }
    acquire(i, acquisition, &[source], operands)
}

/// `width height bits matrix source… multi ncomp colorimage`: samples in
/// the device space of `ncomp` components, from one source or one per
/// component.
pub(crate) fn colorimage(i: &mut Interp) -> Result<(), VmError> {
    pattern::colour_allowed(i)?;
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
        encoded: None,
        mask: None,
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
    let kinds: Vec<SourceKind> = sources
        .iter()
        .map(|&source| source_kind(source))
        .collect::<Option<_>>()
        .ok_or(VmError::TypeCheck)?;
    if kinds.windows(2).any(|pair| pair[0] != pair[1]) {
        return Err(VmError::TypeCheck);
    }
    match kinds[0] {
        SourceKind::Immediate => {
            if let Some(base) = encoded_source(i, &acquisition, sources)? {
                take_encoded(i, &mut acquisition, sources[0], base)?;
                drop_operands(i, operands)?;
                return finish(i, acquisition);
            }
            for &source in sources {
                let wanted = if acquisition.is_planar() {
                    acquisition.plane_needed()
                } else {
                    acquisition.needed
                };
                let data = read_immediate(i, source, wanted)?;
                acquisition.feed(&data);
            }
            drop_operands(i, operands)?;
            finish(i, acquisition)
        }
        SourceKind::Procedure => {
            drop_operands(i, operands)?;
            i.push_frame(Frame::Loop(LoopFrame::ImageData {
                body: sources[0],
                acquisition: Box::new(acquisition),
            }))
        }
    }
}

/// How a data source delivers: strings and files at once, procedures
/// chunk by chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceKind {
    Immediate,
    Procedure,
}

fn source_kind(source: Object) -> Option<SourceKind> {
    match source.ty() {
        Type::String | Type::File => Some(SourceKind::Immediate),
        Type::Array | Type::PackedArray if source.is_executable() => Some(SourceKind::Procedure),
        _ => None,
    }
}

/// A string's bytes, or up to `wanted` bytes of a file, which, when it
/// is a decode filter, is then read through its end-of-data marker so
/// the program resumes after the data.
fn read_immediate(i: &mut Interp, source: Object, wanted: usize) -> Result<Vec<u8>, VmError> {
    if source.ty() == Type::String {
        return bytes(i, source);
    }
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
    drain_to_marker(i, source)?;
    Ok(buffer)
}

/// The encoded stream under the `DCTDecode` filter `source` (whose file
/// beneath is `base`) as the image's data.
fn take_encoded(
    i: &mut Interp,
    acquisition: &mut ImageAcquisition,
    source: Object,
    base: Handle,
) -> Result<(), VmError> {
    let data = read_encoded(i, base)?;
    drain_to_marker(i, source)?;
    acquisition.spec.encoded = Some(Encoded::Dct);
    acquisition.needed = data.len();
    acquisition.data = data;
    Ok(())
}

/// A type 3 image whose mask has its own source: the mask is read first,
/// at once from a string or file, or by a loop frame that goes on to the
/// image's source when the mask is complete.
fn acquire_separate(
    i: &mut Interp,
    mut acquisition: ImageAcquisition,
    source: Object,
    operands: usize,
) -> Result<(), VmError> {
    let mask_source = acquisition.next_source().unwrap_or(source);
    let mask_kind = source_kind(mask_source).ok_or(VmError::TypeCheck)?;
    let data_kind = source_kind(source).ok_or(VmError::TypeCheck)?;
    if mask_kind == SourceKind::Immediate && acquisition.mask_pending() {
        let wanted = acquisition
            .mask
            .as_mut()
            .and_then(|m| m.pending())
            .map_or(0, |separate| separate.needed);
        let bytes = read_immediate(i, mask_source, wanted)?;
        acquisition.feed(&bytes);
        if let Some(separate) = acquisition.mask.as_mut().and_then(|m| m.pending()) {
            separate.done = true;
        }
    }
    if acquisition.mask_pending() || data_kind == SourceKind::Procedure {
        let body = acquisition.next_source().unwrap_or(source);
        drop_operands(i, operands)?;
        return i.push_frame(Frame::Loop(LoopFrame::ImageData {
            body,
            acquisition: Box::new(acquisition),
        }));
    }
    read_data_source(i, &mut acquisition, source)?;
    drop_operands(i, operands)?;
    finish(i, acquisition)
}

/// Reads a separate mask's image source, a string or file, after the
/// mask.
fn read_data_source(
    i: &mut Interp,
    acquisition: &mut ImageAcquisition,
    source: Object,
) -> Result<(), VmError> {
    if let Some(separate) = acquisition.mask.as_mut().and_then(|m| m.separate_mut()) {
        separate.data_read = true;
    }
    if let Some(base) = encoded_source(i, acquisition, &[source])? {
        return take_encoded(i, acquisition, source, base);
    }
    let data = read_immediate(i, source, acquisition.needed)?;
    acquisition.feed(&data);
    Ok(())
}

/// The file under a `DCTDecode` filter when the one source is such a
/// filter and the image takes samples (a mask cannot be DCT-encoded),
/// so the encoded bytes are read from there instead. A mask interleaved
/// with such samples cannot be separated from them: `limitcheck`.
fn encoded_source(
    i: &mut Interp,
    acquisition: &ImageAcquisition,
    sources: &[Object],
) -> Result<Option<Handle>, VmError> {
    let [source] = sources else {
        return Ok(None);
    };
    if source.ty() != Type::File || acquisition.is_planar() || acquisition.spec.is_mask {
        return Ok(None);
    }
    let handle = file_operand(*source, Access::ReadOnly)?;
    let files = i.mem.files();
    if !files.is_dct_layer(handle) {
        return Ok(None);
    }
    if acquisition
        .mask
        .as_ref()
        .is_some_and(|m| m.separate().is_none())
    {
        return Err(VmError::LimitCheck);
    }
    Ok(files.layer_base(handle))
}

/// The JPEG stream at `base`'s position, through its end-of-image
/// marker; a stream cut short by the source's end is what was read. A
/// byte that breaks the marker structure is `ioerror`.
fn read_encoded(i: &mut Interp, base: Handle) -> Result<Vec<u8>, VmError> {
    let mut walker = MarkerWalker::new();
    let mut data = Vec::new();
    let mut byte = [0u8; 1];
    while i.mem.files_mut().read(base, &mut byte)? == 1 {
        data.push(byte[0]);
        match walker.push(byte[0]) {
            Ok(Walk::More) => {}
            Ok(Walk::End) => break,
            Err(_) => return Err(VmError::IoError),
        }
    }
    Ok(data)
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
    if let Some(source) = acquisition.unread_data_source() {
        read_data_source(i, &mut acquisition, source)?;
    }
    let ImageAcquisition {
        mut spec,
        needed,
        mut data,
        cie,
        mask,
        ..
    } = acquisition;
    if let Some(mask) = mask {
        data = mask.resolve(&mut spec, data)?;
    } else if data.len() < needed {
        let row = spec.row_bytes().unwrap_or(0);
        let rows = data.len().checked_div(row).unwrap_or(0);
        data.truncate(rows * row);
        spec.height = u32::try_from(rows).map_err(|_| VmError::LimitCheck)?;
    }
    if let Some(space) = cie
        && spec.encoded.is_none()
        && !spec.is_mask
    {
        let job = cie::image_job(space, spec, &data, "image")?;
        return cie::start_job(i, job);
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

/// A sample space with the CIE space behind it, when there is one, and
/// whether that space collapsed.
type SampleSpace = (Option<SpaceSpec>, Option<(Rc<CieSpace>, bool)>);

/// The space the samples are in: none for a mask, DeviceGray for the
/// operand form (PLRM3 §4.10.5), the current colour space for the
/// dictionary form — with the CIE space behind it when there is one.
fn sample_space(
    i: &mut Interp,
    is_mask: bool,
    from_operands: bool,
) -> Result<SampleSpace, VmError> {
    if is_mask {
        Ok((None, None))
    } else if from_operands {
        Ok((Some(SpaceSpec::DeviceGray), None))
    } else {
        let space = i.backend()?.current_color_space();
        let cie = current_cie(i)?.map(|(_, space, collapsed)| (space, collapsed));
        Ok((Some(space), cie))
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
    let (color_space, _) = sample_space(i, is_mask, true)?;
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
            encoded: None,
            mask: None,
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

/// An image ready for its data: the spec, the image's source, the CIE
/// space the samples convert through, and a type 3 image's mask.
struct Prepared {
    spec: ImageSpec,
    source: Object,
    converting: Option<Rc<CieSpace>>,
    mask: Option<MaskAcquisition>,
}

/// What a type 1 dictionary describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    /// Samples in the current colour space.
    Image,
    /// `imagemask`'s stencil: one bit per sample.
    Stencil,
    /// A type 3 image's mask: one component of any depth, its source
    /// optional; the interleave type decides the rest.
    Mask,
}

/// The dictionary form: type 1 for both operators, types 3 and 4 for
/// `image` only. `MultipleDataSources` other than `false` is outside
/// what the backend accepts.
fn from_dict(i: &mut Interp, dict: Object, is_mask: bool) -> Result<Prepared, VmError> {
    let kind = match entry(i, dict, "ImageType")? {
        Some(kind) => kind.as_i32(),
        None => Some(1),
    };
    match (kind, is_mask) {
        (Some(1), _) => {
            let role = if is_mask { Role::Stencil } else { Role::Image };
            let (spec, source, converting) = type1(i, dict, role)?;
            Ok(Prepared {
                spec,
                source: source.ok_or(VmError::TypeCheck)?,
                converting,
                mask: None,
            })
        }
        (Some(3), false) => type3(i, dict),
        (Some(4), false) => type4(i, dict),
        _ => Err(VmError::RangeCheck),
    }
}

/// A type 4 dictionary: type 1 plus `MaskColor`, kept as a colour key.
fn type4(i: &mut Interp, dict: Object) -> Result<Prepared, VmError> {
    let (mut spec, source, converting) = type1(i, dict, Role::Image)?;
    let source = source.ok_or(VmError::TypeCheck)?;
    let colors = required(i, dict, "MaskColor")?;
    if !matches!(colors.ty(), Type::Array | Type::PackedArray) {
        return Err(VmError::TypeCheck);
    }
    let values: Vec<i32> = items(i, colors)?
        .into_iter()
        .map(|o| o.as_i32().ok_or(VmError::TypeCheck))
        .collect::<Result<_, _>>()?;
    let ranges = masked::key_ranges(&values, spec.components(), spec.bits_per_component)?;
    spec.mask = Some(ImageMask::ColorKey(ranges));
    Ok(Prepared {
        spec,
        source,
        converting,
        mask: None,
    })
}

/// A type 3 dictionary: its own entries, then each sub-dictionary as a
/// type 1 dictionary, then the rules between them (PLRM3 §4.10.6).
fn type3(i: &mut Interp, dict: Object) -> Result<Prepared, VmError> {
    let data_dict = sub_dictionary(i, dict, "DataDict")?;
    let mask_dict = sub_dictionary(i, dict, "MaskDict")?;
    let interleave = required(i, dict, "InterleaveType")?
        .as_i32()
        .ok_or(VmError::TypeCheck)?;
    if !(1..=3).contains(&interleave) {
        return Err(VmError::RangeCheck);
    }
    for sub in [data_dict, mask_dict] {
        if let Some(kind) = entry(i, sub, "ImageType")?
            && kind.as_i32() != Some(1)
        {
            return Err(VmError::TypeCheck);
        }
    }
    let (spec, source, converting) = type1(i, data_dict, Role::Image)?;
    let (mask_spec, mask_source, _) = type1(i, mask_dict, Role::Mask)?;
    let source = source.ok_or(VmError::TypeCheck)?;
    let mask = MaskAcquisition::new(&spec, &mask_spec, interleave, mask_source, source)?;
    Ok(Prepared {
        spec,
        source,
        converting,
        mask: Some(mask),
    })
}

fn sub_dictionary(i: &mut Interp, dict: Object, key: &str) -> Result<Object, VmError> {
    let sub = required(i, dict, key)?;
    if sub.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    Ok(sub)
}

/// A type 1 dictionary's spec, its data source when it has one, and the
/// CIE space its samples convert through.
type Type1 = (ImageSpec, Option<Object>, Option<Rc<CieSpace>>);

/// The Level 2 type 1 dictionary in `role`.
fn type1(i: &mut Interp, dict: Object, role: Role) -> Result<Type1, VmError> {
    if let Some(multiple) = entry(i, dict, "MultipleDataSources")?
        && multiple.as_bool() != Some(false)
    {
        return Err(VmError::TypeCheck);
    }
    let is_mask = role != Role::Image;
    let width = dimension(required(i, dict, "Width")?)?;
    let height = dimension(required(i, dict, "Height")?)?;
    let bits_per_component = bits(
        required(i, dict, "BitsPerComponent")?,
        role == Role::Stencil,
    )?;
    let matrix = required(i, dict, "ImageMatrix")?;
    let matrix = read_matrix(i, matrix)?;
    let source = entry(i, dict, "DataSource")?;
    if source.is_none() && role != Role::Mask {
        return Err(VmError::TypeCheck);
    }
    let interpolate = match entry(i, dict, "Interpolate")? {
        Some(flag) => flag.as_bool().ok_or(VmError::TypeCheck)?,
        None => false,
    };
    let (color_space, cie) = sample_space(i, is_mask, false)?;
    let components = match &cie {
        Some((space, _)) => space.components(),
        None => color_space.as_ref().map_or(1, SpaceSpec::components),
    };
    let decode = match (entry(i, dict, "Decode")?, &cie) {
        (Some(array), _) => decode_array(i, array, components)?,
        (None, Some((space, _))) => space.ranges().to_vec(),
        (None, None) => default_decode(color_space.as_ref(), bits_per_component, components),
    };
    let converting = cie.and_then(|(space, collapsed)| (!collapsed).then_some(space));
    // The samples of a converting space have the family's component
    // count, not the boundary space's three: until the conversion
    // replaces it, the device space of that count stands in.
    let color_space = match &converting {
        Some(space) => Some(match space.components() {
            1 => SpaceSpec::DeviceGray,
            3 => SpaceSpec::DeviceRGB,
            _ => SpaceSpec::DeviceCMYK,
        }),
        None => color_space,
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
            is_mask: role == Role::Stencil,
            encoded: None,
            mask: None,
        },
        source,
        converting,
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
            encoded: None,
            mask: None,
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

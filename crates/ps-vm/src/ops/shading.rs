// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Shading dictionaries (PLRM3 §4.9.3): the seven types read and checked
//! into a [`ShadingSpec`] the boundary carries. The common entries of
//! Table 4.11 come first — the colour space through the ordinary space
//! parser, with a pattern space refused, a CIE-based space that does not
//! collapse to a calibrated one an implementation limit, and an Indexed
//! space allowed only for a mesh without a function — then the type's
//! own entries per Tables 4.12–4.19.
//!
//! Mesh data (types 4 to 7) is carried in one form: the packed bit
//! stream the manual describes for a string or file source, each vertex
//! or patch padded to a byte. An array source is walked as numbers and
//! re-encoded into that form at 32 bits per coordinate, 16 per colour
//! value, and 8 per flag, with a `Decode` array made from each column's
//! own extent (a constant column decodes from `[v v+1]`, so its zero
//! codes give exactly `v`; a parametric value under a `Function` is
//! clipped to the unit interval and decodes from `[0 1]`). Whichever
//! form the data came in, it is walked once to check the structure the
//! tables require: whole vertices, whole triangles after a flag of 0 in
//! a free-form mesh, whole rows and at least two of them in a lattice,
//! whole patches with a flag of 0 first in a patch mesh; a violation is
//! `rangecheck`. The same walk decodes the data back into elements for
//! whoever needs the values.
//!
//! `shfill` hands the checked value to the backend with nothing else:
//! the backend takes the CTM as the placement. The readers keep no
//! state, so a data source that is the job's own file starving mid-read
//! fails the operator with `NeedMore`, its operand still on the stack,
//! and the loop runs it again once bytes arrive (as `image` does).
//! `setsmoothness` and `currentsmoothness` keep the graphics-state
//! parameter of the same section.

use crate::error::VmError;
use crate::graphics::{FunctionSpec, Matrix, ShadingKind, ShadingSpec, SpaceSpec};
use crate::interp::Interp;
use crate::object::{Access, Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::file::file_operand;
use crate::ops::function::{entry, integer, numbers, numbers_of, read_function_or_array, required};
use crate::ops::graphics::{
    drop, is_array, num_at, parse_space, push_real, read_bounds, read_matrix,
};
use crate::ops::pattern;

op_table! { graphics OPS {
    "shfill" => shfill, [Dict];
    "setsmoothness" => setsmoothness, [Num];
    "currentsmoothness" => currentsmoothness;
}}

/// `dict shfill`: the shading painted in current user space under the
/// clip; the path and colour stay. Undefined inside an uncoloured cell,
/// which may set no colour of its own (PLRM3 §4.9.2), as `image` is.
fn shfill(i: &mut Interp) -> Result<(), VmError> {
    pattern::colour_allowed(i)?;
    let spec = read_shading(i, i.peek(0)?)?;
    i.backend()?.shade(&spec)?;
    drop(i, 1)
}

/// `num setsmoothness`: a value outside the unit interval is replaced by
/// the nearest end without an error.
fn setsmoothness(i: &mut Interp) -> Result<(), VmError> {
    let value = num_at(i, 0)?;
    let value = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    i.backend()?.set_smoothness(value)?;
    drop(i, 1)
}

fn currentsmoothness(i: &mut Interp) -> Result<(), VmError> {
    let value = i.backend()?.smoothness();
    push_real(i, value)
}

/// The most bytes of mesh data one shading carries, and the most
/// numbers an array source may hold; beyond either `limitcheck`.
const MAX_MESH_BYTES: usize = 1 << 26;

const COORDINATE_DEPTHS: [i32; 8] = [1, 2, 4, 8, 12, 16, 24, 32];
const COMPONENT_DEPTHS: [i32; 6] = [1, 2, 4, 8, 12, 16];
const FLAG_DEPTHS: [i32; 3] = [2, 4, 8];

/// The depths an array source is re-encoded at.
const ARRAY_COORDINATE_BITS: u8 = 32;
const ARRAY_COMPONENT_BITS: u8 = 16;
const ARRAY_FLAG_BITS: u8 = 8;

/// Reads and checks a shading dictionary.
pub fn read_shading(i: &mut Interp, dict: Object) -> Result<ShadingSpec, VmError> {
    if dict.ty() != Type::Dict {
        return Err(VmError::TypeCheck);
    }
    let ty = required(i, dict, "ShadingType")?
        .as_i32()
        .ok_or(VmError::TypeCheck)?;
    if !(1..=7).contains(&ty) {
        return Err(VmError::RangeCheck);
    }
    let ty = ty as u8;
    let space = required(i, dict, "ColorSpace")?;
    let space = shading_space(i, space)?;
    let components = space.components();
    let background = numbers_of(i, dict, "Background", components)?;
    let bbox = match entry(i, dict, "BBox")? {
        Some(bbox) => Some(read_bounds(i, bbox)?),
        None => None,
    };
    let antialias = match entry(i, dict, "AntiAlias")? {
        Some(flag) => flag.as_bool().ok_or(VmError::TypeCheck)?,
        None => false,
    };
    let function = entry(i, dict, "Function")?;
    if matches!(space, SpaceSpec::Indexed { .. }) && (ty <= 3 || function.is_some()) {
        return Err(VmError::RangeCheck);
    }
    let kind = match ty {
        1 => function_based(i, dict, components)?,
        2 | 3 => axial_or_radial(i, dict, ty, components)?,
        _ => mesh(i, dict, ty, components, function)?,
    };
    Ok(ShadingSpec {
        kind,
        space,
        background,
        bbox,
        antialias,
    })
}

/// The `ColorSpace` entry: any space the parser accepts but a pattern
/// space (`rangecheck`) or one carried as `Lab` — a CIE-based space that
/// did not collapse, whose colours would need the function evaluated to
/// convert, so `limitcheck` — as itself or as the base of an Indexed
/// space.
fn shading_space(i: &mut Interp, object: Object) -> Result<SpaceSpec, VmError> {
    let space = parse_space(i, object, 0)?;
    match &space {
        SpaceSpec::Pattern { .. } => Err(VmError::RangeCheck),
        SpaceSpec::Lab { .. } => Err(VmError::LimitCheck),
        SpaceSpec::Indexed { base, .. } if matches!(**base, SpaceSpec::Lab { .. }) => {
            Err(VmError::LimitCheck)
        }
        _ => Ok(space),
    }
}

// Table 4.12.
fn function_based(i: &mut Interp, dict: Object, components: usize) -> Result<ShadingKind, VmError> {
    let domain = numbers_of(i, dict, "Domain", 4)?.unwrap_or(vec![0.0, 1.0, 0.0, 1.0]);
    if !(domain[0] <= domain[1] && domain[2] <= domain[3]) {
        return Err(VmError::RangeCheck);
    }
    let matrix = match entry(i, dict, "Matrix")? {
        Some(matrix) => read_matrix(i, matrix)?,
        None => Matrix::IDENTITY,
    };
    let function = required(i, dict, "Function")?;
    let function = read_function_or_array(i, function, 2, components)?;
    Ok(ShadingKind::Function {
        domain: domain.try_into().expect("four numbers"),
        matrix,
        function,
    })
}

// Tables 4.13 and 4.14.
fn axial_or_radial(
    i: &mut Interp,
    dict: Object,
    ty: u8,
    components: usize,
) -> Result<ShadingKind, VmError> {
    let count = if ty == 2 { 4 } else { 6 };
    let coords = required(i, dict, "Coords")?;
    let coords = numbers(i, coords)?;
    if coords.len() != count {
        return Err(VmError::RangeCheck);
    }
    if ty == 3 && !(coords[2] >= 0.0 && coords[5] >= 0.0) {
        return Err(VmError::RangeCheck);
    }
    let domain = numbers_of(i, dict, "Domain", 2)?.unwrap_or(vec![0.0, 1.0]);
    let extend = match entry(i, dict, "Extend")? {
        None => [false, false],
        Some(extend) => {
            if !is_array(extend) {
                return Err(VmError::TypeCheck);
            }
            let flags: Vec<bool> = items(i, extend)?
                .into_iter()
                .map(|o| o.as_bool().ok_or(VmError::TypeCheck))
                .collect::<Result<_, _>>()?;
            flags.try_into().map_err(|_| VmError::RangeCheck)?
        }
    };
    let function = required(i, dict, "Function")?;
    let function = read_function_or_array(i, function, 1, components)?;
    let domain = domain.try_into().expect("two numbers");
    Ok(if ty == 2 {
        ShadingKind::Axial {
            coords: coords.try_into().expect("four numbers"),
            domain,
            function,
            extend,
        }
    } else {
        ShadingKind::Radial {
            coords: coords.try_into().expect("six numbers"),
            domain,
            function,
            extend,
        }
    })
}

/// A required bit-depth entry whose value must be one of `allowed`.
fn depth(i: &mut Interp, dict: Object, key: &str, allowed: &[i32]) -> Result<u8, VmError> {
    let value = integer(i, dict, key)?.ok_or(VmError::Undefined)?;
    if !allowed.contains(&value) {
        return Err(VmError::RangeCheck);
    }
    Ok(value as u8)
}

// Tables 4.15–4.19.
fn mesh(
    i: &mut Interp,
    dict: Object,
    ty: u8,
    components: usize,
    function: Option<Object>,
) -> Result<ShadingKind, VmError> {
    let source = required(i, dict, "DataSource")?;
    let function = match function {
        Some(function) => read_function_or_array(i, function, 1, components)?,
        None => Vec::new(),
    };
    let values = if function.is_empty() { components } else { 1 };
    let vertices_per_row = if ty == 5 {
        let per_row = integer(i, dict, "VerticesPerRow")?.ok_or(VmError::Undefined)?;
        if per_row < 2 {
            return Err(VmError::RangeCheck);
        }
        Some(per_row as u32)
    } else {
        None
    };
    let grammar = Grammar {
        ty,
        values,
        vertices_per_row,
    };
    match source.ty() {
        Type::Array | Type::PackedArray => {
            let elements = items(i, source)?;
            if elements.len() > MAX_MESH_BYTES {
                return Err(VmError::LimitCheck);
            }
            from_array(&grammar, elements, !function.is_empty(), function)
        }
        Type::String | Type::File => {
            let bits_per_coordinate = depth(i, dict, "BitsPerCoordinate", &COORDINATE_DEPTHS)?;
            let bits_per_component = depth(i, dict, "BitsPerComponent", &COMPONENT_DEPTHS)?;
            let bits_per_flag = if ty == 5 {
                0
            } else {
                depth(i, dict, "BitsPerFlag", &FLAG_DEPTHS)?
            };
            let decode = required(i, dict, "Decode")?;
            let decode = numbers(i, decode)?;
            if decode.len() != 4 + 2 * values {
                return Err(VmError::RangeCheck);
            }
            let data = read_all(i, source)?;
            let layout = Layout {
                bits_per_coordinate,
                bits_per_component,
                bits_per_flag,
                decode: &decode,
            };
            let mut bits = Bits::new(&data, layout);
            walk(&grammar, &mut bits, &mut |_| {})?;
            Ok(ShadingKind::Mesh {
                ty,
                bits_per_coordinate,
                bits_per_component,
                bits_per_flag,
                decode,
                vertices_per_row,
                function,
                data,
            })
        }
        _ => Err(VmError::TypeCheck),
    }
}

/// The whole of a string, or a file to its end — from its start when it
/// can be positioned, else from where it is.
fn read_all(i: &mut Interp, source: Object) -> Result<Vec<u8>, VmError> {
    if source.ty() == Type::String {
        let data = bytes(i, source)?;
        if data.len() > MAX_MESH_BYTES {
            return Err(VmError::LimitCheck);
        }
        return Ok(data);
    }
    let handle = file_operand(source, Access::ReadOnly)?;
    let files = i.mem.files_mut();
    if !files.is_open(handle) {
        return Err(VmError::IoError);
    }
    if files.is_positionable(handle) {
        files.set_file_position(handle, 0)?;
    }
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let got = files.read(handle, &mut buf)?;
        if got == 0 {
            return Ok(data);
        }
        if data.len() + got > MAX_MESH_BYTES {
            return Err(VmError::LimitCheck);
        }
        data.extend_from_slice(&buf[..got]);
    }
}

/// One element of a mesh's data.
#[derive(Clone, Debug, PartialEq)]
pub enum MeshElement {
    /// A vertex of a triangle mesh with its edge flag (0 in a lattice,
    /// and as given — though ignored — for the second and third
    /// vertices of a new triangle), its coordinates, and its colour
    /// values.
    Vertex {
        flag: u8,
        x: f32,
        y: f32,
        color: Vec<f32>,
    },
    /// A patch with its edge flag, the control points the data gives
    /// explicitly (12 or 16 for a new patch, 8 or 12 for one sharing an
    /// edge), and the corner colours given explicitly (4 or 2).
    Patch {
        flag: u8,
        points: Vec<(f32, f32)>,
        color: Vec<Vec<f32>>,
    },
}

/// Decodes a mesh shading's data into its elements; empty for the other
/// types.
pub fn mesh_elements(spec: &ShadingSpec) -> Result<Vec<MeshElement>, VmError> {
    let ShadingKind::Mesh {
        ty,
        bits_per_coordinate,
        bits_per_component,
        bits_per_flag,
        decode,
        vertices_per_row,
        data,
        ..
    } = &spec.kind
    else {
        return Ok(Vec::new());
    };
    let grammar = Grammar {
        ty: *ty,
        values: spec.values_per_color(),
        vertices_per_row: *vertices_per_row,
    };
    let layout = Layout {
        bits_per_coordinate: *bits_per_coordinate,
        bits_per_component: *bits_per_component,
        bits_per_flag: *bits_per_flag,
        decode,
    };
    let mut bits = Bits::new(data, layout);
    let mut elements = Vec::new();
    walk(&grammar, &mut bits, &mut |element| elements.push(element))?;
    Ok(elements)
}

/// What the data of a mesh type consists of.
struct Grammar {
    ty: u8,
    /// Colour values per vertex or corner.
    values: usize,
    vertices_per_row: Option<u32>,
}

/// Where a mesh's values come from: the packed bit stream or an array
/// of numbers.
trait MeshSource {
    fn at_end(&self) -> bool;
    /// An edge flag, 0 to 3.
    fn flag(&mut self) -> Result<u8, VmError>;
    /// The x (`axis` 0) or y (1) coordinate.
    fn coordinate(&mut self, axis: usize) -> Result<f32, VmError>;
    /// Colour value `k` of a vertex or corner.
    fn component(&mut self, k: usize) -> Result<f32, VmError>;
    /// The vertex or patch is complete.
    fn end_element(&mut self);
}

fn walk<S: MeshSource>(
    grammar: &Grammar,
    source: &mut S,
    sink: &mut impl FnMut(MeshElement),
) -> Result<(), VmError> {
    let vertex = |source: &mut S, flag: u8| -> Result<MeshElement, VmError> {
        let x = source.coordinate(0)?;
        let y = source.coordinate(1)?;
        let color = (0..grammar.values)
            .map(|k| source.component(k))
            .collect::<Result<_, _>>()?;
        source.end_element();
        Ok(MeshElement::Vertex { flag, x, y, color })
    };
    match grammar.ty {
        4 => {
            // A flag of 0 starts a triangle that two more vertices, whose
            // flags do not count, must complete; 1 and 2 extend the
            // previous triangle by one vertex.
            let mut pending = 0;
            let mut any = false;
            while !source.at_end() {
                let flag = source.flag()?;
                if pending > 0 {
                    pending -= 1;
                } else {
                    match flag {
                        0 => pending = 2,
                        1 | 2 if any => {}
                        _ => return Err(VmError::RangeCheck),
                    }
                }
                any = true;
                sink(vertex(source, flag)?);
            }
            if pending > 0 {
                return Err(VmError::RangeCheck);
            }
        }
        5 => {
            let per_row = grammar.vertices_per_row.unwrap_or(2) as usize;
            let mut count = 0usize;
            while !source.at_end() {
                sink(vertex(source, 0)?);
                count += 1;
            }
            if !count.is_multiple_of(per_row) || count / per_row < 2 {
                return Err(VmError::RangeCheck);
            }
        }
        _ => {
            let full = if grammar.ty == 6 { 12 } else { 16 };
            let mut any = false;
            while !source.at_end() {
                let flag = source.flag()?;
                if !any && flag != 0 {
                    return Err(VmError::RangeCheck);
                }
                let (points, corners) = if flag == 0 { (full, 4) } else { (full - 4, 2) };
                let points = (0..points)
                    .map(|_| Ok((source.coordinate(0)?, source.coordinate(1)?)))
                    .collect::<Result<Vec<_>, VmError>>()?;
                let color = (0..corners)
                    .map(|_| {
                        (0..grammar.values)
                            .map(|k| source.component(k))
                            .collect::<Result<Vec<_>, _>>()
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                source.end_element();
                any = true;
                sink(MeshElement::Patch {
                    flag,
                    points,
                    color,
                });
            }
            if !any {
                return Err(VmError::RangeCheck);
            }
        }
    }
    Ok(())
}

/// The bit depths and decode ranges of packed data.
#[derive(Clone, Copy)]
struct Layout<'a> {
    bits_per_coordinate: u8,
    bits_per_component: u8,
    bits_per_flag: u8,
    decode: &'a [f32],
}

/// The largest code `bits` wide, as a real.
fn code_max(bits: u8) -> f64 {
    (1u64 << bits) as f64 - 1.0
}

/// A code's value on the decode range `lo..=hi`.
fn decode_value(code: u32, bits: u8, lo: f32, hi: f32) -> f32 {
    (f64::from(lo) + f64::from(code) * (f64::from(hi) - f64::from(lo)) / code_max(bits)) as f32
}

/// The code `bits` wide nearest `value` on the range `lo..=hi`.
fn encode_value(value: f32, bits: u8, lo: f32, hi: f32) -> u32 {
    let unit =
        ((f64::from(value) - f64::from(lo)) / (f64::from(hi) - f64::from(lo))).clamp(0.0, 1.0);
    (unit * code_max(bits)).round() as u32
}

/// The packed bit stream, most significant bit first, read a value at a
/// time.
struct Bits<'a> {
    data: &'a [u8],
    bit: usize,
    layout: Layout<'a>,
}

impl<'a> Bits<'a> {
    fn new(data: &'a [u8], layout: Layout<'a>) -> Self {
        Bits {
            data,
            bit: 0,
            layout,
        }
    }

    /// The next `bits` bits; `rangecheck` past the data's end.
    fn take(&mut self, bits: u8) -> Result<u32, VmError> {
        let end = self.bit + usize::from(bits);
        if end > self.data.len() * 8 {
            return Err(VmError::RangeCheck);
        }
        let mut value = 0u32;
        for at in self.bit..end {
            let byte = self.data[at / 8];
            value = (value << 1) | u32::from((byte >> (7 - at % 8)) & 1);
        }
        self.bit = end;
        Ok(value)
    }
}

impl MeshSource for Bits<'_> {
    fn at_end(&self) -> bool {
        self.bit >= self.data.len() * 8
    }

    fn flag(&mut self) -> Result<u8, VmError> {
        Ok((self.take(self.layout.bits_per_flag)? & 3) as u8)
    }

    fn coordinate(&mut self, axis: usize) -> Result<f32, VmError> {
        let bits = self.layout.bits_per_coordinate;
        let code = self.take(bits)?;
        let decode = self.layout.decode;
        Ok(decode_value(
            code,
            bits,
            decode[2 * axis],
            decode[2 * axis + 1],
        ))
    }

    fn component(&mut self, k: usize) -> Result<f32, VmError> {
        let bits = self.layout.bits_per_component;
        let code = self.take(bits)?;
        let decode = self.layout.decode;
        Ok(decode_value(
            code,
            bits,
            decode[4 + 2 * k],
            decode[5 + 2 * k],
        ))
    }

    fn end_element(&mut self) {
        self.bit = self.bit.div_ceil(8) * 8;
    }
}

/// An array source: numbers in order, flags as integers.
struct Numbers {
    items: Vec<Object>,
    at: usize,
    /// Clip colour values to the unit interval: they are parameters of
    /// a function.
    clip: bool,
}

impl Numbers {
    fn next(&mut self) -> Result<Object, VmError> {
        let item = self
            .items
            .get(self.at)
            .copied()
            .ok_or(VmError::RangeCheck)?;
        self.at += 1;
        Ok(item)
    }

    fn number(&mut self) -> Result<f32, VmError> {
        self.next()?.as_number().ok_or(VmError::TypeCheck)
    }
}

impl MeshSource for Numbers {
    fn at_end(&self) -> bool {
        self.at >= self.items.len()
    }

    fn flag(&mut self) -> Result<u8, VmError> {
        let flag = self.next()?.as_i32().ok_or(VmError::TypeCheck)?;
        u8::try_from(flag)
            .ok()
            .filter(|flag| *flag <= 3)
            .ok_or(VmError::RangeCheck)
    }

    fn coordinate(&mut self, _: usize) -> Result<f32, VmError> {
        self.number()
    }

    fn component(&mut self, _: usize) -> Result<f32, VmError> {
        let value = self.number()?;
        Ok(if self.clip {
            value.clamp(0.0, 1.0)
        } else {
            value
        })
    }

    fn end_element(&mut self) {}
}

/// Writes values most significant bit first.
#[derive(Default)]
struct BitWriter {
    bytes: Vec<u8>,
    /// Bits used in the last byte, 0 when it is full or absent.
    used: u8,
}

impl BitWriter {
    fn put(&mut self, value: u32, bits: u8) {
        for k in (0..bits).rev() {
            let bit = ((value >> k) & 1) as u8;
            if self.used == 0 {
                self.bytes.push(0);
            }
            let last = self.bytes.last_mut().expect("pushed above");
            *last |= bit << (7 - self.used);
            self.used = (self.used + 1) % 8;
        }
    }

    fn align(&mut self) {
        self.used = 0;
    }
}

/// An array source walked, its extents taken, and its values packed.
fn from_array(
    grammar: &Grammar,
    items: Vec<Object>,
    clip: bool,
    function: Vec<FunctionSpec>,
) -> Result<ShadingKind, VmError> {
    let mut source = Numbers { items, at: 0, clip };
    let mut elements = Vec::new();
    walk(grammar, &mut source, &mut |element| elements.push(element))?;
    let columns = 2 + grammar.values;
    let mut lo = vec![f32::INFINITY; columns];
    let mut hi = vec![f32::NEG_INFINITY; columns];
    let mut note = |column: usize, value: f32| {
        lo[column] = lo[column].min(value);
        hi[column] = hi[column].max(value);
    };
    for element in &elements {
        match element {
            MeshElement::Vertex { x, y, color, .. } => {
                note(0, *x);
                note(1, *y);
                for (k, &value) in color.iter().enumerate() {
                    note(2 + k, value);
                }
            }
            MeshElement::Patch { points, color, .. } => {
                for &(x, y) in points {
                    note(0, x);
                    note(1, y);
                }
                for corner in color {
                    for (k, &value) in corner.iter().enumerate() {
                        note(2 + k, value);
                    }
                }
            }
        }
    }
    // A parametric column decodes from the unit interval it was clipped
    // to; a column with no values (an empty mesh) from it too; a
    // constant column from a range one wide so its zero codes give the
    // value exactly.
    let decode: Vec<f32> = (0..columns)
        .flat_map(|column| {
            let (lo, hi) = (lo[column], hi[column]);
            if (clip && column >= 2) || lo > hi {
                [0.0, 1.0]
            } else if lo == hi {
                [lo, lo + 1.0]
            } else {
                [lo, hi]
            }
        })
        .collect();
    let mut writer = BitWriter::default();
    let coordinate = |writer: &mut BitWriter, axis: usize, value: f32| {
        let code = encode_value(
            value,
            ARRAY_COORDINATE_BITS,
            decode[2 * axis],
            decode[2 * axis + 1],
        );
        writer.put(code, ARRAY_COORDINATE_BITS);
    };
    let component = |writer: &mut BitWriter, k: usize, value: f32| {
        let code = encode_value(
            value,
            ARRAY_COMPONENT_BITS,
            decode[4 + 2 * k],
            decode[5 + 2 * k],
        );
        writer.put(code, ARRAY_COMPONENT_BITS);
    };
    for element in &elements {
        match element {
            MeshElement::Vertex { flag, x, y, color } => {
                if grammar.ty != 5 {
                    writer.put(u32::from(*flag), ARRAY_FLAG_BITS);
                }
                coordinate(&mut writer, 0, *x);
                coordinate(&mut writer, 1, *y);
                for (k, &value) in color.iter().enumerate() {
                    component(&mut writer, k, value);
                }
            }
            MeshElement::Patch {
                flag,
                points,
                color,
            } => {
                writer.put(u32::from(*flag), ARRAY_FLAG_BITS);
                for &(x, y) in points {
                    coordinate(&mut writer, 0, x);
                    coordinate(&mut writer, 1, y);
                }
                for corner in color {
                    for (k, &value) in corner.iter().enumerate() {
                        component(&mut writer, k, value);
                    }
                }
            }
        }
        writer.align();
    }
    Ok(ShadingKind::Mesh {
        ty: grammar.ty,
        bits_per_coordinate: ARRAY_COORDINATE_BITS,
        bits_per_component: ARRAY_COMPONENT_BITS,
        bits_per_flag: if grammar.ty == 5 { 0 } else { ARRAY_FLAG_BITS },
        decode,
        vertices_per_row: grammar.vertices_per_row,
        function,
        data: writer.bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_round_trip_through_their_ranges() {
        assert_eq!(decode_value(0, 8, -1.0, 1.0), -1.0);
        assert_eq!(decode_value(255, 8, -1.0, 1.0), 1.0);
        assert_eq!(decode_value(u32::MAX, 32, 0.0, 10.0), 10.0);
        assert_eq!(encode_value(10.0, 32, 0.0, 10.0), u32::MAX);
        assert_eq!(encode_value(-5.0, 16, 0.0, 1.0), 0);
        assert_eq!(encode_value(0.5, 1, 0.0, 1.0), 1);
        for value in [0.0f32, 0.25, 0.3333, 0.9, 1.0] {
            let code = encode_value(value, 16, 0.0, 1.0);
            assert!((decode_value(code, 16, 0.0, 1.0) - value).abs() < 1e-4);
        }
    }

    #[test]
    fn the_bit_writer_packs_most_significant_first_and_pads() {
        let mut w = BitWriter::default();
        w.put(0b10, 2);
        w.put(0xABC, 12);
        w.align();
        w.put(1, 1);
        assert_eq!(w.bytes, [0b1010_1010, 0b1111_0000, 0b1000_0000]);
        let layout = Layout {
            bits_per_coordinate: 12,
            bits_per_component: 1,
            bits_per_flag: 2,
            decode: &[0.0, 4095.0, 0.0, 1.0, 0.0, 1.0],
        };
        let mut bits = Bits::new(&w.bytes, layout);
        assert_eq!(bits.flag(), Ok(2));
        assert_eq!(bits.coordinate(0), Ok(0xABC as f32));
        bits.end_element();
        assert_eq!(bits.component(0), Ok(1.0));
        assert!(!bits.at_end());
        bits.end_element();
        assert!(bits.at_end());
        assert_eq!(bits.take(1), Err(VmError::RangeCheck));
    }
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Graphics operators (PLRM3 §4, §8.2): the operand side of the
//! `GraphicsBackend` boundary. Each operator checks and pops its operands
//! and calls the backend, which owns all graphics state. What stays on
//! this side is pure computation on operands: arithmetic on matrix
//! operands, the conversions behind the device-space colour operators,
//! and colour-space parsing. The group enters `systemdict` only when a
//! backend is installed.

use crate::error::VmError;
use crate::graphics::{Bounds, LineCap, LineJoin, Matrix, Point, Rect, SpaceSpec};
use crate::interp::{Interp, scan_error};
use crate::object::{Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::{image, output};
use crate::scanner::scan_all;

op_table! { graphics OPS {
    "gsave" => gsave;
    "grestore" => grestore;
    "grestoreall" => grestoreall;
    "initgraphics" => initgraphics;
    "setlinewidth" => setlinewidth, [Num];
    "currentlinewidth" => currentlinewidth;
    "setlinecap" => setlinecap, [Int];
    "currentlinecap" => currentlinecap;
    "setlinejoin" => setlinejoin, [Int];
    "currentlinejoin" => currentlinejoin;
    "setmiterlimit" => setmiterlimit, [Num];
    "currentmiterlimit" => currentmiterlimit;
    "setdash" => setdash, [Array, Num];
    "currentdash" => currentdash;
    "setflat" => setflat, [Num];
    "currentflat" => currentflat;
    "matrix" => matrix;
    "identmatrix" => identmatrix, [Array];
    "defaultmatrix" => defaultmatrix, [Array];
    "initmatrix" => initmatrix;
    "currentmatrix" => currentmatrix, [Array];
    "setmatrix" => setmatrix, [Array];
    "translate" => translate;
    "scale" => scale;
    "rotate" => rotate;
    "concat" => concat, [Array];
    "concatmatrix" => concatmatrix, [Array, Array, Array];
    "transform" => transform;
    "itransform" => itransform;
    "dtransform" => dtransform;
    "idtransform" => idtransform;
    "invertmatrix" => invertmatrix, [Array, Array];
    "newpath" => newpath;
    "moveto" => moveto, [Num, Num];
    "rmoveto" => rmoveto, [Num, Num];
    "lineto" => lineto, [Num, Num];
    "rlineto" => rlineto, [Num, Num];
    "curveto" => curveto, [Num, Num, Num, Num, Num, Num];
    "rcurveto" => rcurveto, [Num, Num, Num, Num, Num, Num];
    "arc" => arc, [Num, Num, Num, Num, Num];
    "arcn" => arcn, [Num, Num, Num, Num, Num];
    "arcto" => arcto, [Num, Num, Num, Num, Num];
    "closepath" => closepath;
    "currentpoint" => currentpoint;
    "pathbbox" => pathbbox;
    "fill" => fill;
    "eofill" => eofill;
    "stroke" => stroke;
    "clip" => clip;
    "eoclip" => eoclip;
    "initclip" => initclip;
    "clippath" => clippath;
    "rectfill" => rectfill;
    "rectstroke" => rectstroke;
    "rectclip" => rectclip;
    "setgray" => setgray, [Num];
    "currentgray" => currentgray;
    "setrgbcolor" => setrgbcolor, [Num, Num, Num];
    "currentrgbcolor" => currentrgbcolor;
    "sethsbcolor" => sethsbcolor, [Num, Num, Num];
    "currenthsbcolor" => currenthsbcolor;
    "setcmykcolor" => setcmykcolor, [Num, Num, Num, Num];
    "currentcmykcolor" => currentcmykcolor;
    "setcolorspace" => setcolorspace, [Any];
    "currentcolorspace" => currentcolorspace;
    "setcolor" => setcolor;
    "currentcolor" => currentcolor;
    "image" => image::image, [Any];
    "imagemask" => image::imagemask, [Any];
    "showpage" => showpage;
    "copypage" => copypage;
    "erasepage" => erasepage;
    "nulldevice" => nulldevice;
}}

// --- operand helpers ---------------------------------------------------------
//
// Operands are read in place and popped only after the backend accepted
// the call, so a failing operator leaves them on the stack.

fn num_at(i: &Interp, n: usize) -> Result<f32, VmError> {
    i.peek(n)?.as_number().ok_or(VmError::TypeCheck)
}

/// The point whose `y` is `n` below the top and `x` just beneath it.
fn point_at(i: &Interp, n: usize) -> Result<Point, VmError> {
    Ok(Point::new(num_at(i, n + 1)?, num_at(i, n)?))
}

fn drop(i: &mut Interp, count: usize) -> Result<(), VmError> {
    for _ in 0..count {
        i.pop()?;
    }
    Ok(())
}

fn push_real(i: &mut Interp, value: f32) -> Result<(), VmError> {
    i.push(Object::real(value))
}

fn push_point(i: &mut Interp, p: Point) -> Result<(), VmError> {
    push_real(i, p.x)?;
    push_real(i, p.y)
}

fn is_array(object: Object) -> bool {
    matches!(object.ty(), Type::Array | Type::PackedArray)
}

/// The six numbers of a matrix operand.
pub(crate) fn read_matrix(i: &Interp, object: Object) -> Result<Matrix, VmError> {
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let items = items(i, object)?;
    if items.len() != 6 {
        return Err(VmError::RangeCheck);
    }
    let mut elements = [0.0; 6];
    for (slot, item) in elements.iter_mut().zip(items) {
        *slot = item.as_number().ok_or(VmError::TypeCheck)?;
    }
    Ok(Matrix(elements))
}

/// Stores `matrix` into the array operand and returns it.
fn write_matrix(i: &mut Interp, object: Object, matrix: Matrix) -> Result<Object, VmError> {
    if object.ty() != Type::Array {
        return Err(VmError::TypeCheck);
    }
    if object.length() != Some(6) {
        return Err(VmError::RangeCheck);
    }
    let values: Vec<Object> = matrix.0.iter().map(|&v| Object::real(v)).collect();
    i.mem.array_put_items(object, 0, &values)?;
    Ok(object)
}

fn reals_array(i: &mut Interp, values: &[f32]) -> Result<Object, VmError> {
    let items = values.iter().map(|&v| Object::real(v)).collect();
    i.mem.alloc_array(items)
}

// --- graphics-state stack -------------------------------------------------------

fn gsave(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.gsave()
}

// `grestore` never pops the state a `save` left on the stack: at that
// floor it restores the state and leaves it in place, which is a pop
// followed by a push of the same state.
fn grestore(i: &mut Interp) -> Result<(), VmError> {
    let floor = i.gstate_floor();
    let backend = i.backend()?;
    let depth = backend.gstate_depth();
    if depth > floor {
        backend.grestore()
    } else if depth > 0 {
        backend.grestore()?;
        backend.gsave()
    } else {
        Ok(())
    }
}

fn grestoreall(i: &mut Interp) -> Result<(), VmError> {
    let floor = i.gstate_floor();
    let backend = i.backend()?;
    if backend.gstate_depth() > floor {
        backend.grestore_to(floor)?;
    }
    if floor > 0 && backend.gstate_depth() == floor {
        backend.grestore()?;
        backend.gsave()?;
    }
    Ok(())
}

fn initgraphics(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.initgraphics()
}

// --- line parameters ---------------------------------------------------------------

fn setlinewidth(i: &mut Interp) -> Result<(), VmError> {
    let width = num_at(i, 0)?;
    i.backend()?.set_line_width(width)?;
    drop(i, 1)
}

fn currentlinewidth(i: &mut Interp) -> Result<(), VmError> {
    let width = i.backend()?.line_width();
    push_real(i, width)
}

fn setlinecap(i: &mut Interp) -> Result<(), VmError> {
    let code = i.peek(0)?.as_i32().expect("integer");
    let cap = LineCap::from_code(code).ok_or(VmError::RangeCheck)?;
    i.backend()?.set_line_cap(cap)?;
    drop(i, 1)
}

fn currentlinecap(i: &mut Interp) -> Result<(), VmError> {
    let cap = i.backend()?.line_cap();
    i.push(Object::integer(cap.code()))
}

fn setlinejoin(i: &mut Interp) -> Result<(), VmError> {
    let code = i.peek(0)?.as_i32().expect("integer");
    let join = LineJoin::from_code(code).ok_or(VmError::RangeCheck)?;
    i.backend()?.set_line_join(join)?;
    drop(i, 1)
}

fn currentlinejoin(i: &mut Interp) -> Result<(), VmError> {
    let join = i.backend()?.line_join();
    i.push(Object::integer(join.code()))
}

fn setmiterlimit(i: &mut Interp) -> Result<(), VmError> {
    let limit = num_at(i, 0)?;
    if limit < 1.0 {
        return Err(VmError::RangeCheck);
    }
    i.backend()?.set_miter_limit(limit)?;
    drop(i, 1)
}

fn currentmiterlimit(i: &mut Interp) -> Result<(), VmError> {
    let limit = i.backend()?.miter_limit();
    push_real(i, limit)
}

fn setdash(i: &mut Interp) -> Result<(), VmError> {
    let phase = num_at(i, 0)?;
    let array = i.peek(1)?;
    let lengths: Vec<f32> = items(i, array)?
        .into_iter()
        .map(|o| o.as_number().ok_or(VmError::TypeCheck))
        .collect::<Result<_, _>>()?;
    if lengths.iter().any(|&l| l < 0.0)
        || (!lengths.is_empty() && lengths.iter().all(|&l| l == 0.0))
    {
        return Err(VmError::RangeCheck);
    }
    i.backend()?.set_dash(&lengths, phase)?;
    drop(i, 2)
}

fn currentdash(i: &mut Interp) -> Result<(), VmError> {
    let (lengths, phase) = i.backend()?.dash();
    let array = reals_array(i, &lengths)?;
    i.push(array)?;
    push_real(i, phase)
}

fn setflat(i: &mut Interp) -> Result<(), VmError> {
    let flatness = num_at(i, 0)?;
    i.backend()?.set_flatness(flatness)?;
    drop(i, 1)
}

fn currentflat(i: &mut Interp) -> Result<(), VmError> {
    let flatness = i.backend()?.flatness();
    push_real(i, flatness)
}

// --- matrices ------------------------------------------------------------------------

fn matrix(i: &mut Interp) -> Result<(), VmError> {
    let array = reals_array(i, &Matrix::IDENTITY.0)?;
    i.push(array)
}

// Fills the matrix operand on top of the stack and leaves it there.
fn fill_matrix(i: &mut Interp, matrix: Matrix) -> Result<(), VmError> {
    let target = i.peek(0)?;
    write_matrix(i, target, matrix)?;
    Ok(())
}

fn identmatrix(i: &mut Interp) -> Result<(), VmError> {
    fill_matrix(i, Matrix::IDENTITY)
}

fn defaultmatrix(i: &mut Interp) -> Result<(), VmError> {
    let matrix = i.backend()?.default_matrix();
    fill_matrix(i, matrix)
}

fn initmatrix(i: &mut Interp) -> Result<(), VmError> {
    let backend = i.backend()?;
    let matrix = backend.default_matrix();
    backend.set_matrix(matrix)
}

fn currentmatrix(i: &mut Interp) -> Result<(), VmError> {
    let matrix = i.backend()?.current_matrix();
    fill_matrix(i, matrix)
}

fn setmatrix(i: &mut Interp) -> Result<(), VmError> {
    let matrix = read_matrix(i, i.peek(0)?)?;
    i.backend()?.set_matrix(matrix)?;
    drop(i, 1)
}

// The two forms of `translate`, `scale`, and `rotate`: with a matrix
// operand on top the result is stored there, otherwise it is concatenated
// to the CTM. `count` numbers precede the optional matrix.
fn concat_or_fill(
    i: &mut Interp,
    count: usize,
    build: impl FnOnce(&[f32]) -> Matrix,
) -> Result<(), VmError> {
    let top = i.peek(0)?;
    let with_matrix = is_array(top);
    let first = if with_matrix { count } else { count - 1 };
    let mut numbers = Vec::with_capacity(count);
    for k in 0..count {
        numbers.push(num_at(i, first - k)?);
    }
    let matrix = build(&numbers);
    if with_matrix {
        write_matrix(i, top, matrix)?;
        drop(i, count + 1)?;
        i.push(top)
    } else {
        i.backend()?.concat(matrix)?;
        drop(i, count)
    }
}

fn translate(i: &mut Interp) -> Result<(), VmError> {
    concat_or_fill(i, 2, |n| Matrix::translation(n[0], n[1]))
}

fn scale(i: &mut Interp) -> Result<(), VmError> {
    concat_or_fill(i, 2, |n| Matrix::scaling(n[0], n[1]))
}

fn rotate(i: &mut Interp) -> Result<(), VmError> {
    concat_or_fill(i, 1, |n| Matrix::rotation(n[0]))
}

fn concat(i: &mut Interp) -> Result<(), VmError> {
    let matrix = read_matrix(i, i.peek(0)?)?;
    i.backend()?.concat(matrix)?;
    drop(i, 1)
}

fn concatmatrix(i: &mut Interp) -> Result<(), VmError> {
    let target = i.peek(0)?;
    let second = read_matrix(i, i.peek(1)?)?;
    let first = read_matrix(i, i.peek(2)?)?;
    write_matrix(i, target, first.then(second))?;
    drop(i, 3)?;
    i.push(target)
}

// `x y transform` uses the CTM; `x y matrix transform` the given matrix.
fn transform_with(i: &mut Interp, delta: bool, inverse: bool) -> Result<(), VmError> {
    let top = i.peek(0)?;
    let (matrix, operands) = if is_array(top) {
        (read_matrix(i, top)?, 3)
    } else {
        (i.backend()?.current_matrix(), 2)
    };
    let p = point_at(i, operands - 2)?;
    let matrix = if inverse {
        matrix.inverse().ok_or(VmError::UndefinedResult)?
    } else {
        matrix
    };
    let result = if delta {
        matrix.apply_delta(p)
    } else {
        matrix.apply(p)
    };
    drop(i, operands)?;
    push_point(i, result)
}

fn transform(i: &mut Interp) -> Result<(), VmError> {
    transform_with(i, false, false)
}

fn itransform(i: &mut Interp) -> Result<(), VmError> {
    transform_with(i, false, true)
}

fn dtransform(i: &mut Interp) -> Result<(), VmError> {
    transform_with(i, true, false)
}

fn idtransform(i: &mut Interp) -> Result<(), VmError> {
    transform_with(i, true, true)
}

fn invertmatrix(i: &mut Interp) -> Result<(), VmError> {
    let target = i.peek(0)?;
    let source = read_matrix(i, i.peek(1)?)?;
    let inverse = source.inverse().ok_or(VmError::UndefinedResult)?;
    write_matrix(i, target, inverse)?;
    drop(i, 2)?;
    i.push(target)
}

// --- path construction ----------------------------------------------------------------

fn newpath(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.newpath()
}

fn moveto(i: &mut Interp) -> Result<(), VmError> {
    let p = point_at(i, 0)?;
    i.backend()?.moveto(p)?;
    drop(i, 2)
}

fn offset(base: Point, delta: Point) -> Point {
    Point::new(base.x + delta.x, base.y + delta.y)
}

// Relative construction adds to the current point in user space, which
// is what the backend reports.
fn rmoveto(i: &mut Interp) -> Result<(), VmError> {
    let delta = point_at(i, 0)?;
    let backend = i.backend()?;
    let current = backend.current_point()?;
    backend.moveto(offset(current, delta))?;
    drop(i, 2)
}

fn lineto(i: &mut Interp) -> Result<(), VmError> {
    let p = point_at(i, 0)?;
    i.backend()?.lineto(p)?;
    drop(i, 2)
}

fn rlineto(i: &mut Interp) -> Result<(), VmError> {
    let delta = point_at(i, 0)?;
    let backend = i.backend()?;
    let current = backend.current_point()?;
    backend.lineto(offset(current, delta))?;
    drop(i, 2)
}

fn curveto(i: &mut Interp) -> Result<(), VmError> {
    let p = point_at(i, 0)?;
    let c2 = point_at(i, 2)?;
    let c1 = point_at(i, 4)?;
    i.backend()?.curveto(c1, c2, p)?;
    drop(i, 6)
}

fn rcurveto(i: &mut Interp) -> Result<(), VmError> {
    let p = point_at(i, 0)?;
    let c2 = point_at(i, 2)?;
    let c1 = point_at(i, 4)?;
    let backend = i.backend()?;
    let current = backend.current_point()?;
    backend.curveto(offset(current, c1), offset(current, c2), offset(current, p))?;
    drop(i, 6)
}

fn arc_operands(i: &Interp) -> Result<(Point, f32, f32, f32), VmError> {
    let end = num_at(i, 0)?;
    let start = num_at(i, 1)?;
    let radius = num_at(i, 2)?;
    let center = point_at(i, 3)?;
    Ok((center, radius, start, end))
}

fn arc(i: &mut Interp) -> Result<(), VmError> {
    let (center, radius, start, end) = arc_operands(i)?;
    i.backend()?.arc(center, radius, start, end)?;
    drop(i, 5)
}

fn arcn(i: &mut Interp) -> Result<(), VmError> {
    let (center, radius, start, end) = arc_operands(i)?;
    i.backend()?.arcn(center, radius, start, end)?;
    drop(i, 5)
}

fn arcto(i: &mut Interp) -> Result<(), VmError> {
    let radius = num_at(i, 0)?;
    let p2 = point_at(i, 1)?;
    let p1 = point_at(i, 3)?;
    let (t1, t2) = i.backend()?.arcto(p1, p2, radius)?;
    drop(i, 5)?;
    push_point(i, t1)?;
    push_point(i, t2)
}

fn closepath(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.closepath()
}

fn currentpoint(i: &mut Interp) -> Result<(), VmError> {
    let p = i.backend()?.current_point()?;
    push_point(i, p)
}

fn pathbbox(i: &mut Interp) -> Result<(), VmError> {
    let Bounds { llx, lly, urx, ury } = i.backend()?.path_bbox()?;
    for value in [llx, lly, urx, ury] {
        push_real(i, value)?;
    }
    Ok(())
}

// --- painting and clipping ---------------------------------------------------------

fn fill(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.fill()
}

fn eofill(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.eofill()
}

fn stroke(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.stroke()
}

fn clip(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.clip()
}

fn eoclip(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.eoclip()
}

fn initclip(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.initclip()
}

fn clippath(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.clippath()?;
    Ok(())
}

// The rectangles of a `rect…` operator: four numbers, or an array of
// 4n numbers. Encoded number strings are not supported and are
// `typecheck`.
fn rect_operands(i: &Interp) -> Result<(Vec<Rect>, usize), VmError> {
    let top = i.peek(0)?;
    if is_array(top) {
        let numbers: Vec<f32> = items(i, top)?
            .into_iter()
            .map(|o| o.as_number().ok_or(VmError::TypeCheck))
            .collect::<Result<_, _>>()?;
        if !numbers.len().is_multiple_of(4) {
            return Err(VmError::RangeCheck);
        }
        let rects = numbers
            .chunks(4)
            .map(|r| Rect {
                x: r[0],
                y: r[1],
                width: r[2],
                height: r[3],
            })
            .collect();
        return Ok((rects, 1));
    }
    if !top.is_number() {
        return Err(VmError::TypeCheck);
    }
    let rect = Rect {
        x: num_at(i, 3)?,
        y: num_at(i, 2)?,
        width: num_at(i, 1)?,
        height: num_at(i, 0)?,
    };
    Ok((vec![rect], 4))
}

fn rectfill(i: &mut Interp) -> Result<(), VmError> {
    let (rects, operands) = rect_operands(i)?;
    i.backend()?.rectfill(&rects)?;
    drop(i, operands)
}

fn rectstroke(i: &mut Interp) -> Result<(), VmError> {
    let (rects, operands) = rect_operands(i)?;
    i.backend()?.rectstroke(&rects)?;
    drop(i, operands)
}

fn rectclip(i: &mut Interp) -> Result<(), VmError> {
    let (rects, operands) = rect_operands(i)?;
    i.backend()?.rectclip(&rects)?;
    drop(i, operands)
}

// --- colour ----------------------------------------------------------------------------

fn unit(value: f32) -> f32 {
    value.clamp(0.0, 1.0)
}

fn component(color: &[f32], n: usize) -> f32 {
    color.get(n).copied().unwrap_or(0.0)
}

/// The device-space operators set a device space and a colour together.
fn set_device_color(i: &mut Interp, space: SpaceSpec, count: usize) -> Result<(), VmError> {
    let mut components = Vec::with_capacity(count);
    for k in 0..count {
        components.push(unit(num_at(i, count - 1 - k)?));
    }
    let backend = i.backend()?;
    backend.set_color_space(&space)?;
    backend.set_color(&components)?;
    drop(i, count)
}

// The conversions the `current…color` queries perform between device
// spaces (PLRM3 §7.2). Spaces the operator layer cannot evaluate (a tint
// transform would be needed) read as black.
fn rgb_of(space: &SpaceSpec, color: &[f32]) -> [f32; 3] {
    match space {
        SpaceSpec::DeviceGray => [component(color, 0); 3],
        SpaceSpec::DeviceRGB => [
            component(color, 0),
            component(color, 1),
            component(color, 2),
        ],
        SpaceSpec::DeviceCMYK => {
            let k = component(color, 3);
            let channel = |n| 1.0 - (component(color, n) + k).min(1.0);
            [channel(0), channel(1), channel(2)]
        }
        _ => [0.0; 3],
    }
}

fn gray_of(space: &SpaceSpec, color: &[f32]) -> f32 {
    match space {
        SpaceSpec::DeviceGray => component(color, 0),
        SpaceSpec::DeviceRGB => luminance(rgb_of(space, color)),
        SpaceSpec::DeviceCMYK => {
            let dark = 0.3 * component(color, 0)
                + 0.59 * component(color, 1)
                + 0.11 * component(color, 2)
                + component(color, 3);
            1.0 - dark.min(1.0)
        }
        _ => 0.0,
    }
}

fn luminance([r, g, b]: [f32; 3]) -> f32 {
    0.3 * r + 0.59 * g + 0.11 * b
}

fn cmyk_of(space: &SpaceSpec, color: &[f32]) -> [f32; 4] {
    match space {
        SpaceSpec::DeviceGray => [0.0, 0.0, 0.0, 1.0 - component(color, 0)],
        SpaceSpec::DeviceRGB => {
            let [r, g, b] = rgb_of(space, color);
            [1.0 - r, 1.0 - g, 1.0 - b, 0.0]
        }
        SpaceSpec::DeviceCMYK => [
            component(color, 0),
            component(color, 1),
            component(color, 2),
            component(color, 3),
        ],
        _ => [0.0, 0.0, 0.0, 1.0],
    }
}

/// HSB to RGB, hue as a fraction of the circle.
pub(crate) fn hsb_to_rgb([h, s, b]: [f32; 3]) -> [f32; 3] {
    if s <= 0.0 {
        return [b; 3];
    }
    let sector = (h.rem_euclid(1.0)) * 6.0;
    let index = sector.floor();
    let fraction = sector - index;
    let low = b * (1.0 - s);
    let falling = b * (1.0 - s * fraction);
    let rising = b * (1.0 - s * (1.0 - fraction));
    match index as u32 % 6 {
        0 => [b, rising, low],
        1 => [falling, b, low],
        2 => [low, b, rising],
        3 => [low, falling, b],
        4 => [rising, low, b],
        _ => [b, low, falling],
    }
}

pub(crate) fn rgb_to_hsb([r, g, b]: [f32; 3]) -> [f32; 3] {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let saturation = if max > 0.0 { delta / max } else { 0.0 };
    if delta <= 0.0 {
        return [0.0, saturation, max];
    }
    let hue = if max == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    [hue / 6.0, saturation, max]
}

fn setgray(i: &mut Interp) -> Result<(), VmError> {
    set_device_color(i, SpaceSpec::DeviceGray, 1)
}

fn current_device_color(i: &mut Interp) -> Result<(SpaceSpec, Vec<f32>), VmError> {
    let backend = i.backend()?;
    Ok((backend.current_color_space(), backend.current_color()))
}

fn currentgray(i: &mut Interp) -> Result<(), VmError> {
    let (space, color) = current_device_color(i)?;
    push_real(i, gray_of(&space, &color))
}

fn setrgbcolor(i: &mut Interp) -> Result<(), VmError> {
    set_device_color(i, SpaceSpec::DeviceRGB, 3)
}

fn currentrgbcolor(i: &mut Interp) -> Result<(), VmError> {
    let (space, color) = current_device_color(i)?;
    for value in rgb_of(&space, &color) {
        push_real(i, value)?;
    }
    Ok(())
}

fn sethsbcolor(i: &mut Interp) -> Result<(), VmError> {
    let hsb = [
        unit(num_at(i, 2)?),
        unit(num_at(i, 1)?),
        unit(num_at(i, 0)?),
    ];
    let rgb = hsb_to_rgb(hsb);
    let backend = i.backend()?;
    backend.set_color_space(&SpaceSpec::DeviceRGB)?;
    backend.set_color(&rgb)?;
    drop(i, 3)
}

fn currenthsbcolor(i: &mut Interp) -> Result<(), VmError> {
    let (space, color) = current_device_color(i)?;
    for value in rgb_to_hsb(rgb_of(&space, &color)) {
        push_real(i, value)?;
    }
    Ok(())
}

fn setcmykcolor(i: &mut Interp) -> Result<(), VmError> {
    set_device_color(i, SpaceSpec::DeviceCMYK, 4)
}

fn currentcmykcolor(i: &mut Interp) -> Result<(), VmError> {
    let (space, color) = current_device_color(i)?;
    for value in cmyk_of(&space, &color) {
        push_real(i, value)?;
    }
    Ok(())
}

/// Colour-space arrays nested deeper than this (Indexed over Separation
/// over …) are `limitcheck`; it also bounds the recursion.
const MAX_SPACE_NESTING: usize = 8;

const MAX_HIVAL: i32 = 4095;

fn name_bytes(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    match object.ty() {
        Type::Name => Ok(i.mem.name_text(object.as_name().expect("name")).to_vec()),
        Type::String => bytes(i, object),
        _ => Err(VmError::TypeCheck),
    }
}

/// A procedure captured as source text, for a tint transform.
fn procedure_source(i: &Interp, object: Object) -> Result<Vec<u8>, VmError> {
    if !is_array(object) || !object.is_executable() {
        return Err(VmError::TypeCheck);
    }
    Ok(output::source(i, object))
}

fn device_space(family: &[u8]) -> Option<SpaceSpec> {
    match family {
        b"DeviceGray" => Some(SpaceSpec::DeviceGray),
        b"DeviceRGB" => Some(SpaceSpec::DeviceRGB),
        b"DeviceCMYK" => Some(SpaceSpec::DeviceCMYK),
        _ => None,
    }
}

/// A colour space from its name or array form. Families outside what
/// the IR carries are `undefined`; an Indexed lookup given as a
/// procedure is `typecheck`, since only string tables are captured.
pub(crate) fn parse_space(i: &Interp, object: Object, depth: usize) -> Result<SpaceSpec, VmError> {
    if depth > MAX_SPACE_NESTING {
        return Err(VmError::LimitCheck);
    }
    if object.ty() == Type::Name {
        let family = name_bytes(i, object)?;
        return device_space(&family).ok_or(VmError::Undefined);
    }
    if !is_array(object) {
        return Err(VmError::TypeCheck);
    }
    let elements = items(i, object)?;
    let (&first, rest) = elements.split_first().ok_or(VmError::RangeCheck)?;
    if first.ty() != Type::Name {
        return Err(VmError::TypeCheck);
    }
    let family = name_bytes(i, first)?;
    if let Some(space) = device_space(&family) {
        return if rest.is_empty() {
            Ok(space)
        } else {
            Err(VmError::RangeCheck)
        };
    }
    match (family.as_slice(), rest) {
        (b"Separation", &[name, alternate, tint]) => Ok(SpaceSpec::Separation {
            name: name_bytes(i, name)?,
            alternate: Box::new(parse_space(i, alternate, depth + 1)?),
            tint_source: procedure_source(i, tint)?,
        }),
        (b"DeviceN", &[names, alternate, tint]) => {
            if !is_array(names) {
                return Err(VmError::TypeCheck);
            }
            let names: Vec<Vec<u8>> = items(i, names)?
                .into_iter()
                .map(|n| name_bytes(i, n))
                .collect::<Result<_, _>>()?;
            if names.is_empty() {
                return Err(VmError::RangeCheck);
            }
            Ok(SpaceSpec::DeviceN {
                names,
                alternate: Box::new(parse_space(i, alternate, depth + 1)?),
                tint_source: procedure_source(i, tint)?,
            })
        }
        (b"Indexed", &[base, hival, lookup]) => {
            let base = parse_space(i, base, depth + 1)?;
            let hival = hival.as_i32().ok_or(VmError::TypeCheck)?;
            if !(0..=MAX_HIVAL).contains(&hival) {
                return Err(VmError::RangeCheck);
            }
            let lookup = if lookup.ty() == Type::String {
                bytes(i, lookup)?
            } else {
                return Err(VmError::TypeCheck);
            };
            if lookup.len() < (hival as usize + 1) * base.components() {
                return Err(VmError::RangeCheck);
            }
            Ok(SpaceSpec::Indexed {
                base: Box::new(base),
                hival: hival as u16,
                lookup,
            })
        }
        (b"Separation" | b"DeviceN" | b"Indexed", _) => Err(VmError::RangeCheck),
        _ => Err(VmError::Undefined),
    }
}

/// The array form of a colour space, rebuilt from the specification; a
/// tint transform is re-scanned from its captured source. A device space
/// nested as an alternate or base is given as its bare name.
pub(crate) fn space_object(
    i: &mut Interp,
    space: &SpaceSpec,
    depth: usize,
) -> Result<Object, VmError> {
    if depth > MAX_SPACE_NESTING {
        return Err(VmError::LimitCheck);
    }
    let family = i.intern(space.family());
    let items = match space {
        SpaceSpec::DeviceGray | SpaceSpec::DeviceRGB | SpaceSpec::DeviceCMYK => {
            if depth > 0 {
                return Ok(family);
            }
            vec![family]
        }
        SpaceSpec::Separation {
            name,
            alternate,
            tint_source,
        } => vec![
            family,
            i.mem.intern(name)?,
            space_object(i, alternate, depth + 1)?,
            procedure_object(i, tint_source)?,
        ],
        SpaceSpec::DeviceN {
            names,
            alternate,
            tint_source,
        } => {
            let mut atoms = Vec::with_capacity(names.len());
            for name in names {
                atoms.push(i.mem.intern(name)?);
            }
            let names = i.mem.alloc_array(atoms)?;
            vec![
                family,
                names,
                space_object(i, alternate, depth + 1)?,
                procedure_object(i, tint_source)?,
            ]
        }
        SpaceSpec::Indexed {
            base,
            hival,
            lookup,
        } => vec![
            family,
            space_object(i, base, depth + 1)?,
            Object::integer(i32::from(*hival)),
            i.mem.alloc_string(lookup.clone()),
        ],
    };
    i.mem.alloc_array(items)
}

fn procedure_object(i: &mut Interp, source: &[u8]) -> Result<Object, VmError> {
    let tokens = scan_all(source, &mut i.mem, &mut ()).map_err(|e| scan_error(e.kind))?;
    match tokens.first() {
        Some(&(object, _)) if is_array(object) && object.is_executable() => Ok(object),
        _ => i.mem.alloc_procedure(Vec::new()),
    }
}

fn setcolorspace(i: &mut Interp) -> Result<(), VmError> {
    let space = parse_space(i, i.peek(0)?, 0)?;
    i.backend()?.set_color_space(&space)?;
    drop(i, 1)
}

fn currentcolorspace(i: &mut Interp) -> Result<(), VmError> {
    let space = i.backend()?.current_color_space();
    let array = space_object(i, &space, 0)?;
    i.push(array)
}

// `setcolor` takes as many numbers as the current space has components;
// the backend decides what range they must lie in.
fn setcolor(i: &mut Interp) -> Result<(), VmError> {
    let count = i.backend()?.current_color_space().components();
    let mut components = Vec::with_capacity(count);
    for k in 0..count {
        components.push(num_at(i, count - 1 - k)?);
    }
    i.backend()?.set_color(&components)?;
    drop(i, count)
}

fn currentcolor(i: &mut Interp) -> Result<(), VmError> {
    let (space, color) = current_device_color(i)?;
    let indexed = matches!(space, SpaceSpec::Indexed { .. });
    for value in color {
        if indexed {
            i.push(Object::integer(value as i32))?;
        } else {
            push_real(i, value)?;
        }
    }
    Ok(())
}

// --- pages and devices ------------------------------------------------------------------

fn showpage(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.showpage()
}

fn copypage(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.copypage()
}

fn erasepage(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.erasepage()
}

fn nulldevice(i: &mut Interp) -> Result<(), VmError> {
    i.backend()?.nulldevice()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn hsb_round_trips_through_rgb() {
        assert!(close(hsb_to_rgb([0.0, 1.0, 1.0]), [1.0, 0.0, 0.0]));
        assert!(close(hsb_to_rgb([1.0 / 3.0, 1.0, 1.0]), [0.0, 1.0, 0.0]));
        assert!(close(hsb_to_rgb([2.0 / 3.0, 1.0, 0.5]), [0.0, 0.0, 0.5]));
        assert!(close(hsb_to_rgb([0.4, 0.0, 0.7]), [0.7; 3]));
        for hsb in [[0.1, 0.5, 0.9], [0.55, 1.0, 0.2], [0.9, 0.3, 0.6]] {
            assert!(close(rgb_to_hsb(hsb_to_rgb(hsb)), hsb), "{hsb:?}");
        }
        assert!(close(rgb_to_hsb([0.5; 3]), [0.0, 0.0, 0.5]));
    }

    #[test]
    fn device_conversions() {
        let gray = SpaceSpec::DeviceGray;
        assert_eq!(rgb_of(&gray, &[0.25]), [0.25; 3]);
        assert_eq!(cmyk_of(&gray, &[0.25]), [0.0, 0.0, 0.0, 0.75]);
        let rgb = SpaceSpec::DeviceRGB;
        assert!((gray_of(&rgb, &[1.0, 1.0, 1.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cmyk_of(&rgb, &[1.0, 0.0, 0.5]), [0.0, 1.0, 0.5, 0.0]);
        let cmyk = SpaceSpec::DeviceCMYK;
        assert_eq!(rgb_of(&cmyk, &[0.0, 0.0, 0.0, 1.0]), [0.0; 3]);
        assert_eq!(gray_of(&cmyk, &[0.0, 0.0, 0.0, 0.0]), 1.0);
        let sep = SpaceSpec::Separation {
            name: b"S".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceGray),
            tint_source: Vec::new(),
        };
        assert_eq!(gray_of(&sep, &[0.5]), 0.0);
        assert_eq!(cmyk_of(&sep, &[0.5]), [0.0, 0.0, 0.0, 1.0]);
    }
}

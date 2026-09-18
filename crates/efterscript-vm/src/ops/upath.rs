// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! User paths (PLRM3 §4.6): a path held as a self-contained array and
//! interpreted by `uappend`, `ufill`, `ueofill`, `ustroke`, and read back
//! by `upath`, plus `setbbox`, `ucache`, `ucachestatus`, and
//! `setucacheparams`, and `arct`, the form of `arcto` a user path may
//! contain. A user path is walked without executing it: its elements are
//! numbers and the names (or operator objects) of the twelve operators
//! §4.6.1 allows, or, in the encoded form of §4.6.2, a number sequence
//! and a string of operator codes; either drives the same backend calls
//! the ordinary path operators make. No user path cache exists: `ucache`
//! is accepted and ignored, `ucachestatus` reports nothing cached.
//!
//! The bounding box is checked as `setbbox` describes it (§8.2): the box
//! is taken to default user space through the CTM, enlarged to the
//! enclosing axis-aligned rectangle there, and every coordinate the path
//! reaches is checked against it. For an arc the figure itself is
//! checked — its end points and the axis extremes the sweep crosses —
//! not the control points of the curves it becomes, which depend on
//! how finely the backend cuts the sweep.

use crate::error::VmError;
use crate::graphics::{Bounds, GraphicsBackend, Matrix, Point, Seg};
use crate::interp::Interp;
use crate::numbers;
use crate::object::{Object, Type};
use crate::ops::array::{bytes, items};
use crate::ops::graphics::{
    drop, is_array, num_at, point_at, rcurveto_by, read_matrix, rlineto_by, rmoveto_by,
};
use crate::ops::pattern;

op_table! { graphics OPS {
    "arct" => arct, [Num, Num, Num, Num, Num];
    "setbbox" => setbbox, [Num, Num, Num, Num];
    "ucache" => ucache;
    "ucachestatus" => ucachestatus;
    "setucacheparams" => setucacheparams;
    "uappend" => uappend, [Any];
    "ufill" => ufill, [Any];
    "ueofill" => ueofill, [Any];
    "ustroke" => ustroke, [Any];
    "upath" => upath, [Bool];
}}

op_table! { graphics OUTLINE_OPS {
    "ustrokepath" => ustrokepath, [Any];
}}

/// The operators a user path may contain, in the order of their codes
/// in the encoded form (PLRM3 §4.6.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    SetBbox,
    MoveTo,
    RMoveTo,
    LineTo,
    RLineTo,
    CurveTo,
    RCurveTo,
    Arc,
    ArcN,
    ArcT,
    ClosePath,
    UCache,
}

impl Op {
    const ALL: [Op; 12] = [
        Op::SetBbox,
        Op::MoveTo,
        Op::RMoveTo,
        Op::LineTo,
        Op::RLineTo,
        Op::CurveTo,
        Op::RCurveTo,
        Op::Arc,
        Op::ArcN,
        Op::ArcT,
        Op::ClosePath,
        Op::UCache,
    ];

    fn from_code(code: u8) -> Option<Op> {
        Op::ALL.get(usize::from(code)).copied()
    }

    fn name(self) -> &'static str {
        match self {
            Op::SetBbox => "setbbox",
            Op::MoveTo => "moveto",
            Op::RMoveTo => "rmoveto",
            Op::LineTo => "lineto",
            Op::RLineTo => "rlineto",
            Op::CurveTo => "curveto",
            Op::RCurveTo => "rcurveto",
            Op::Arc => "arc",
            Op::ArcN => "arcn",
            Op::ArcT => "arct",
            Op::ClosePath => "closepath",
            Op::UCache => "ucache",
        }
    }

    fn from_name(name: &[u8]) -> Option<Op> {
        Op::ALL.into_iter().find(|op| op.name().as_bytes() == name)
    }

    fn operands(self) -> usize {
        match self {
            Op::ClosePath | Op::UCache => 0,
            Op::MoveTo | Op::RMoveTo | Op::LineTo | Op::RLineTo => 2,
            Op::SetBbox => 4,
            Op::Arc | Op::ArcN | Op::ArcT => 5,
            Op::CurveTo | Op::RCurveTo => 6,
        }
    }

    /// Whether the operator sets a current point without needing one.
    fn positions(self) -> bool {
        matches!(self, Op::MoveTo | Op::Arc | Op::ArcN)
    }
}

/// A user path reduced to its steps, checked for shape but not yet for
/// geometry.
type Steps = Vec<(Op, Vec<f32>)>;

/// Reads a user path operand into its steps: the literal form (an array
/// of at least five elements) or the encoded form (an array of two,
/// the numbers and the operator string). Every deviation from the rules
/// of PLRM3 §4.6.1 and §4.6.2 is `typecheck`; a malformed number string
/// answers as `numbers::decode` does.
fn parse(i: &Interp, path: Object) -> Result<Steps, VmError> {
    if !is_array(path) {
        return Err(VmError::TypeCheck);
    }
    let elements = items(i, path)?;
    match elements.len() {
        2 => parse_encoded(i, elements[0], elements[1]),
        n if n >= 5 => parse_literal(i, &elements),
        _ => Err(VmError::TypeCheck),
    }
}

fn parse_literal(i: &Interp, elements: &[Object]) -> Result<Steps, VmError> {
    let mut steps = Vec::new();
    let mut pending = Vec::new();
    for &element in elements {
        if element.is_number() {
            pending.push(element.as_number().expect("a number"));
            continue;
        }
        let op = match element.ty() {
            Type::Name if element.is_executable() => {
                let atom = element.as_name().expect("a name");
                Op::from_name(i.mem.name_text(atom))
            }
            Type::Operator => {
                let index = element.as_operator().expect("an operator") as usize;
                i.ops
                    .get(index)
                    .and_then(|entry| Op::from_name(entry.name.as_bytes()))
            }
            _ => None,
        }
        .ok_or(VmError::TypeCheck)?;
        if pending.len() != op.operands() {
            return Err(VmError::TypeCheck);
        }
        steps.push((op, std::mem::take(&mut pending)));
    }
    if !pending.is_empty() {
        return Err(VmError::TypeCheck);
    }
    Ok(steps)
}

/// The encoded form: a number sequence and a string of operator codes,
/// each code optionally preceded by a repeat count (33 to 255, meaning
/// 32 fewer repetitions). Running out of numbers is `typecheck`; numbers
/// left over are ignored, and so is a count with nothing after it, as
/// other interpreters do.
fn parse_encoded(i: &Interp, data: Object, codes: Object) -> Result<Steps, VmError> {
    let numbers: Vec<f32> = match data.ty() {
        Type::String => numbers::decode(&bytes(i, data)?)?
            .into_iter()
            .map(|n| n.as_f32())
            .collect(),
        Type::Array | Type::PackedArray => items(i, data)?
            .into_iter()
            .map(|n| n.as_number().ok_or(VmError::TypeCheck))
            .collect::<Result<_, _>>()?,
        _ => return Err(VmError::TypeCheck),
    };
    let codes = bytes(i, codes)?;
    let mut steps = Vec::new();
    let mut next = 0;
    let mut repeat = 1;
    for &code in &codes {
        if code > 32 {
            repeat = usize::from(code - 32);
            continue;
        }
        let op = Op::from_code(code).ok_or(VmError::TypeCheck)?;
        for _ in 0..repeat {
            let end = next + op.operands();
            let args = numbers.get(next..end).ok_or(VmError::TypeCheck)?;
            steps.push((op, args.to_vec()));
            next = end;
        }
        repeat = 1;
    }
    Ok(steps)
}

/// Runs the steps against the backend, enforcing the structure of PLRM3
/// §4.6.1 — `ucache` only first, then `setbbox` exactly once, then
/// construction starting with a positioning operator — and the box.
struct Walker {
    ctm: Matrix,
    /// The box in default user space, once `setbbox` has run.
    bbox: Option<Bounds>,
    cached: bool,
    positioned: bool,
}

/// Rounding room, in default user space units, for a point that lies
/// on the box's edge after the trip through the CTM.
const BOX_TOLERANCE: f32 = 1e-3;

impl Walker {
    fn new(backend: &dyn GraphicsBackend) -> Self {
        Walker {
            ctm: backend.current_matrix(),
            bbox: None,
            cached: false,
            positioned: false,
        }
    }

    fn run(mut self, backend: &mut dyn GraphicsBackend, steps: &Steps) -> Result<(), VmError> {
        for (op, args) in steps {
            self.step(backend, *op, args)?;
        }
        if self.bbox.is_none() {
            return Err(VmError::TypeCheck);
        }
        Ok(())
    }

    fn step(
        &mut self,
        backend: &mut dyn GraphicsBackend,
        op: Op,
        args: &[f32],
    ) -> Result<(), VmError> {
        let point = |k: usize| Point::new(args[k], args[k + 1]);
        match op {
            Op::UCache => {
                if self.cached || self.bbox.is_some() {
                    return Err(VmError::TypeCheck);
                }
                self.cached = true;
                return Ok(());
            }
            Op::SetBbox => {
                if self.bbox.is_some() {
                    return Err(VmError::TypeCheck);
                }
                let [llx, lly, urx, ury] = [args[0], args[1], args[2], args[3]];
                if urx < llx || ury < lly {
                    return Err(VmError::RangeCheck);
                }
                self.bbox = Some(enclosing(
                    self.ctm,
                    &[
                        Point::new(llx, lly),
                        Point::new(urx, lly),
                        Point::new(urx, ury),
                        Point::new(llx, ury),
                    ],
                ));
                return Ok(());
            }
            _ => {}
        }
        if self.bbox.is_none() || (!self.positioned && !op.positions()) {
            return Err(VmError::TypeCheck);
        }
        self.positioned = true;
        match op {
            Op::MoveTo => {
                self.check(&[point(0)])?;
                backend.moveto(point(0))
            }
            Op::LineTo => {
                self.check(&[point(0)])?;
                backend.lineto(point(0))
            }
            Op::RMoveTo => {
                self.check_relative(backend, &[point(0)])?;
                rmoveto_by(backend, point(0))
            }
            Op::RLineTo => {
                self.check_relative(backend, &[point(0)])?;
                rlineto_by(backend, point(0))
            }
            Op::CurveTo => {
                self.check(&[point(0), point(2), point(4)])?;
                backend.curveto(point(0), point(2), point(4))
            }
            Op::RCurveTo => {
                self.check_relative(backend, &[point(0), point(2), point(4)])?;
                rcurveto_by(backend, point(0), point(2), point(4))
            }
            Op::Arc | Op::ArcN => {
                let (center, radius, start, end) = (point(0), args[2], args[3], args[4]);
                let ccw = op == Op::Arc;
                self.check(&arc_figure(center, radius, start, end, ccw))?;
                if ccw {
                    backend.arc(center, radius, start, end)
                } else {
                    backend.arcn(center, radius, start, end)
                }
            }
            Op::ArcT => {
                self.check(&[point(0), point(2)])?;
                backend.arcto(point(0), point(2), args[4])?;
                Ok(())
            }
            Op::ClosePath => backend.closepath(),
            Op::SetBbox | Op::UCache => unreachable!("handled above"),
        }
    }

    /// `rangecheck` unless every user-space point lies in the box.
    fn check(&self, points: &[Point]) -> Result<(), VmError> {
        let bbox = self.bbox.expect("checked by the caller");
        for &p in points {
            let d = self.ctm.apply(p);
            if d.x < bbox.llx - BOX_TOLERANCE
                || d.x > bbox.urx + BOX_TOLERANCE
                || d.y < bbox.lly - BOX_TOLERANCE
                || d.y > bbox.ury + BOX_TOLERANCE
                || !d.x.is_finite()
                || !d.y.is_finite()
            {
                return Err(VmError::RangeCheck);
            }
        }
        Ok(())
    }

    fn check_relative(
        &self,
        backend: &dyn GraphicsBackend,
        deltas: &[Point],
    ) -> Result<(), VmError> {
        let current = backend.current_point()?;
        let points: Vec<Point> = deltas
            .iter()
            .map(|d| Point::new(current.x + d.x, current.y + d.y))
            .collect();
        self.check(&points)
    }
}

/// The axis-aligned rectangle in default user space enclosing `points`
/// taken through `ctm`.
fn enclosing(ctm: Matrix, points: &[Point]) -> Bounds {
    let mut bounds = Bounds::new(
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in points.iter().map(|&p| ctm.apply(p)) {
        bounds.llx = bounds.llx.min(p.x);
        bounds.lly = bounds.lly.min(p.y);
        bounds.urx = bounds.urx.max(p.x);
        bounds.ury = bounds.ury.max(p.y);
    }
    bounds
}

/// The points that bound an arc's figure: its two ends and the point at
/// every multiple of a quarter turn the sweep passes through.
fn arc_figure(center: Point, radius: f32, start: f32, end: f32, ccw: bool) -> Vec<Point> {
    let at = |degrees: f64| {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Point::new(
            (f64::from(center.x) + f64::from(radius) * cos) as f32,
            (f64::from(center.y) + f64::from(radius) * sin) as f32,
        )
    };
    let (start, mut end) = (f64::from(start), f64::from(end));
    if !start.is_finite() || !end.is_finite() {
        return vec![center];
    }
    let mut points = vec![at(start), at(end)];
    if ccw {
        while end < start {
            end += 360.0;
        }
        let mut angle = (start / 90.0).ceil() * 90.0;
        while angle <= end {
            points.push(at(angle));
            angle += 90.0;
        }
    } else {
        while end > start {
            end -= 360.0;
        }
        let mut angle = (start / 90.0).floor() * 90.0;
        while angle >= end {
            points.push(at(angle));
            angle -= 90.0;
        }
    }
    points
}

/// Appends the steps to the current path.
fn append(i: &mut Interp, steps: &Steps) -> Result<(), VmError> {
    let backend = i.backend()?;
    Walker::new(backend).run(backend, steps)
}

fn uappend(i: &mut Interp) -> Result<(), VmError> {
    let steps = parse(i, i.peek(0)?)?;
    append(i, &steps)?;
    drop(i, 1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Paint {
    Fill,
    EoFill,
    Stroke,
}

/// `gsave newpath uappend <paint> grestore`, the matrix of `ustroke`'s
/// second form concatenated after the path is built (PLRM3 §8.2), so it
/// scales the line width and dash but not the path. The state is
/// restored whether or not the walk succeeds; the operands stay on the
/// stack when it fails.
fn paint(i: &mut Interp, kind: Paint) -> Result<(), VmError> {
    let top = i.peek(0)?;
    let matrix = if kind == Paint::Stroke {
        six_numbers(i, top)
    } else {
        None
    };
    let (path, operands) = match matrix {
        Some(_) => (i.peek(1)?, 2),
        None => (top, 1),
    };
    let steps = parse(i, path)?;
    let operator = match kind {
        Paint::Fill => "ufill",
        Paint::EoFill => "ueofill",
        Paint::Stroke => "ustroke",
    };
    if pattern::capture_cell(i, operator)? {
        return Ok(());
    }
    let depth = i.backend()?.gstate_depth();
    i.gsave()?;
    let painted = (|| {
        let backend = i.backend()?;
        backend.newpath()?;
        Walker::new(backend).run(backend, &steps)?;
        if let Some(matrix) = matrix {
            backend.concat(matrix)?;
        }
        match kind {
            Paint::Fill => backend.fill(),
            Paint::EoFill => backend.eofill(),
            Paint::Stroke => backend.stroke(),
        }
    })();
    let restored = i.grestore_to(depth);
    painted?;
    restored?;
    drop(i, operands)
}

/// A matrix operand: an array of exactly six numbers. Anything else on
/// top of the stack is taken for the user path.
fn six_numbers(i: &Interp, object: Object) -> Option<Matrix> {
    if !is_array(object) || object.length() != Some(6) {
        return None;
    }
    read_matrix(i, object).ok()
}

fn ufill(i: &mut Interp) -> Result<(), VmError> {
    paint(i, Paint::Fill)
}

fn ueofill(i: &mut Interp) -> Result<(), VmError> {
    paint(i, Paint::EoFill)
}

fn ustroke(i: &mut Interp) -> Result<(), VmError> {
    paint(i, Paint::Stroke)
}

/// `newpath uappend strokepath`, and in the second form the matrix
/// concatenated between the walk and the outline with the CTM put back
/// afterwards (PLRM3 §8.2 `ustrokepath`): the outline stays as the
/// current path, which is the operator's whole effect. The operands
/// stay on the stack when the walk fails.
fn ustrokepath(i: &mut Interp) -> Result<(), VmError> {
    let top = i.peek(0)?;
    let matrix = six_numbers(i, top);
    let (path, operands) = match matrix {
        Some(_) => (i.peek(1)?, 2),
        None => (top, 1),
    };
    let steps = parse(i, path)?;
    i.set_declared_path_bbox(None);
    let backend = i.backend()?;
    backend.newpath()?;
    Walker::new(backend).run(backend, &steps)?;
    match matrix {
        Some(matrix) => {
            let ctm = backend.current_matrix();
            backend.concat(matrix)?;
            let outlined = backend.stroke_outline();
            backend.set_matrix(ctm)?;
            outlined?;
        }
        None => backend.stroke_outline()?,
    }
    drop(i, operands)
}

/// `bool upath`: the current path as an executable array in the current
/// user space — `ucache` first when the operand is true, then the
/// path's bounding box (every point, a trailing `moveto` included; all
/// zeros for an empty path) and `setbbox`, then the segments with real
/// coordinates and executable operator names.
fn upath(i: &mut Interp) -> Result<(), VmError> {
    let cache = i.peek(0)?.as_bool().expect("a boolean");
    let segs = i.backend()?.current_path();
    let points: Vec<Point> = segs
        .iter()
        .flat_map(|seg| match *seg {
            Seg::Move(p) | Seg::Line(p) => vec![p],
            Seg::Curve(a, b, c) => vec![a, b, c],
            Seg::Close => Vec::new(),
        })
        .collect();
    let bbox = if points.is_empty() {
        Bounds::default()
    } else {
        enclosing(Matrix::IDENTITY, &points)
    };
    let mut elements = Vec::with_capacity(segs.len() * 3 + 6);
    let name = |i: &mut Interp, op: Op| i.intern(op.name()).as_executable();
    let real = Object::real;
    if cache {
        elements.push(name(i, Op::UCache));
    }
    elements.extend([bbox.llx, bbox.lly, bbox.urx, bbox.ury].map(real));
    elements.push(name(i, Op::SetBbox));
    for seg in segs {
        match seg {
            Seg::Move(p) => {
                elements.extend([real(p.x), real(p.y), name(i, Op::MoveTo)]);
            }
            Seg::Line(p) => {
                elements.extend([real(p.x), real(p.y), name(i, Op::LineTo)]);
            }
            Seg::Curve(a, b, c) => {
                elements.extend([a.x, a.y, b.x, b.y, c.x, c.y].map(real));
                elements.push(name(i, Op::CurveTo));
            }
            Seg::Close => elements.push(name(i, Op::ClosePath)),
        }
    }
    let array = i.mem.alloc_procedure(elements)?;
    drop(i, 1)?;
    i.push(array)
}

/// `arct`: `arcto` without its tangent-point results.
fn arct(i: &mut Interp) -> Result<(), VmError> {
    let radius = num_at(i, 0)?;
    let p2 = point_at(i, 1)?;
    let p1 = point_at(i, 3)?;
    i.backend()?.arcto(p1, p2, radius)?;
    drop(i, 5)
}

/// `setbbox` outside a user path: the box is checked and recorded on
/// the interpreter (`Interp::declared_path_bbox`) but not enforced on
/// the path operators that follow.
fn setbbox(i: &mut Interp) -> Result<(), VmError> {
    let ury = num_at(i, 0)?;
    let urx = num_at(i, 1)?;
    let lly = num_at(i, 2)?;
    let llx = num_at(i, 3)?;
    if urx < llx || ury < lly {
        return Err(VmError::RangeCheck);
    }
    i.set_declared_path_bbox(Some(Bounds::new(llx, lly, urx, ury)));
    drop(i, 4)
}

fn ucache(_: &mut Interp) -> Result<(), VmError> {
    Ok(())
}

/// `ucachestatus`: a mark and five integers, all zero, since nothing is
/// ever cached and no limit applies.
fn ucachestatus(i: &mut Interp) -> Result<(), VmError> {
    i.push(Object::mark())?;
    for _ in 0..5 {
        i.push(Object::integer(0))?;
    }
    Ok(())
}

/// `setucacheparams`: the operands down to and including the mark are
/// removed and nothing is set.
fn setucacheparams(i: &mut Interp) -> Result<(), VmError> {
    let at = i
        .ostack()
        .iter()
        .rposition(|o| o.ty() == Type::Mark)
        .ok_or(VmError::UnmatchedMark)?;
    let extra = i.ostack().len() - at;
    drop(i, extra)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
    }

    #[test]
    fn codes_follow_the_table_order() {
        assert_eq!(Op::from_code(0), Some(Op::SetBbox));
        assert_eq!(Op::from_code(9), Some(Op::ArcT));
        assert_eq!(Op::from_code(11), Some(Op::UCache));
        assert_eq!(Op::from_code(12), None);
        for op in Op::ALL {
            assert_eq!(Op::from_name(op.name().as_bytes()), Some(op));
        }
        assert_eq!(Op::from_name(b"arcto"), None);
        assert_eq!(Op::from_name(b"charpath"), None);
    }

    #[test]
    fn arc_figures_take_in_the_axis_extremes() {
        let c = Point::new(5.0, 5.0);
        // 45° to 135° counter-clockwise passes through 90°: the top.
        let up = arc_figure(c, 5.0, 45.0, 135.0, true);
        assert!(up.iter().any(|&p| close(p, Point::new(5.0, 10.0))));
        assert!(!up.iter().any(|&p| close(p, Point::new(10.0, 5.0))));
        // The same angles clockwise go the long way round through 0°,
        // -90°, and -180°.
        let down = arc_figure(c, 5.0, 45.0, 135.0, false);
        assert!(down.iter().any(|&p| close(p, Point::new(10.0, 5.0))));
        assert!(down.iter().any(|&p| close(p, Point::new(5.0, 0.0))));
        assert!(down.iter().any(|&p| close(p, Point::new(0.0, 5.0))));
        assert!(!down.iter().any(|&p| close(p, Point::new(5.0, 10.0))));
        // A full circle from 0 touches all four.
        assert_eq!(arc_figure(c, 1.0, 0.0, 360.0, true).len(), 7);
        // A sweep that ends before the start wraps forward.
        let wrapped = arc_figure(c, 5.0, 350.0, 10.0, true);
        assert!(wrapped.iter().any(|&p| close(p, Point::new(10.0, 5.0))));
        assert_eq!(wrapped.len(), 3);
    }

    #[test]
    fn enclosing_box_is_axis_aligned_after_the_ctm() {
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(10.0, 0.0),
            Point::new(10.0, 10.0),
            Point::new(0.0, 10.0),
        ];
        let b = enclosing(Matrix::rotation(45.0), &corners);
        let side = 10.0 * 2f32.sqrt() / 2.0;
        assert!((b.llx + side).abs() < 1e-4);
        assert!((b.urx - side).abs() < 1e-4);
        assert!(b.lly.abs() < 1e-4);
        assert!((b.ury - 2.0 * side).abs() < 1e-4);
    }
}

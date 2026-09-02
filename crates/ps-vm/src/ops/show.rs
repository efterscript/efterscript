// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The show family and `stringwidth` as a loop frame (PLRM3 §5.3, §5.7).
//!
//! A frame holds the string being shown and the run of glyphs assembled
//! so far. A resident font's glyphs are measured from the embedded
//! metrics and consumed without leaving the step; a Type 3 font's glyph
//! is one step each: the glyph procedure runs as frames above this one,
//! and when the loop returns here the width it declared is read and the
//! glyph joins the run. The run is handed to the backend as one `show`
//! call when the string ends or, for `kshow`, before the procedure runs
//! between two glyphs. `stringwidth` runs the same frame in measuring
//! mode: no run is shown and the total displacement is pushed instead.

use ps_fonts::StdFont;

use crate::error::VmError;
use crate::graphics::{Bounds, FontInfo, FontRef, FontSource, Glyph, Matrix, Point};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Object, Type};
use crate::ops::array::items;
use crate::ops::font::entry;

/// How a font's glyphs are produced.
#[derive(Clone, Copy, Debug)]
pub(crate) enum FontKind {
    /// Widths from the resident metrics; nothing is executed.
    Resident(StdFont),
    /// `BuildGlyph` (glyph names) or `BuildChar` (codes) is run per glyph.
    Type3 { build: Object, by_name: bool },
}

/// The show operator and the extra operands it took.
#[derive(Clone, Debug)]
pub(crate) enum Variant {
    Show,
    AShow {
        ax: f32,
        ay: f32,
    },
    WidthShow {
        cx: f32,
        cy: f32,
        code: i32,
    },
    AWidthShow {
        cx: f32,
        cy: f32,
        code: i32,
        ax: f32,
        ay: f32,
    },
    KShow {
        procedure: Object,
    },
    /// One number per glyph, replacing its x displacement.
    XShow(Vec<f32>),
    YShow(Vec<f32>),
    /// Two numbers per glyph.
    XYShow(Vec<f32>),
    /// The glyph named, looked up in the encoding for its code.
    GlyphShow(Object),
}

/// A Type 3 glyph whose procedure is running.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RunningGlyph {
    pub(crate) code: u8,
    /// The graphics-state depth to return to; `None` without a backend.
    pub(crate) depth: Option<usize>,
    /// Declared by `setcachedevice`, `setcachedevice2`, or `setcharwidth`.
    pub(crate) width: Option<(f32, f32)>,
    pub(crate) bbox: Option<Bounds>,
}

#[derive(Clone, Debug)]
pub struct ShowFrame {
    pub(crate) operator: &'static str,
    pub(crate) font: FontRef,
    pub(crate) dict: Object,
    pub(crate) kind: FontKind,
    pub(crate) encoding: Object,
    pub(crate) codes: Vec<u8>,
    pub(crate) variant: Variant,
    pub(crate) measure: bool,
    pub(crate) next: usize,
    /// The run assembled since the last flush.
    pub(crate) pending: Vec<Glyph>,
    /// The run's displacement so far, in glyph space.
    pub(crate) total: Point,
    /// Where the run starts, in user space; read from the backend when the
    /// first Type 3 glyph needs it.
    pub(crate) origin: Option<Point>,
    pub(crate) running: Option<RunningGlyph>,
}

impl ShowFrame {
    /// The procedure the frame runs between its steps, if any.
    pub fn procedure(&self) -> Object {
        match (&self.variant, self.kind) {
            (Variant::KShow { procedure }, _) => *procedure,
            (_, FontKind::Type3 { build, .. }) => build,
            _ => Object::null(),
        }
    }

    pub(crate) fn references(&self) -> Vec<Object> {
        let mut objects = vec![self.dict, self.encoding];
        if let FontKind::Type3 { build, .. } = self.kind {
            objects.push(build);
        }
        if let Variant::GlyphShow(name) = self.variant {
            objects.push(name);
        }
        objects
    }
}

enum Next {
    Done,
    Run(Object),
}

fn is_array(object: Object) -> bool {
    matches!(object.ty(), Type::Array | Type::PackedArray)
}

/// How a font dictionary's glyphs are produced, or `invalidfont` when
/// they cannot be: a Type 1 or Type 42 dictionary without the resident
/// marker has no glyph source until font programs are parsed.
pub(crate) fn font_kind(i: &mut Interp, dict: Object) -> Result<FontKind, VmError> {
    let font_type = entry(i, dict, "FontType")?.and_then(Object::as_i32);
    match font_type {
        Some(3) => {
            if let Some(build) = entry(i, dict, "BuildGlyph")?.filter(|p| is_array(*p)) {
                Ok(FontKind::Type3 {
                    build,
                    by_name: true,
                })
            } else if let Some(build) = entry(i, dict, "BuildChar")?.filter(|p| is_array(*p)) {
                Ok(FontKind::Type3 {
                    build,
                    by_name: false,
                })
            } else {
                Err(VmError::InvalidFont)
            }
        }
        Some(1 | 42) => entry(i, dict, "ResidentFont")?
            .and_then(Object::as_i32)
            .and_then(|n| usize::try_from(n).ok())
            .and_then(StdFont::from_index)
            .map(FontKind::Resident)
            .ok_or(VmError::InvalidFont),
        _ => Err(VmError::InvalidFont),
    }
}

/// Starts a show or a measurement of `codes` in the current font. The
/// `operands` the operator took are popped only once everything has been
/// checked, so a failing operator leaves them in place.
pub(crate) fn begin(
    i: &mut Interp,
    operator: &'static str,
    variant: Variant,
    codes: Vec<u8>,
    measure: bool,
    operands: usize,
) -> Result<(), VmError> {
    let font = i.current_font().ok_or(VmError::InvalidFont)?;
    let dict = i.font_dict(font.instance).ok_or(VmError::InvalidFont)?;
    let kind = font_kind(i, dict)?;
    let encoding = entry(i, dict, "Encoding")?
        .filter(|e| is_array(*e))
        .ok_or(VmError::InvalidFont)?;
    let codes = match variant {
        Variant::GlyphShow(name) => vec![glyphshow_code(i, encoding, name, kind)?],
        _ => codes,
    };
    let needed = match &variant {
        Variant::XShow(values) | Variant::YShow(values) => Some((values.len(), codes.len())),
        Variant::XYShow(values) => Some((values.len(), 2 * codes.len())),
        _ => None,
    };
    if needed.is_some_and(|(have, need)| have < need) {
        return Err(VmError::RangeCheck);
    }
    if !measure {
        i.backend()?.current_point()?;
    }
    if i.has_graphics_backend() && !i.font_described(font.instance) {
        let info = describe(i, dict, kind, encoding, font.matrix)?;
        i.backend()?.define_font(font.instance, &info)?;
        i.mark_font_described(font.instance);
    }
    for _ in 0..operands {
        i.pop()?;
    }
    i.push_frame(Frame::Loop(LoopFrame::Show(Box::new(ShowFrame {
        operator,
        font,
        dict,
        kind,
        encoding,
        codes,
        variant,
        measure,
        next: 0,
        pending: Vec::new(),
        total: Point::default(),
        origin: None,
        running: None,
    }))))
}

/// What the backend needs to know about the font: its glyph source and
/// its encoding as names. A Type 3 font is identified by its `FID` and
/// carries the matrix it was defined with, so a scaled instance records
/// its glyphs in the same space as the original.
fn describe(
    i: &mut Interp,
    dict: Object,
    kind: FontKind,
    encoding: Object,
    matrix: Matrix,
) -> Result<FontInfo, VmError> {
    let encoding = items(i, encoding)?
        .into_iter()
        .map(|entry| entry.as_name().map(|atom| i.mem.name_text(atom).to_vec()))
        .collect();
    let source = match kind {
        FontKind::Resident(font) => FontSource::Resident(font),
        FontKind::Type3 { .. } => {
            let family = entry(i, dict, "FID")?
                .and_then(Object::as_font_id)
                .unwrap_or(u32::MAX);
            let font_bbox = match entry(i, dict, "FontBBox")? {
                Some(array) if is_array(array) => {
                    let values: Vec<f32> = items(i, array)?
                        .iter()
                        .filter_map(|o| o.as_number())
                        .collect();
                    match values.as_slice() {
                        &[llx, lly, urx, ury] => Bounds::new(llx, lly, urx, ury),
                        _ => Bounds::default(),
                    }
                }
                _ => Bounds::default(),
            };
            FontSource::Type3 {
                family,
                font_matrix: i.defined_matrix(family).unwrap_or(matrix),
                font_bbox,
            }
        }
    };
    Ok(FontInfo { source, encoding })
}

// The code the encoding gives the name; a name outside the encoding can
// still be shown by name (resident metrics, `BuildGlyph`) and is recorded
// under code 0, but a `BuildChar` font has nothing to run for it.
fn glyphshow_code(
    i: &Interp,
    encoding: Object,
    name: Object,
    kind: FontKind,
) -> Result<u8, VmError> {
    let found = items(i, encoding)?
        .iter()
        .position(|entry| entry.ty() == Type::Name && entry.eq(name));
    match (found, kind) {
        (Some(code), _) => u8::try_from(code).map_err(|_| VmError::RangeCheck),
        (None, FontKind::Type3 { by_name: false, .. }) => Err(VmError::RangeCheck),
        (None, _) => Ok(0),
    }
}

/// One step of the frame on top of the execution stack.
pub(crate) fn step(i: &mut Interp) {
    let Some(Frame::Loop(LoopFrame::Show(mut frame))) = i.estack.pop() else {
        return;
    };
    match advance(i, &mut frame) {
        Ok(Next::Done) => {}
        Ok(Next::Run(procedure)) => {
            i.estack.push(Frame::Loop(LoopFrame::Show(frame)));
            if let Err(e) = i.push_proc(procedure) {
                i.raise(e, procedure);
            }
        }
        Err(e) => {
            abandon(i, &frame);
            let command = i.operator(frame.operator).unwrap_or(Object::null());
            i.raise(e, command);
        }
    }
}

/// Cleans up after a frame that is being discarded while a glyph
/// procedure was running: the capture is closed with no width and the
/// graphics state returns to what it was before the glyph.
pub(crate) fn abandon(i: &mut Interp, frame: &ShowFrame) {
    if let Some(run) = frame.running
        && let Some(depth) = run.depth
        && let Some(backend) = i.graphics_backend()
    {
        let _ = backend.end_glyph((0.0, 0.0), None);
        let _ = backend.grestore_to(depth);
    }
}

fn advance(i: &mut Interp, f: &mut ShowFrame) -> Result<Next, VmError> {
    if let Some(run) = f.running.take() {
        finish_glyph(i, f, run)?;
        if let Some(procedure) = after_glyph(i, f)? {
            return Ok(Next::Run(procedure));
        }
    }
    loop {
        let Some(&code) = f.codes.get(f.next) else {
            finish(i, f)?;
            return Ok(Next::Done);
        };
        match f.kind {
            FontKind::Resident(font) => {
                let width = glyph_name(i, f, code)
                    .map(|name| i.mem.name_text(name.as_name().expect("name")).to_vec())
                    .and_then(|name| font.width(std::str::from_utf8(&name).ok()?))
                    .unwrap_or(0);
                let displacement = displacement(f, code, Point::new(f32::from(width), 0.0))?;
                add_glyph(f, code, displacement);
                if let Some(procedure) = after_glyph(i, f)? {
                    return Ok(Next::Run(procedure));
                }
            }
            FontKind::Type3 { build, by_name } => {
                begin_glyph(i, f, code, by_name)?;
                return Ok(Next::Run(build));
            }
        }
    }
}

/// The glyph name a code selects: the `glyphshow` name, else the
/// encoding's entry when it is a name.
fn glyph_name(i: &Interp, f: &ShowFrame, code: u8) -> Option<Object> {
    if let Variant::GlyphShow(name) = f.variant {
        return Some(name);
    }
    i.mem
        .array(f.encoding)?
        .get(usize::from(code))
        .copied()
        .filter(|entry| entry.ty() == Type::Name)
}

// A user-space distance in glyph space; a singular font matrix maps
// every distance to nothing.
fn to_glyph(f: &ShowFrame, dx: f32, dy: f32) -> Point {
    f.font
        .matrix
        .inverse()
        .map(|m| m.apply_delta(Point::new(dx, dy)))
        .unwrap_or_default()
}

fn add(a: Point, b: Point) -> Point {
    Point::new(a.x + b.x, a.y + b.y)
}

/// The displacement the glyph at `f.next` gets: its width plus the
/// variant's addition, or the variant's replacement.
fn displacement(f: &ShowFrame, code: u8, width: Point) -> Result<Point, VmError> {
    let k = f.next;
    let at = |values: &[f32], index: usize| values.get(index).copied().ok_or(VmError::RangeCheck);
    Ok(match &f.variant {
        Variant::Show | Variant::KShow { .. } | Variant::GlyphShow(_) => width,
        Variant::AShow { ax, ay } => add(width, to_glyph(f, *ax, *ay)),
        Variant::WidthShow { cx, cy, code: c } => {
            if i32::from(code) == *c {
                add(width, to_glyph(f, *cx, *cy))
            } else {
                width
            }
        }
        Variant::AWidthShow {
            cx,
            cy,
            code: c,
            ax,
            ay,
        } => {
            let mut d = add(width, to_glyph(f, *ax, *ay));
            if i32::from(code) == *c {
                d = add(d, to_glyph(f, *cx, *cy));
            }
            d
        }
        Variant::XShow(values) => to_glyph(f, at(values, k)?, 0.0),
        Variant::YShow(values) => to_glyph(f, 0.0, at(values, k)?),
        Variant::XYShow(values) => to_glyph(f, at(values, 2 * k)?, at(values, 2 * k + 1)?),
    })
}

fn add_glyph(f: &mut ShowFrame, code: u8, displacement: Point) {
    f.pending.push(Glyph {
        code,
        dx: displacement.x,
        dy: displacement.y,
    });
    f.total = add(f.total, displacement);
    f.next += 1;
}

/// After a glyph joined the run: for `kshow` with glyphs still to come,
/// the run is shown and the procedure gets the two codes.
fn after_glyph(i: &mut Interp, f: &mut ShowFrame) -> Result<Option<Object>, VmError> {
    let Variant::KShow { procedure } = f.variant else {
        return Ok(None);
    };
    let Some(&following) = f.codes.get(f.next) else {
        return Ok(None);
    };
    flush(i, f)?;
    let previous = f.codes[f.next - 1];
    i.push(Object::integer(i32::from(previous)))?;
    i.push(Object::integer(i32::from(following)))?;
    Ok(Some(procedure))
}

/// Hands the pending run to the backend, which advances the current
/// point; the next run starts wherever that leaves it.
fn flush(i: &mut Interp, f: &mut ShowFrame) -> Result<(), VmError> {
    if !f.pending.is_empty()
        && !f.measure
        && let Some(backend) = i.graphics_backend()
    {
        backend.show(&f.pending)?;
    }
    f.pending.clear();
    f.total = Point::default();
    f.origin = None;
    Ok(())
}

fn finish(i: &mut Interp, f: &mut ShowFrame) -> Result<(), VmError> {
    if f.measure {
        let width = f.font.matrix.apply_delta(f.total);
        i.push(Object::real(width.x))?;
        i.push(Object::real(width.y))
    } else {
        flush(i, f)
    }
}

/// Sets up a Type 3 glyph: a saved graphics state whose CTM maps glyph
/// space to the glyph's origin, capture begun in the backend, and the
/// font dictionary and glyph name (or code) on the operand stack.
fn begin_glyph(i: &mut Interp, f: &mut ShowFrame, code: u8, by_name: bool) -> Result<(), VmError> {
    let name = match glyph_name(i, f, code) {
        Some(name) => name,
        None => i.intern(".notdef"),
    };
    let name_text = i.mem.name_text(name.as_name().expect("name")).to_vec();
    let mut run = RunningGlyph {
        code,
        depth: None,
        width: None,
        bbox: None,
    };
    if i.has_graphics_backend() {
        let origin = match f.origin {
            Some(origin) => origin,
            None => {
                let origin = match i.backend()?.current_point() {
                    Ok(p) => p,
                    Err(VmError::NoCurrentPoint) if f.measure => Point::default(),
                    Err(e) => return Err(e),
                };
                f.origin = Some(origin);
                origin
            }
        };
        let at = add(origin, f.font.matrix.apply_delta(f.total));
        let backend = i.backend()?;
        run.depth = Some(backend.gstate_depth());
        backend.gsave()?;
        f.running = Some(run);
        let ctm = backend.current_matrix();
        let glyph_ctm = f
            .font
            .matrix
            .then(Matrix::translation(at.x, at.y))
            .then(ctm);
        backend.set_matrix(glyph_ctm)?;
        backend.newpath()?;
        backend.begin_glyph(f.font, code, &name_text, f.measure)?;
    } else {
        f.running = Some(run);
    }
    i.push(f.dict)?;
    if by_name {
        i.push(name)
    } else {
        i.push(Object::integer(i32::from(code)))
    }
}

/// The glyph procedure has returned: its width is read, the capture
/// ended, the graphics state restored, and the glyph joins the run.
fn finish_glyph(i: &mut Interp, f: &mut ShowFrame, run: RunningGlyph) -> Result<(), VmError> {
    let width = run.width.unwrap_or((0.0, 0.0));
    if let Some(depth) = run.depth {
        let backend = i.backend()?;
        let ended = backend.end_glyph(width, run.bbox);
        let restored = backend.grestore_to(depth);
        ended?;
        restored?;
    }
    let displacement = displacement(f, run.code, Point::new(width.0, width.1))?;
    add_glyph(f, run.code, displacement);
    Ok(())
}

/// The glyph whose procedure is running innermost, for the width
/// operators.
pub(crate) fn running_glyph(i: &mut Interp) -> Option<&mut RunningGlyph> {
    i.estack.iter_mut().rev().find_map(|frame| match frame {
        Frame::Loop(LoopFrame::Show(show)) => show.running.as_mut(),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(variant: Variant, matrix: Matrix) -> ShowFrame {
        ShowFrame {
            operator: "show",
            font: FontRef {
                instance: 0,
                matrix,
            },
            dict: Object::null(),
            kind: FontKind::Resident(StdFont::Helvetica),
            encoding: Object::null(),
            codes: vec![97, 32, 98],
            variant,
            measure: false,
            next: 0,
            pending: Vec::new(),
            total: Point::default(),
            origin: None,
            running: None,
        }
    }

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    #[test]
    fn variants_add_or_replace_in_glyph_space() {
        let m = Matrix::scaling(0.01, 0.01);
        let w = Point::new(500.0, 0.0);
        assert_eq!(displacement(&frame(Variant::Show, m), 97, w), Ok(w));
        let ashow = frame(Variant::AShow { ax: 1.0, ay: 2.0 }, m);
        assert!(close(
            displacement(&ashow, 97, w).unwrap(),
            Point::new(600.0, 200.0)
        ));
        let mut widthshow = frame(
            Variant::WidthShow {
                cx: 5.0,
                cy: 0.0,
                code: 32,
            },
            m,
        );
        assert_eq!(displacement(&widthshow, 97, w), Ok(w));
        widthshow.next = 1;
        assert!(close(
            displacement(&widthshow, 32, w).unwrap(),
            Point::new(1000.0, 0.0)
        ));
        let mut xshow = frame(Variant::XShow(vec![10.0, 20.0]), m);
        assert!(close(
            displacement(&xshow, 97, w).unwrap(),
            Point::new(1000.0, 0.0)
        ));
        xshow.next = 2;
        assert_eq!(displacement(&xshow, 98, w), Err(VmError::RangeCheck));
        let xyshow = frame(Variant::XYShow(vec![1.0, 2.0]), m);
        assert!(close(
            displacement(&xyshow, 97, w).unwrap(),
            Point::new(100.0, 200.0)
        ));
        let yshow = frame(Variant::YShow(vec![3.0]), m);
        assert!(close(
            displacement(&yshow, 97, w).unwrap(),
            Point::new(0.0, 300.0)
        ));
        let singular = frame(
            Variant::AShow { ax: 1.0, ay: 1.0 },
            Matrix::scaling(0.0, 0.0),
        );
        assert_eq!(displacement(&singular, 97, w), Ok(w));
    }

    #[test]
    fn a_run_accumulates() {
        let mut f = frame(Variant::Show, Matrix::scaling(0.001, 0.001));
        add_glyph(&mut f, 97, Point::new(556.0, 0.0));
        add_glyph(&mut f, 32, Point::new(278.0, 1.0));
        assert_eq!(f.next, 2);
        assert_eq!(f.total, Point::new(834.0, 1.0));
        assert_eq!(f.pending.len(), 2);
        assert_eq!(f.pending[1].code, 32);
        assert!(f.procedure().ty() == Type::Null);
    }
}

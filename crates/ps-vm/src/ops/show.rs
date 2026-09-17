// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The show family and `stringwidth` as a loop frame (PLRM3 §5.3, §5.7,
//! §5.11).
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
//! `charpath` runs it in outline mode: a Type 1 or Type 42 glyph's
//! outline, or a resident face's from its outline asset, is appended to
//! the current path through the font matrix at the glyph's position,
//! and the current point advances at the end.
//!
//! A Type 0 font decodes the string through its CMap: each code is one
//! to four bytes and selects a CID, which the descendant CIDFont's
//! program answers with a glyph; the run's glyphs carry the code, its
//! length, and the CID. In writing mode 1 every glyph advances downward
//! by the default vertical advance and is positioned at its vertical
//! origin.

use std::rc::Rc;

use ps_fonts::{CMap, Glyph as ProgramGlyph, OutlineOp, Program, ResidentFace};

use crate::error::VmError;
use crate::graphics::{
    Bounds, FontInfo, FontRef, FontSource, Glyph, Matrix, Point, apply64, compose64, envelope64,
};
use crate::interp::{Frame, Interp, LoopFrame};
use crate::object::{Object, Type};
use crate::ops::array::items;
use crate::ops::cidinit;
use crate::ops::font::{self, entry};
use crate::ops::pattern;

/// The glyph units of an em in a program's glyph space: a thousand for
/// charstring programs, one for TrueType after the scale to the unit em.
fn em_units(program: &Program, scale: f32) -> f32 {
    program
        .units_per_em()
        .map_or(1000.0, |units| f32::from(units) * scale)
}

/// The default vertical origin, as a fraction of the em above the
/// horizontal origin (PLRM3 §5.11.2: the position vector is `(w0/2,
/// 880)` in thousandths).
const VERTICAL_ORIGIN_Y: f32 = 0.88;

/// How a font's glyphs are produced.
#[derive(Clone, Debug)]
pub(crate) enum FontKind {
    /// Widths from the resident metrics, outlines from the face's asset
    /// when it has one; nothing is executed.
    Resident(ResidentFace),
    /// `BuildGlyph` (glyph names) or `BuildChar` (codes) is run per glyph.
    Type3 { build: Object, by_name: bool },
    /// A Type 1, Type 2 (CFF), or Type 42 program the job defined;
    /// `scale` takes the program's glyph units to the space the font
    /// matrix maps (one for charstring units, the reciprocal of the units
    /// per em for TrueType).
    Embedded { program: Rc<Program>, scale: f32 },
    /// A Type 0 font: the CMap decodes the string and the descendant
    /// answers CIDs.
    Composite(Box<Composite>),
}

#[derive(Clone, Debug)]
pub(crate) struct Composite {
    pub(crate) cmap: Rc<CMap>,
    /// The descendant index each font number selects.
    pub(crate) numbers: Vec<usize>,
    /// The descendant font number 0 selects, the one the backend was
    /// told about; a code selecting another is `invalidfont`.
    pub(crate) descendant: Descendant,
}

#[derive(Clone, Debug)]
pub(crate) struct Descendant {
    pub(crate) dict: Object,
    /// The descendant's `Encoding` for a simple descendant, null for a
    /// CIDFont.
    pub(crate) encoding: Object,
    pub(crate) kind: DescendantKind,
}

#[derive(Clone, Debug)]
pub(crate) enum DescendantKind {
    /// A CIDFont: glyphs by CID through its program.
    Cid { program: Rc<Program>, scale: f32 },
    /// A simple font, whose glyph the CID selects as a one-byte code.
    Simple(FontKind),
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

/// One code of the string: its bytes as a value, how many, the CID it
/// selects (the code itself for a simple font), and the CMap's font
/// number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Code {
    code: u32,
    len: u8,
    cid: u16,
    font: u8,
}

impl Code {
    fn simple(byte: u8) -> Self {
        Code {
            code: u32::from(byte),
            len: 1,
            cid: u16::from(byte),
            font: 0,
        }
    }
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
    /// The font's `Encoding`; null for a Type 0 font, whose descendant
    /// carries its own.
    pub(crate) encoding: Object,
    pub(crate) codes: Vec<u8>,
    pub(crate) variant: Variant,
    pub(crate) measure: bool,
    /// `charpath`: outlines join the current path and no run is shown.
    pub(crate) outline: bool,
    /// The byte the next code starts at.
    pub(crate) next: usize,
    /// How many glyphs have been decoded, which the positioning variants
    /// index their numbers by.
    pub(crate) glyphs: usize,
    /// The code of the last glyph decoded, for `kshow`.
    pub(crate) last_code: u32,
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
        match (&self.variant, &self.kind) {
            (Variant::KShow { procedure }, _) => *procedure,
            (_, FontKind::Type3 { build, .. }) => *build,
            _ => Object::null(),
        }
    }

    pub(crate) fn references(&self) -> Vec<Object> {
        let mut objects = vec![self.dict, self.encoding];
        if let FontKind::Type3 { build, .. } = &self.kind {
            objects.push(*build);
        }
        if let FontKind::Composite(composite) = &self.kind {
            objects.push(composite.descendant.dict);
            objects.push(composite.descendant.encoding);
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
/// marker must carry a program the snapshot can be built from, and a
/// CIDFont draws only as a descendant of a Type 0 font.
pub(crate) fn font_kind(i: &mut Interp, dict: Object) -> Result<FontKind, VmError> {
    if font::is_cidfont(i, dict)? {
        return Err(VmError::InvalidFont);
    }
    let font_type = entry(i, dict, "FontType")?.and_then(Object::as_i32);
    match font_type {
        Some(0) => composite_kind(i, dict),
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
        Some(1 | 2 | 42) => {
            let resident = entry(i, dict, "ResidentFont")?
                .and_then(Object::as_i32)
                .and_then(|n| usize::try_from(n).ok())
                .and_then(ResidentFace::from_index);
            match resident {
                Some(font) => Ok(FontKind::Resident(font)),
                None => {
                    let program = i.font_program(dict)?;
                    let scale = program
                        .units_per_em()
                        .map_or(1.0, |units| 1.0 / f32::from(units));
                    Ok(FontKind::Embedded { program, scale })
                }
            }
        }
        _ => Err(VmError::InvalidFont),
    }
}

/// A Type 0 font's CMap and its first descendant. The CMap is the
/// dictionary or, for a name, the defined or already loaded predefined
/// resource (`definefont` loaded it); a descendant that is a CIDFont
/// answers by CID through its program, a simple one by code; a Type 3
/// or Type 0 descendant is `invalidfont`.
fn composite_kind(i: &mut Interp, dict: Object) -> Result<FontKind, VmError> {
    let cmap = entry(i, dict, "CMap")?.ok_or(VmError::InvalidFont)?;
    let cmap = match cmap.ty() {
        Type::Dict => cmap,
        Type::Name => {
            let key = i.mem.dict_key(cmap)?;
            let category = i.cmap_category;
            let text = i.mem.name_text(key.as_name().expect("name")).to_vec();
            i.mem
                .dict_get(category.local, key)?
                .or(i.mem.dict_get(category.global, key)?)
                .or_else(|| i.predefined_cmap(&text))
                .ok_or(VmError::InvalidFont)?
        }
        _ => return Err(VmError::InvalidFont),
    };
    let cmap = cidinit::cmap_of(i, cmap)?;
    let numbers = font::font_numbers(i, dict)?;
    let descendant = font::first_descendant(i, dict)?;
    let (encoding, kind) = if font::is_cidfont(i, descendant)? {
        let program = i.font_program(descendant)?;
        let scale = program
            .units_per_em()
            .map_or(1.0, |units| 1.0 / f32::from(units));
        (Object::null(), DescendantKind::Cid { program, scale })
    } else {
        let kind = font_kind(i, descendant)?;
        if !matches!(kind, FontKind::Resident(_) | FontKind::Embedded { .. }) {
            return Err(VmError::InvalidFont);
        }
        let encoding = entry(i, descendant, "Encoding")?
            .filter(|e| is_array(*e))
            .ok_or(VmError::InvalidFont)?;
        (encoding, DescendantKind::Simple(kind))
    };
    Ok(FontKind::Composite(Box::new(Composite {
        cmap,
        numbers,
        descendant: Descendant {
            dict: descendant,
            encoding,
            kind,
        },
    })))
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
    start(i, operator, variant, codes, measure, false, operands)
}

/// Starts `charpath` of `codes`: outline mode, which a Type 1 or Type 42
/// program and a resident face with an outline asset support; Symbol,
/// ZapfDingbats, Type 3 fonts, and every resident face in a build
/// without the assets are `invalidfont`.
pub(crate) fn begin_charpath(i: &mut Interp, codes: Vec<u8>) -> Result<(), VmError> {
    start(i, "charpath", Variant::Show, codes, false, true, 2)
}

fn can_outline(kind: &FontKind) -> bool {
    match kind {
        FontKind::Embedded { .. } => true,
        FontKind::Resident(face) => face.has_outlines(),
        FontKind::Type3 { .. } => false,
        FontKind::Composite(composite) => match &composite.descendant.kind {
            DescendantKind::Cid { .. } => true,
            DescendantKind::Simple(kind) => can_outline(kind),
        },
    }
}

fn start(
    i: &mut Interp,
    operator: &'static str,
    variant: Variant,
    codes: Vec<u8>,
    measure: bool,
    outline: bool,
    operands: usize,
) -> Result<(), VmError> {
    let font = i.current_font().ok_or(VmError::InvalidFont)?;
    let dict = i.font_dict(font.instance).ok_or(VmError::InvalidFont)?;
    let kind = font_kind(i, dict)?;
    if outline && !can_outline(&kind) {
        return Err(VmError::InvalidFont);
    }
    let encoding = match &kind {
        FontKind::Composite(_) => Object::null(),
        _ => entry(i, dict, "Encoding")?
            .filter(|e| is_array(*e))
            .ok_or(VmError::InvalidFont)?,
    };
    let codes = match variant {
        Variant::GlyphShow(name) => vec![glyphshow_code(i, encoding, name, &kind)?],
        _ => codes,
    };
    let glyph_count = match &kind {
        FontKind::Composite(composite) => composite.cmap.decode_all(&codes).len(),
        _ => codes.len(),
    };
    let needed = match &variant {
        Variant::XShow(values) | Variant::YShow(values) => Some((values.len(), glyph_count)),
        Variant::XYShow(values) => Some((values.len(), 2 * glyph_count)),
        _ => None,
    };
    if needed.is_some_and(|(have, need)| have < need) {
        return Err(VmError::RangeCheck);
    }
    if !measure {
        i.backend()?.current_point()?;
    }
    if i.has_graphics_backend() && !i.font_described(font.instance) {
        let info = describe(i, dict, &kind, encoding, font.matrix)?;
        i.backend()?.define_font(font.instance, &info)?;
        i.mark_font_described(font.instance);
    }
    // Text is painted with the current colour: a pattern's cell is
    // captured first, and the operator runs again over its operands.
    if !measure && !outline && pattern::capture_cell(i, operator)? {
        return Ok(());
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
        outline,
        next: 0,
        glyphs: 0,
        last_code: 0,
        pending: Vec::new(),
        total: Point::default(),
        origin: None,
        running: None,
    }))))
}

/// The encoding's names, `None` where an entry is not a name.
fn encoding_names(i: &Interp, encoding: Object) -> Result<Vec<Option<Vec<u8>>>, VmError> {
    if encoding.ty() == Type::Null {
        return Ok(vec![None; 256]);
    }
    Ok(items(i, encoding)?
        .into_iter()
        .map(|entry| entry.as_name().map(|atom| i.mem.name_text(atom).to_vec()))
        .collect())
}

/// What the backend needs to know about the font: its glyph source and
/// its encoding as names. A Type 0 font's own `Encoding` lists
/// descendant indices; what the backend can use is a simple
/// descendant's encoding, through which each CID selects a glyph, so
/// that is the one described (a CIDFont descendant has none).
fn describe(
    i: &mut Interp,
    dict: Object,
    kind: &FontKind,
    encoding: Object,
    matrix: Matrix,
) -> Result<FontInfo, VmError> {
    let encoding = match kind {
        FontKind::Composite(composite) => composite.descendant.encoding,
        _ => encoding,
    };
    let encoding = encoding_names(i, encoding)?;
    let source = source_of(i, dict, kind, matrix)?;
    Ok(FontInfo { source, encoding })
}

fn name_entry(i: &mut Interp, dict: Object, key: &str) -> Result<Vec<u8>, VmError> {
    Ok(match entry(i, dict, key)? {
        Some(name) if name.ty() == Type::Name => {
            i.mem.name_text(name.as_name().expect("name")).to_vec()
        }
        Some(name) if name.ty() == Type::String => {
            i.mem.string(name).map(<[u8]>::to_vec).unwrap_or_default()
        }
        _ => Vec::new(),
    })
}

/// The source of a font's glyphs. A Type 3 font is identified by its
/// `FID` and carries the matrix it was defined with, so a scaled
/// instance records its glyphs in the same space as the original. A
/// resident face outside the fourteen whose outline asset is embedded
/// is described as an embedded font built from that asset, so the
/// output carries its program; the fourteen stay resident. A Type 0
/// font carries its CMap and the source of its first descendant.
fn source_of(
    i: &mut Interp,
    dict: Object,
    kind: &FontKind,
    matrix: Matrix,
) -> Result<FontSource, VmError> {
    let family = entry(i, dict, "FID")?
        .and_then(Object::as_font_id)
        .unwrap_or(u32::MAX);
    Ok(match kind {
        FontKind::Resident(face) => match face.outlines().filter(|_| face.std_font().is_none()) {
            Some(outlines) => FontSource::Embedded {
                family,
                kind: outlines.program().kind(),
                program: outlines.program().clone(),
                font_matrix: Matrix::scaling(0.001, 0.001),
                font_name: outlines.font_name().to_vec(),
            },
            None => FontSource::Resident(*face),
        },
        FontKind::Embedded { program, .. } => FontSource::Embedded {
            family,
            kind: program.kind(),
            program: program.clone(),
            font_matrix: i.defined_matrix(family).unwrap_or(matrix),
            font_name: name_entry(i, dict, "FontName")?,
        },
        FontKind::Type3 { .. } => {
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
        FontKind::Composite(composite) => {
            let descendant = &composite.descendant;
            let inner = match &descendant.kind {
                DescendantKind::Cid { program, .. } => {
                    let cid_family = entry(i, descendant.dict, "FID")?
                        .and_then(Object::as_font_id)
                        .unwrap_or(u32::MAX);
                    let mut font_name = name_entry(i, descendant.dict, "CIDFontName")?;
                    if font_name.is_empty() {
                        font_name = name_entry(i, descendant.dict, "FontName")?;
                    }
                    let font_matrix = match i.defined_matrix(cid_family) {
                        Some(m) => m,
                        None => font::effective_matrix(i, descendant.dict)?,
                    };
                    FontSource::Embedded {
                        family: cid_family,
                        kind: program.kind(),
                        program: program.clone(),
                        font_matrix,
                        font_name,
                    }
                }
                DescendantKind::Simple(kind) => {
                    let inner_matrix = font::effective_matrix(i, descendant.dict)?;
                    source_of(i, descendant.dict, kind, inner_matrix)?
                }
            };
            let cmap = composite.cmap.clone();
            FontSource::Composite {
                family,
                cmap_name: cmap.name.clone(),
                wmode: cmap.wmode,
                unicode_based: cmap.unicode_based,
                cmap,
                descendant: Box::new(inner),
            }
        }
    })
}

// The code the encoding gives the name; a name outside the encoding can
// still be shown by name (resident metrics, `BuildGlyph`) and is recorded
// under code 0, but a `BuildChar` font has nothing to run for it, and a
// Type 0 font selects glyphs by CID, not by name.
fn glyphshow_code(
    i: &Interp,
    encoding: Object,
    name: Object,
    kind: &FontKind,
) -> Result<u8, VmError> {
    if matches!(kind, FontKind::Composite(_)) {
        return Err(VmError::InvalidFont);
    }
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
        && i.has_graphics_backend()
    {
        let _ = i.backend().and_then(|b| b.end_glyph((0.0, 0.0), None));
        let _ = i.grestore_to(depth);
    }
}

/// The code at the frame's position, `None` at the end of the string.
fn next_code(f: &ShowFrame) -> Option<Code> {
    let rest = f.codes.get(f.next..)?;
    let &first = rest.first()?;
    Some(match &f.kind {
        FontKind::Composite(composite) => {
            let decoded = composite.cmap.decode(rest);
            Code {
                code: decoded.code,
                len: decoded.len,
                cid: decoded.cid.unwrap_or(0),
                font: decoded.font,
            }
        }
        _ => Code::simple(first),
    })
}

/// A simple font's glyph for a one-byte code: the width in glyph units
/// and, when the font has outlines, the program glyph with the scale
/// its units need. A resident face advances a name it lacks by its
/// `.notdef` width and draws nothing for it. A charstring or glyph
/// record that cannot be interpreted is `invalidfont`.
type SimpleGlyph = (Point, Option<(Rc<ProgramGlyph>, f32)>);

fn simple_glyph(
    i: &mut Interp,
    f: &ShowFrame,
    kind: &FontKind,
    encoding: Object,
    code: u8,
) -> Result<SimpleGlyph, VmError> {
    match kind {
        FontKind::Resident(face) => {
            let name = glyph_name(i, f, encoding, code)
                .map(|name| i.mem.name_text(name.as_name().expect("name")).to_vec());
            let width = name
                .as_deref()
                .and_then(|name| face.width(std::str::from_utf8(name).ok()?))
                .map_or_else(|| face.notdef_width(), f32::from);
            let outline = match (&name, f.outline) {
                (Some(name), true) => face
                    .outline(name)
                    .map_err(|_| VmError::InvalidFont)?
                    .map(|glyph| (glyph, 1.0)),
                _ => None,
            };
            Ok((Point::new(width, 0.0), outline))
        }
        FontKind::Embedded { program, scale } => {
            let glyph = program_glyph(i, f, encoding, program, code)?;
            let width = glyph.as_ref().map_or(Point::default(), |g| {
                Point::new(g.advance.0 * scale, g.advance.1 * scale)
            });
            let outline = glyph.filter(|_| f.outline).map(|g| (g, *scale));
            Ok((width, outline))
        }
        FontKind::Type3 { .. } | FontKind::Composite(_) => Err(VmError::InvalidFont),
    }
}

/// The glyph a CID selects: the program's glyph for it, else its
/// notdef (CID 0), else nothing.
fn cid_glyph(program: &Program, cid: u16) -> Result<Option<Rc<ProgramGlyph>>, VmError> {
    let lookup = |cid: u16| program.glyph_by_cid(cid).map_err(|_| VmError::InvalidFont);
    if let Some(glyph) = lookup(cid)? {
        return Ok(Some(glyph));
    }
    lookup(0)
}

fn advance(i: &mut Interp, f: &mut ShowFrame) -> Result<Next, VmError> {
    if let Some(run) = f.running.take() {
        finish_glyph(i, f, run)?;
        if let Some(procedure) = after_glyph(i, f)? {
            return Ok(Next::Run(procedure));
        }
    }
    loop {
        let Some(code) = next_code(f) else {
            finish(i, f)?;
            return Ok(Next::Done);
        };
        match &f.kind {
            FontKind::Resident(_) | FontKind::Embedded { .. } => {
                let kind = f.kind.clone();
                let (width, outline) = simple_glyph(i, f, &kind, f.encoding, code.code as u8)?;
                if let Some((glyph, scale)) = outline {
                    append_outline(i, f, &glyph, scale, Point::default())?;
                }
                let displacement = displacement(f, code.code, width)?;
                add_glyph(f, code, displacement);
            }
            FontKind::Type3 { build, by_name } => {
                let (build, by_name) = (*build, *by_name);
                begin_glyph(i, f, code.code as u8, by_name)?;
                return Ok(Next::Run(build));
            }
            FontKind::Composite(composite) => {
                let composite = composite.clone();
                let selected = composite.numbers.get(usize::from(code.font));
                if selected != composite.numbers.first() {
                    return Err(VmError::InvalidFont);
                }
                let (width, outline, em) = match &composite.descendant.kind {
                    DescendantKind::Cid { program, scale } => {
                        let glyph = cid_glyph(program, code.cid)?;
                        let width = glyph.as_ref().map_or(Point::default(), |g| {
                            Point::new(g.advance.0 * scale, g.advance.1 * scale)
                        });
                        let outline = glyph.filter(|_| f.outline).map(|g| (g, *scale));
                        (width, outline, em_units(program, *scale))
                    }
                    DescendantKind::Simple(kind) => {
                        let byte = u8::try_from(code.cid).unwrap_or(0);
                        let (width, outline) =
                            simple_glyph(i, f, kind, composite.descendant.encoding, byte)?;
                        let em = match kind {
                            FontKind::Embedded { program, scale } => em_units(program, *scale),
                            _ => 1000.0,
                        };
                        (width, outline, em)
                    }
                };
                // Writing mode 1: the glyph sits at its vertical origin
                // and the pen moves down by the default vertical advance.
                let (width, shift) = if composite.cmap.wmode == 1 {
                    (
                        Point::new(0.0, -em),
                        Point::new(-width.x / 2.0, -VERTICAL_ORIGIN_Y * em),
                    )
                } else {
                    (width, Point::default())
                };
                if let Some((glyph, scale)) = outline {
                    append_outline(i, f, &glyph, scale, shift)?;
                }
                let displacement = displacement(f, code.code, width)?;
                add_glyph(f, code, displacement);
            }
        }
        if let Some(procedure) = after_glyph(i, f)? {
            return Ok(Next::Run(procedure));
        }
    }
}

/// The program's glyph for `code`: by the encoding's name, else the
/// program's `.notdef`, else nothing (a zero-width blank). A charstring
/// or glyph record that cannot be interpreted is `invalidfont`.
fn program_glyph(
    i: &Interp,
    f: &ShowFrame,
    encoding: Object,
    program: &Program,
    code: u8,
) -> Result<Option<Rc<ProgramGlyph>>, VmError> {
    let lookup = |name: &[u8]| program.glyph(name).map_err(|_| VmError::InvalidFont);
    if let Some(name) = glyph_name(i, f, encoding, code) {
        let text = i.mem.name_text(name.as_name().expect("name")).to_vec();
        if let Some(glyph) = lookup(&text)? {
            return Ok(Some(glyph));
        }
    }
    lookup(b".notdef")
}

/// Appends a glyph's outline to the current path: glyph units scaled,
/// shifted by `shift` (the vertical origin in writing mode 1), taken
/// through the font matrix, and moved to the glyph's position in user
/// space; the backend applies the CTM.
fn append_outline(
    i: &mut Interp,
    f: &mut ShowFrame,
    glyph: &ProgramGlyph,
    scale: f32,
    shift: Point,
) -> Result<(), VmError> {
    let origin = match f.origin {
        Some(origin) => origin,
        None => {
            let origin = i.backend()?.current_point()?;
            f.origin = Some(origin);
            origin
        }
    };
    let at = add(origin, f.font.matrix.apply_delta(f.total));
    let matrix = f.font.matrix;
    let map = |x: f32, y: f32| {
        add(
            matrix.apply(Point::new(x * scale + shift.x, y * scale + shift.y)),
            at,
        )
    };
    let backend = i.backend()?;
    for op in &glyph.outline.ops {
        match *op {
            OutlineOp::MoveTo(x, y) => backend.moveto(map(x, y))?,
            OutlineOp::LineTo(x, y) => backend.lineto(map(x, y))?,
            OutlineOp::CurveTo(x1, y1, x2, y2, x, y) => {
                backend.curveto(map(x1, y1), map(x2, y2), map(x, y))?;
            }
            OutlineOp::Close => backend.closepath()?,
        }
    }
    Ok(())
}

/// The glyph name a code selects: the `glyphshow` name, else the
/// encoding's entry when it is a name.
fn glyph_name(i: &Interp, f: &ShowFrame, encoding: Object, code: u8) -> Option<Object> {
    if let Variant::GlyphShow(name) = f.variant {
        return Some(name);
    }
    i.mem
        .array(encoding)?
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

/// The displacement the next glyph gets: its width plus the variant's
/// addition, or the variant's replacement.
fn displacement(f: &ShowFrame, code: u32, width: Point) -> Result<Point, VmError> {
    let k = f.glyphs;
    let at = |values: &[f32], index: usize| values.get(index).copied().ok_or(VmError::RangeCheck);
    let is = |c: i32| i64::from(code) == i64::from(c);
    Ok(match &f.variant {
        Variant::Show | Variant::KShow { .. } | Variant::GlyphShow(_) => width,
        Variant::AShow { ax, ay } => add(width, to_glyph(f, *ax, *ay)),
        Variant::WidthShow { cx, cy, code: c } => {
            if is(*c) {
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
            if is(*c) {
                d = add(d, to_glyph(f, *cx, *cy));
            }
            d
        }
        Variant::XShow(values) => to_glyph(f, at(values, k)?, 0.0),
        Variant::YShow(values) => to_glyph(f, 0.0, at(values, k)?),
        Variant::XYShow(values) => to_glyph(f, at(values, 2 * k)?, at(values, 2 * k + 1)?),
    })
}

fn add_glyph(f: &mut ShowFrame, code: Code, displacement: Point) {
    f.pending.push(Glyph {
        code: code.code,
        len: code.len,
        cid: code.cid,
        dx: displacement.x,
        dy: displacement.y,
    });
    f.total = add(f.total, displacement);
    f.next += usize::from(code.len);
    f.glyphs += 1;
    f.last_code = code.code;
}

/// A code as the integer `kshow` hands its procedure.
fn code_object(code: u32) -> Object {
    Object::integer(i32::try_from(code).unwrap_or(i32::MAX))
}

/// After a glyph joined the run: for `kshow` with glyphs still to come,
/// the run is shown and the procedure gets the two codes.
fn after_glyph(i: &mut Interp, f: &mut ShowFrame) -> Result<Option<Object>, VmError> {
    let Variant::KShow { procedure } = f.variant else {
        return Ok(None);
    };
    let Some(following) = next_code(f) else {
        return Ok(None);
    };
    flush(i, f)?;
    i.push(code_object(f.last_code))?;
    i.push(code_object(following.code))?;
    Ok(Some(procedure))
}

/// Hands the pending run to the backend, which advances the current
/// point; the next run starts wherever that leaves it. In outline mode
/// the outlines are already in the path and the current point moves to
/// the end of the run.
fn flush(i: &mut Interp, f: &mut ShowFrame) -> Result<(), VmError> {
    if f.outline {
        let origin = match f.origin {
            Some(origin) => origin,
            None => i.backend()?.current_point()?,
        };
        let end = add(origin, f.font.matrix.apply_delta(f.total));
        i.backend()?.moveto(end)?;
    } else if !f.pending.is_empty()
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
    let name = match glyph_name(i, f, f.encoding, code) {
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
        run.depth = Some(i.backend()?.gstate_depth());
        i.gsave()?;
        // A glyph procedure starts without stroke adjustment (PLRM3
        // §8.2 `setstrokeadjust`); the saved state brings it back.
        i.set_stroke_adjust(false);
        f.running = Some(run);
        let backend = i.backend()?;
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
        let ended = i.backend()?.end_glyph(width, run.bbox);
        let restored = i.grestore_to(depth);
        ended?;
        restored?;
    }
    let code = Code::simple(run.code);
    let displacement = displacement(f, code.code, Point::new(width.0, width.1))?;
    add_glyph(f, code, displacement);
    Ok(())
}

/// The metrics a glyph procedure declares: its width vector, the box a
/// `setcachedevice` gives, and for `setcachedevice2` the writing-mode-1
/// width and the vertical origin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Metrics {
    pub(crate) width: (f32, f32),
    pub(crate) bbox: Option<Bounds>,
    pub(crate) vertical: Option<Vertical>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Vertical {
    pub(crate) width: (f32, f32),
    pub(crate) origin: (f32, f32),
}

/// `metrics`, declared under the CTM `current`, carried into glyph
/// space, whose CTM is `glyph` (PLRM3 §5.4 and the `setcachedevice`
/// entry: the operands are glyph-space numbers, which the reference
/// reads through the CTM in effect at the call). Vectors go through the
/// delta transform of `current × glyph⁻¹`, the box through the full
/// transform and its axis-aligned envelope, computed in double precision
/// and rounded once. An unchanged CTM returns the operands untouched, so
/// a procedure that declares before transforming sees no rounding; a
/// singular glyph matrix has no inverse and does the same.
pub(crate) fn into_glyph_space(current: Matrix, glyph: Matrix, metrics: Metrics) -> Metrics {
    if current == glyph {
        return metrics;
    }
    let Some(back) = glyph.inverse64() else {
        return metrics;
    };
    let carry = compose64(current.as_f64(), back);
    let [a, b, c, d, _, _] = carry;
    let vector = |(x, y): (f32, f32)| {
        let (x, y) = (f64::from(x), f64::from(y));
        (single(a * x + c * y), single(b * x + d * y))
    };
    let bbox = metrics.bbox.map(|box_| {
        let corners = [
            (box_.llx, box_.lly),
            (box_.urx, box_.lly),
            (box_.urx, box_.ury),
            (box_.llx, box_.ury),
        ]
        .map(|(x, y)| apply64(carry, f64::from(x), f64::from(y)));
        let [llx, lly, urx, ury] = envelope64(&corners);
        Bounds::new(single(llx), single(lly), single(urx), single(ury))
    });
    Metrics {
        width: vector(metrics.width),
        bbox,
        vertical: metrics.vertical.map(|v| Vertical {
            width: vector(v.width),
            origin: vector(v.origin),
        }),
    }
}

// Adding zero turns a negative zero into a plain one.
fn single(value: f64) -> f32 {
    value as f32 + 0.0
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
            kind: FontKind::Resident(ResidentFace::Helvetica),
            encoding: Object::null(),
            codes: vec![97, 32, 98],
            variant,
            measure: false,
            outline: false,
            next: 0,
            glyphs: 0,
            last_code: 0,
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
        widthshow.glyphs = 1;
        assert!(close(
            displacement(&widthshow, 32, w).unwrap(),
            Point::new(1000.0, 0.0)
        ));
        let mut xshow = frame(Variant::XShow(vec![10.0, 20.0]), m);
        assert!(close(
            displacement(&xshow, 97, w).unwrap(),
            Point::new(1000.0, 0.0)
        ));
        xshow.glyphs = 2;
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
        assert_eq!(next_code(&f), Some(Code::simple(97)));
        add_glyph(&mut f, Code::simple(97), Point::new(556.0, 0.0));
        add_glyph(&mut f, Code::simple(32), Point::new(278.0, 1.0));
        assert_eq!(f.next, 2);
        assert_eq!(f.glyphs, 2);
        assert_eq!(f.last_code, 32);
        assert_eq!(f.total, Point::new(834.0, 1.0));
        assert_eq!(f.pending.len(), 2);
        assert_eq!(f.pending[1].code, 32);
        assert_eq!(f.pending[1].cid, 32);
        assert_eq!(f.pending[1].len, 1);
        assert!(f.procedure().ty() == Type::Null);
        f.next = 3;
        assert_eq!(next_code(&f), None);
    }

    fn declared(width: (f32, f32), bbox: Option<Bounds>) -> Metrics {
        Metrics {
            width,
            bbox,
            vertical: None,
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn bounds_near(a: Option<Bounds>, b: Bounds) -> bool {
        let a = a.expect("a box");
        near(a.llx, b.llx) && near(a.lly, b.lly) && near(a.urx, b.urx) && near(a.ury, b.ury)
    }

    #[test]
    fn metrics_under_an_unchanged_ctm_are_the_operands_bit_for_bit() {
        let glyph = Matrix([0.02, 0.0, 0.0, 0.02, 100.1, 100.7]);
        let metrics = declared((1000.3, 0.1), Some(Bounds::new(-1.5, -2.5, 750.7, 751.9)));
        assert_eq!(into_glyph_space(glyph, glyph, metrics), metrics);
    }

    #[test]
    fn a_scale_before_the_declaration_shrinks_width_and_box() {
        let glyph = Matrix([0.02, 0.0, 0.0, 0.02, 100.0, 100.0]);
        let current = Matrix::scaling(0.5, 0.5).then(glyph);
        let carried = into_glyph_space(
            current,
            glyph,
            declared((1200.0, 0.0), Some(Bounds::new(0.0, 0.0, 1200.0, 1200.0))),
        );
        assert!(near(carried.width.0, 600.0) && near(carried.width.1, 0.0));
        assert!(bounds_near(
            carried.bbox,
            Bounds::new(0.0, 0.0, 600.0, 600.0)
        ));
        assert_eq!(carried.vertical, None);
    }

    #[test]
    fn a_translation_moves_the_box_and_leaves_the_width() {
        let glyph = Matrix([0.02, 0.0, 0.0, 0.02, 100.0, 100.0]);
        let current = Matrix::translation(50.0, -20.0).then(glyph);
        let carried = into_glyph_space(
            current,
            glyph,
            declared((600.0, 0.0), Some(Bounds::new(0.0, 0.0, 100.0, 200.0))),
        );
        assert!(near(carried.width.0, 600.0) && near(carried.width.1, 0.0));
        assert!(bounds_near(
            carried.bbox,
            Bounds::new(50.0, -20.0, 150.0, 180.0)
        ));
    }

    #[test]
    fn a_rotation_turns_the_width_and_envelopes_the_box() {
        let glyph = Matrix([0.02, 0.0, 0.0, 0.02, 100.0, 100.0]);
        let current = Matrix::rotation(90.0).then(glyph);
        let carried = into_glyph_space(
            current,
            glyph,
            Metrics {
                width: (600.0, 0.0),
                bbox: Some(Bounds::new(0.0, 0.0, 600.0, 400.0)),
                vertical: Some(Vertical {
                    width: (0.0, -1000.0),
                    origin: (300.0, 800.0),
                }),
            },
        );
        assert!(near(carried.width.0, 0.0) && near(carried.width.1, 600.0));
        assert!(bounds_near(
            carried.bbox,
            Bounds::new(-400.0, 0.0, 0.0, 600.0)
        ));
        let vertical = carried.vertical.expect("carried");
        assert!(near(vertical.width.0, 1000.0) && near(vertical.width.1, 0.0));
        assert!(near(vertical.origin.0, -800.0) && near(vertical.origin.1, 300.0));
    }

    #[test]
    fn a_singular_glyph_matrix_leaves_the_operands() {
        let glyph = Matrix::scaling(0.0, 0.02);
        let current = Matrix::scaling(2.0, 2.0).then(glyph);
        let metrics = declared((500.0, 0.0), None);
        assert_eq!(into_glyph_space(current, glyph, metrics), metrics);
    }

    #[test]
    fn em_units_follow_the_program_kind() {
        let cff = ps_fonts::testing::corpus_cff().program().unwrap();
        assert_eq!(em_units(&cff, 1.0), 1000.0);
        let tt = ps_fonts::testing::corpus_truetype().program().unwrap();
        assert_eq!(em_units(&tt, 1.0 / 2048.0), 1.0);
    }
}

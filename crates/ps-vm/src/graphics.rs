// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The boundary between the VM and a graphics implementation.
//!
//! The graphics operators pop and type-check their operands and call a
//! [`GraphicsBackend`], which owns every piece of graphics state: the
//! graphics-state stack, the CTM, colour, the current path, the clip, and
//! the page. The trait speaks numbers and the small value types defined
//! here, never objects or interpreter state, so it can be implemented and
//! tested without an interpreter. Nothing behind it ever executes
//! PostScript: procedure-driven work (image data, tint transforms) is
//! resolved by the operators before the call.

use std::rc::Rc;

use ps_fonts::{CMap, Program, ProgramKind, ResidentFace};

use crate::error::VmError;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Point { x, y }
    }
}

/// A rectangle given as origin and extent, as the `rect…` operators take
/// it. Width and height may be negative.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A rectangle given by its lower-left and upper-right corners, as
/// `pathbbox` returns it and as a media box is given.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub llx: f32,
    pub lly: f32,
    pub urx: f32,
    pub ury: f32,
}

impl Bounds {
    pub const fn new(llx: f32, lly: f32, urx: f32, ury: f32) -> Self {
        Bounds { llx, lly, urx, ury }
    }
}

/// A transformation matrix in the PostScript element order `[a b c d tx
/// ty]`: `x' = a·x + c·y + tx`, `y' = b·x + d·y + ty`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Matrix(pub [f32; 6]);

impl Default for Matrix {
    fn default() -> Self {
        Matrix::IDENTITY
    }
}

impl Matrix {
    pub const IDENTITY: Matrix = Matrix([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub const fn translation(tx: f32, ty: f32) -> Self {
        Matrix([1.0, 0.0, 0.0, 1.0, tx, ty])
    }

    pub const fn scaling(sx: f32, sy: f32) -> Self {
        Matrix([sx, 0.0, 0.0, sy, 0.0, 0.0])
    }

    /// A rotation by `degrees`, counter-clockwise for positive angles.
    pub fn rotation(degrees: f32) -> Self {
        let (sin, cos) = degrees.to_radians().sin_cos();
        Matrix([cos, sin, -sin, cos, 0.0, 0.0])
    }

    /// The matrix applying `self` first and `other` second: what
    /// `concatmatrix` computes and what `concat` does to the CTM with
    /// `other` as the CTM.
    pub fn then(self, other: Matrix) -> Matrix {
        let [a, b, c, d, tx, ty] = self.0;
        let [a2, b2, c2, d2, tx2, ty2] = other.0;
        Matrix([
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            tx * a2 + ty * c2 + tx2,
            tx * b2 + ty * d2 + ty2,
        ])
    }

    /// The inverse, or `None` for a singular matrix.
    pub fn inverse(self) -> Option<Matrix> {
        let [a, b, c, d, tx, ty] = self.0;
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        // Adding zero turns a negative zero into a plain one.
        Some(Matrix(
            [ia, ib, ic, id, -(tx * ia + ty * ic), -(tx * ib + ty * id)].map(|v| v + 0.0),
        ))
    }

    pub fn apply(self, p: Point) -> Point {
        let [a, b, c, d, tx, ty] = self.0;
        Point::new(a * p.x + c * p.y + tx, b * p.x + d * p.y + ty)
    }

    /// Transforms a distance vector: the matrix without its translation.
    pub fn apply_delta(self, p: Point) -> Point {
        let [a, b, c, d, _, _] = self.0;
        Point::new(a * p.x + c * p.y, b * p.x + d * p.y)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineCap {
    #[default]
    Butt,
    Round,
    Square,
}

impl LineCap {
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(LineCap::Butt),
            1 => Some(LineCap::Round),
            2 => Some(LineCap::Square),
            _ => None,
        }
    }

    pub fn code(self) -> i32 {
        self as i32
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

impl LineJoin {
    pub fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(LineJoin::Miter),
            1 => Some(LineJoin::Round),
            2 => Some(LineJoin::Bevel),
            _ => None,
        }
    }

    pub fn code(self) -> i32 {
        self as i32
    }
}

/// A colour space as data. Tint transforms are carried as PostScript
/// source text (the procedure's syntactic form), never evaluated here.
#[derive(Clone, Debug, PartialEq)]
pub enum SpaceSpec {
    DeviceGray,
    DeviceRGB,
    DeviceCMYK,
    Separation {
        name: Vec<u8>,
        alternate: Box<SpaceSpec>,
        tint_source: Vec<u8>,
    },
    DeviceN {
        names: Vec<Vec<u8>>,
        alternate: Box<SpaceSpec>,
        tint_source: Vec<u8>,
    },
    Indexed {
        base: Box<SpaceSpec>,
        hival: u16,
        /// `(hival + 1) × base components` bytes.
        lookup: Vec<u8>,
    },
    /// A calibrated gray space (ISO 32000-1 §8.6.5.2): the single-stage
    /// form a `CIEBasedA` space collapses to. `white` and `black` are
    /// tristimulus values; `gamma` is the exponent applied to the one
    /// component.
    CalGray {
        white: [f32; 3],
        black: [f32; 3],
        gamma: f32,
    },
    /// A calibrated RGB space (ISO 32000-1 §8.6.5.3): the single-stage
    /// form a `CIEBasedABC` space collapses to. `matrix` maps the decoded
    /// components to XYZ, three elements per input component, the same
    /// element order as the manual's `MatrixABC`.
    CalRGB {
        white: [f32; 3],
        black: [f32; 3],
        gamma: [f32; 3],
        matrix: [f32; 9],
    },
    /// The L*a*b* space (ISO 32000-1 §8.6.5.4) every other CIE-based
    /// space is carried as: L* runs 0 to 100, `range` bounds a* and b*
    /// as `[amin amax bmin bmax]`.
    Lab {
        white: [f32; 3],
        black: [f32; 3],
        range: [f32; 4],
    },
    /// The pattern space (PLRM3 §4.9): a colour is a pattern instance,
    /// given to the backend as a [`PatternInfo`] beside the components of
    /// `base`, the underlying space an uncoloured pattern is painted in;
    /// a coloured pattern has no base and no components.
    Pattern {
        base: Option<Box<SpaceSpec>>,
    },
}

impl SpaceSpec {
    /// Number of colour components a colour in this space has.
    pub fn components(&self) -> usize {
        match self {
            SpaceSpec::DeviceGray
            | SpaceSpec::Separation { .. }
            | SpaceSpec::Indexed { .. }
            | SpaceSpec::CalGray { .. } => 1,
            SpaceSpec::DeviceRGB | SpaceSpec::CalRGB { .. } | SpaceSpec::Lab { .. } => 3,
            SpaceSpec::DeviceCMYK => 4,
            SpaceSpec::DeviceN { names, .. } => names.len(),
            SpaceSpec::Pattern { base } => base.as_ref().map_or(0, |b| b.components()),
        }
    }

    /// The space the components are in: the base of a pattern space,
    /// otherwise the space itself.
    pub fn component_space(&self) -> Option<&SpaceSpec> {
        match self {
            SpaceSpec::Pattern { base } => base.as_deref(),
            other => Some(other),
        }
    }

    /// The family name, as the first element of the array form.
    pub fn family(&self) -> &'static str {
        match self {
            SpaceSpec::DeviceGray => "DeviceGray",
            SpaceSpec::DeviceRGB => "DeviceRGB",
            SpaceSpec::DeviceCMYK => "DeviceCMYK",
            SpaceSpec::Separation { .. } => "Separation",
            SpaceSpec::DeviceN { .. } => "DeviceN",
            SpaceSpec::Indexed { .. } => "Indexed",
            SpaceSpec::CalGray { .. } => "CalGray",
            SpaceSpec::CalRGB { .. } => "CalRGB",
            SpaceSpec::Lab { .. } => "Lab",
            SpaceSpec::Pattern { .. } => "Pattern",
        }
    }

    /// The values component `index` may take, as the space clamps them:
    /// the index range of an Indexed space, L* and the a*/b* ranges of a
    /// Lab space, the unit interval otherwise.
    pub fn component_limits(&self, index: usize) -> (f32, f32) {
        match self.component_space() {
            Some(SpaceSpec::Indexed { hival, .. }) => (0.0, f32::from(*hival)),
            Some(SpaceSpec::Lab { range, .. }) => match index {
                0 => (0.0, 100.0),
                1 => (range[0], range[1]),
                _ => (range[2], range[3]),
            },
            _ => (0.0, 1.0),
        }
    }

    /// The colour selected when the space is set: black, which for
    /// Separation and DeviceN means full tint, and for a Lab space the
    /// value nearest zero its ranges allow.
    pub fn initial_color(&self) -> Vec<f32> {
        match self {
            SpaceSpec::DeviceGray
            | SpaceSpec::DeviceRGB
            | SpaceSpec::Indexed { .. }
            | SpaceSpec::CalGray { .. }
            | SpaceSpec::CalRGB { .. } => vec![0.0; self.components()],
            SpaceSpec::Lab { .. } => (0..3)
                .map(|k| {
                    let (lo, hi) = self.component_limits(k);
                    0.0f32.clamp(lo, hi.max(lo))
                })
                .collect(),
            SpaceSpec::DeviceCMYK => vec![0.0, 0.0, 0.0, 1.0],
            SpaceSpec::Separation { .. } | SpaceSpec::DeviceN { .. } => {
                vec![1.0; self.components()]
            }
            SpaceSpec::Pattern { base } => base.as_ref().map_or(Vec::new(), |b| b.initial_color()),
        }
    }
}

/// Everything an image or image mask carries besides its sample data.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageSpec {
    pub width: u32,
    pub height: u32,
    pub bits_per_component: u8,
    /// The space the samples are in; `None` for a mask, whose samples
    /// select the current colour.
    pub color_space: Option<SpaceSpec>,
    /// Two entries per component; the operator supplies the default when
    /// the program gives none.
    pub decode: Vec<f32>,
    /// Maps unit square coordinates to the image's sample grid.
    pub matrix: Matrix,
    pub interpolate: bool,
    pub is_mask: bool,
    /// The data is not raw samples but their encoding in the named
    /// format, passed through as read; the dimensions and depth still
    /// describe the decoded samples.
    pub encoded: Option<Encoded>,
}

/// An encoding an image's data is carried in rather than decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoded {
    /// The `DCTDecode` filter's format: a baseline or progressive JPEG
    /// stream of ITU-T T.81, from its start-of-image marker through its
    /// end-of-image marker.
    Dct,
}

impl ImageSpec {
    pub fn components(&self) -> usize {
        self.color_space.as_ref().map_or(1, SpaceSpec::components)
    }

    /// Bytes per row: rows are padded to a byte boundary.
    pub fn row_bytes(&self) -> Option<usize> {
        let bits = (self.width as usize)
            .checked_mul(usize::from(self.bits_per_component))?
            .checked_mul(self.components())?;
        Some(bits.div_ceil(8))
    }

    /// Total sample bytes for the full height.
    pub fn data_len(&self) -> Option<usize> {
        self.row_bytes()?.checked_mul(self.height as usize)
    }
}

/// The current font as the graphics state holds it: the VM's instance id
/// for the font dictionary (see `Interp::font_dict`) and that dictionary's
/// `FontMatrix`, which maps glyph space to text space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FontRef {
    pub instance: u32,
    pub matrix: Matrix,
}

/// One glyph of a shown run: the code bytes that selected it (`len`
/// bytes, one to four, as the big-endian value `code`), the CID the
/// font's CMap gave them (the code itself for a simple font, whose codes
/// are one byte), and the displacement, in glyph space, applied to the
/// current point after it. The displacement is the glyph's width plus
/// whatever the show variant added, taken back through the font matrix;
/// in writing mode 1 it is the vertical advance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub code: u32,
    pub len: u8,
    pub cid: u16,
    pub dx: f32,
    pub dy: f32,
}

impl Glyph {
    /// A simple font's glyph: a one-byte code that is its own CID.
    pub const fn simple(code: u8, dx: f32, dy: f32) -> Self {
        Glyph {
            code: code as u32,
            len: 1,
            cid: code as u16,
            dx,
            dy,
        }
    }

    /// The code as its bytes, big-endian, `len` of them.
    pub fn code_bytes(&self) -> Vec<u8> {
        let len = usize::from(self.len.clamp(1, 4));
        self.code.to_be_bytes()[4 - len..].to_vec()
    }

    /// The sum of the displacements of a run, in glyph space.
    pub fn total(glyphs: &[Glyph]) -> Point {
        glyphs.iter().fold(Point::default(), |acc, g| {
            Point::new(acc.x + g.dx, acc.y + g.dy)
        })
    }
}

/// Where a font instance's glyphs come from, as a backend recording text
/// needs to know it.
#[derive(Clone, Debug)]
pub enum FontSource {
    /// One of the thirty-five resident faces; widths come from its
    /// metrics and the backend draws nothing itself.
    Resident(ResidentFace),
    /// A Type 3 font, whose glyphs are procedures the VM runs between
    /// `begin_glyph` and `end_glyph`. `family` is the `FID` every derived
    /// instance shares; `font_matrix` is the matrix the font was defined
    /// with, before any scaling, and glyph procedures paint in its space.
    Type3 {
        family: u32,
        font_matrix: Matrix,
        font_bbox: Bounds,
    },
    /// A Type 1 or Type 42 font the job defined, with the immutable
    /// snapshot of its program: the backend reads it and never alters it.
    /// `family` and `font_matrix` are as for Type 3; `font_name` is the
    /// dictionary's `FontName`. Glyph displacements are in the space the
    /// font matrix maps: charstring units for Type 1, the unit em for
    /// Type 42 (the program's font units divided by its units per em).
    Embedded {
        family: u32,
        kind: ProgramKind,
        program: Rc<Program>,
        font_matrix: Matrix,
        font_name: Vec<u8>,
    },
    /// A Type 0 font: its CMap decodes the run's bytes into codes and
    /// CIDs, and `descendant` is the CID-keyed font the CMap's font
    /// number 0 selects (an `Embedded` source whose program is addressed
    /// by CID, or, for a simple descendant, that font's own source).
    /// `family` is the Type 0 font's `FID`; `wmode` is the CMap's
    /// writing mode, in which every run's glyphs are positioned at their
    /// vertical origin; `unicode_based` marks a CMap whose codes are
    /// Unicode, so `Glyph::code` bytes are UTF-16BE. Displacements are in
    /// the descendant's glyph space, and the font matrix the run carries
    /// is the descendant's composed with the Type 0 font's.
    Composite {
        family: u32,
        cmap_name: Vec<u8>,
        wmode: u8,
        unicode_based: bool,
        cmap: Rc<CMap>,
        descendant: Box<FontSource>,
    },
}

impl PartialEq for FontSource {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (FontSource::Resident(a), FontSource::Resident(b)) => a == b,
            (
                FontSource::Type3 {
                    family,
                    font_matrix,
                    font_bbox,
                },
                FontSource::Type3 {
                    family: family2,
                    font_matrix: matrix2,
                    font_bbox: bbox2,
                },
            ) => family == family2 && font_matrix == matrix2 && font_bbox == bbox2,
            (
                FontSource::Embedded {
                    family,
                    kind,
                    program,
                    font_matrix,
                    font_name,
                },
                FontSource::Embedded {
                    family: family2,
                    kind: kind2,
                    program: program2,
                    font_matrix: matrix2,
                    font_name: name2,
                },
            ) => {
                family == family2
                    && kind == kind2
                    && Rc::ptr_eq(program, program2)
                    && font_matrix == matrix2
                    && font_name == name2
            }
            (
                FontSource::Composite {
                    family,
                    cmap_name,
                    wmode,
                    unicode_based,
                    cmap,
                    descendant,
                },
                FontSource::Composite {
                    family: family2,
                    cmap_name: name2,
                    wmode: wmode2,
                    unicode_based: unicode2,
                    cmap: cmap2,
                    descendant: descendant2,
                },
            ) => {
                family == family2
                    && cmap_name == name2
                    && wmode == wmode2
                    && unicode_based == unicode2
                    && Rc::ptr_eq(cmap, cmap2)
                    && descendant == descendant2
            }
            _ => false,
        }
    }
}

/// What the VM tells the backend about a font instance before the first
/// `show` or `begin_glyph` that names it.
#[derive(Clone, Debug, PartialEq)]
pub struct FontInfo {
    pub source: FontSource,
    /// The glyph name each code selects, `None` where the encoding's
    /// entry is not a name; 256 entries.
    pub encoding: Vec<Option<Vec<u8>>>,
}

/// One value of a `pdfmark` entry, as the operator converts the objects
/// between the mark and the kind: names and strings as bytes, numbers,
/// booleans, `null`, arrays, and name-keyed dictionaries in insertion
/// order. Anything else in a mark is `typecheck` in the operator, so a
/// backend never sees a procedure or an operator.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkValue {
    Name(Vec<u8>),
    String(Vec<u8>),
    Int(i32),
    Real(f32),
    Bool(bool),
    Null,
    Array(Vec<MarkValue>),
    Dict(Vec<(Vec<u8>, MarkValue)>),
}

impl MarkValue {
    pub fn as_name(&self) -> Option<&[u8]> {
        match self {
            MarkValue::Name(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&[u8]> {
        match self {
            MarkValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// The bytes of a name or a string, which several mark keys accept
    /// interchangeably.
    pub fn as_text(&self) -> Option<&[u8]> {
        match self {
            MarkValue::Name(b) | MarkValue::String(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_int(&self) -> Option<i32> {
        match self {
            MarkValue::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f32> {
        match self {
            MarkValue::Int(i) => Some(*i as f32),
            MarkValue::Real(r) => Some(*r),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            MarkValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[MarkValue]> {
        match self {
            MarkValue::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_dict(&self) -> Option<&[(Vec<u8>, MarkValue)]> {
        match self {
            MarkValue::Dict(d) => Some(d),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, MarkValue::Null)
    }
}

/// An opaque reference to a procedure the graphics state carries on the
/// VM's behalf: a screen's spot function or a transfer function. The VM
/// resolves it (`Interp::graphics_proc`); [`ProcRef::IDENTITY`] is the
/// empty procedure every state starts with.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ProcRef(pub u32);

impl ProcRef {
    pub const IDENTITY: ProcRef = ProcRef(0);
}

/// The colour a program set in a CIE-based space (PLRM3 §4.8.3), kept
/// beside the boundary colour the backend paints with: `space` is the
/// VM's handle for the space as the program gave it and `components` the
/// program's values clamped to the space's ranges, unused trailing
/// entries zero. The backend stores it with the graphics state and hands
/// it back unchanged, as it does a pattern instance; it never interprets
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CieColor {
    pub space: u32,
    pub components: [f32; 4],
}

/// A halftone screen as `setscreen` records it (PLRM3 §7.4): frequency in
/// lines per inch, angle in degrees, and the spot function. Recorded so
/// the getters answer; never applied.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub frequency: f32,
    pub angle: f32,
    pub spot: ProcRef,
}

impl Screen {
    pub const DEFAULT: Screen = Screen {
        frequency: 60.0,
        angle: 45.0,
        spot: ProcRef::IDENTITY,
    };
}

impl Default for Screen {
    fn default() -> Self {
        Screen::DEFAULT
    }
}

/// A path segment, in user space, as `clippath` reports the clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
    Close,
}

/// A tiling pattern instance as the backend sees it (PLRM3 §4.9.2): the
/// value part of what `makepattern` made. `id` distinguishes instances
/// for the life of the interpreter; `matrix` maps pattern space to
/// default user space (the instance's matrix already concatenated with
/// the CTM at `makepattern`); `bbox` and the steps are in pattern space;
/// `paint_type` is 1 (coloured) or 2 (uncoloured) and `tiling_type` 1 to
/// 3. The paint procedure stays with the VM, which runs it under
/// `begin_pattern_cell`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PatternInfo {
    pub id: u64,
    pub matrix: Matrix,
    pub bbox: Bounds,
    pub xstep: f32,
    pub ystep: f32,
    pub paint_type: u8,
    pub tiling_type: u8,
}

/// A form as the backend sees it (PLRM3 §4.7): `id` identifies the form
/// dictionary, `bbox` is in form space, and `matrix` maps form space to
/// default user space (the dictionary's `Matrix` concatenated with the
/// CTM at `execform`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FormInfo {
    pub id: u64,
    pub bbox: Bounds,
    pub matrix: Matrix,
}

/// What a graphics implementation provides to the VM. Every method maps to
/// one operator or query; angles are in degrees, coordinates in the
/// current user space, and the backend applies the CTM.
pub trait GraphicsBackend {
    // --- graphics-state stack ------------------------------------------------

    fn gsave(&mut self) -> Result<(), VmError>;
    fn grestore(&mut self) -> Result<(), VmError>;
    /// Number of states on the stack below the current one.
    fn gstate_depth(&self) -> usize;
    /// Pops states until the depth is `depth`, restoring each in turn;
    /// what `restore` does with the depth its `save` recorded.
    fn grestore_to(&mut self, depth: usize) -> Result<(), VmError>;
    /// Resets the current state's parameters to their defaults without
    /// touching the stack.
    fn initgraphics(&mut self) -> Result<(), VmError>;

    // --- line parameters and flatness ----------------------------------------

    fn set_line_width(&mut self, width: f32) -> Result<(), VmError>;
    fn line_width(&self) -> f32;
    fn set_line_cap(&mut self, cap: LineCap) -> Result<(), VmError>;
    fn line_cap(&self) -> LineCap;
    fn set_line_join(&mut self, join: LineJoin) -> Result<(), VmError>;
    fn line_join(&self) -> LineJoin;
    fn set_miter_limit(&mut self, limit: f32) -> Result<(), VmError>;
    fn miter_limit(&self) -> f32;
    fn set_dash(&mut self, array: &[f32], phase: f32) -> Result<(), VmError>;
    fn dash(&self) -> (Vec<f32>, f32);
    fn set_flatness(&mut self, flatness: f32) -> Result<(), VmError>;
    fn flatness(&self) -> f32;

    // --- coordinate system ----------------------------------------------------

    /// Replaces the CTM by `matrix` followed by the current CTM.
    fn concat(&mut self, matrix: Matrix) -> Result<(), VmError>;
    fn set_matrix(&mut self, matrix: Matrix) -> Result<(), VmError>;
    fn current_matrix(&self) -> Matrix;
    /// The CTM `initgraphics` installs.
    fn default_matrix(&self) -> Matrix;

    // --- colour ------------------------------------------------------------------

    fn set_color_space(&mut self, space: &SpaceSpec) -> Result<(), VmError>;
    /// `components` has the arity of the current space.
    fn set_color(&mut self, components: &[f32]) -> Result<(), VmError>;
    fn current_color_space(&self) -> SpaceSpec;
    fn current_color(&self) -> Vec<f32>;

    // --- path construction ------------------------------------------------------

    fn newpath(&mut self) -> Result<(), VmError>;
    fn moveto(&mut self, p: Point) -> Result<(), VmError>;
    fn lineto(&mut self, p: Point) -> Result<(), VmError>;
    fn curveto(&mut self, c1: Point, c2: Point, p: Point) -> Result<(), VmError>;
    fn closepath(&mut self) -> Result<(), VmError>;
    /// Counter-clockwise arc from `start` to `end` degrees.
    fn arc(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError>;
    /// Clockwise arc from `start` to `end` degrees.
    fn arcn(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError>;
    /// Tangent arc between the current point, `p1`, and `p2`; returns the
    /// two tangent points.
    fn arcto(&mut self, p1: Point, p2: Point, radius: f32) -> Result<(Point, Point), VmError>;
    /// `nocurrentpoint` when the path is empty.
    fn current_point(&self) -> Result<Point, VmError>;
    /// `nocurrentpoint` when the path is empty.
    fn path_bbox(&self) -> Result<Bounds, VmError>;

    // --- painting --------------------------------------------------------------

    fn fill(&mut self) -> Result<(), VmError>;
    fn eofill(&mut self) -> Result<(), VmError>;
    fn stroke(&mut self) -> Result<(), VmError>;
    /// Fills the rectangles without disturbing the current path.
    fn rectfill(&mut self, rects: &[Rect]) -> Result<(), VmError>;
    fn rectstroke(&mut self, rects: &[Rect]) -> Result<(), VmError>;
    /// Intersects the clip with the rectangles and empties the current
    /// path.
    fn rectclip(&mut self, rects: &[Rect]) -> Result<(), VmError>;

    // --- clipping ----------------------------------------------------------------

    fn clip(&mut self) -> Result<(), VmError>;
    fn eoclip(&mut self) -> Result<(), VmError>;
    fn initclip(&mut self) -> Result<(), VmError>;
    /// Replaces the current path with the clip path and returns its
    /// segments.
    fn clippath(&mut self) -> Result<Vec<Seg>, VmError>;
    /// The current path's segments in the current user space, for
    /// `pathforall`. A backend that keeps no path reports none.
    fn current_path(&self) -> Vec<Seg> {
        Vec::new()
    }

    // --- halftone and transfer ---------------------------------------------------

    /// Records the red, green, blue, and gray screens, in that order
    /// (`setscreen` gives one screen for all four); saved and restored
    /// with the state, never applied. A backend without a state slot for
    /// them answers the defaults.
    fn set_screens(&mut self, screens: [Screen; 4]) -> Result<(), VmError> {
        let _ = screens;
        Ok(())
    }
    fn screens(&self) -> [Screen; 4] {
        [Screen::DEFAULT; 4]
    }
    /// Records the red, green, blue, and gray transfer functions, in that
    /// order; as `set_screens`.
    fn set_transfers(&mut self, transfers: [ProcRef; 4]) -> Result<(), VmError> {
        let _ = transfers;
        Ok(())
    }
    fn transfers(&self) -> [ProcRef; 4] {
        [ProcRef::IDENTITY; 4]
    }

    // --- images ------------------------------------------------------------------

    /// `data` holds `spec.height` complete rows of `spec.row_bytes()`.
    fn image(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError>;
    fn imagemask(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError>;

    // --- text ---------------------------------------------------------------------

    /// Describes a font instance before the first `show` or `begin_glyph`
    /// that names it; the VM sends each instance once per backend. A
    /// backend that records nothing about text may ignore it.
    fn define_font(&mut self, instance: u32, info: &FontInfo) -> Result<(), VmError> {
        let _ = (instance, info);
        Ok(())
    }
    /// Makes `font` the current font of the graphics state (`None` clears
    /// it); saved and restored with the rest of the state.
    fn set_font(&mut self, font: Option<FontRef>) -> Result<(), VmError>;
    fn font(&self) -> Option<FontRef>;
    /// Shows a run of glyphs in the current font, starting at the current
    /// point. The backend records the run and advances the current point
    /// by the sum of the glyph displacements taken through the font matrix
    /// (a user-space delta); the path is otherwise left alone. Without a
    /// current point it is `nocurrentpoint`, without a font `invalidfont`.
    fn show(&mut self, glyphs: &[Glyph]) -> Result<(), VmError>;
    /// Starts capturing a Type 3 glyph: until `end_glyph`, marks go into
    /// the glyph's own procedure in the coordinates of the CTM in effect
    /// now (glyph space), and page operations are refused. With `measure`
    /// nothing is kept; the glyph is only being measured.
    fn begin_glyph(
        &mut self,
        font: FontRef,
        code: u8,
        name: &[u8],
        measure: bool,
    ) -> Result<(), VmError>;
    /// Ends the capture begun by `begin_glyph`. `width` is the glyph's
    /// displacement in glyph space; `bbox` is present for a glyph declared
    /// with `setcachedevice` (colour-independent) and absent for one
    /// declared with `setcharwidth`.
    fn end_glyph(&mut self, width: (f32, f32), bbox: Option<Bounds>) -> Result<(), VmError>;

    // --- patterns and forms --------------------------------------------------------

    /// Makes `pattern` the current colour in a pattern space, with the
    /// components of the underlying space for an uncoloured pattern
    /// (`pattern.paint_type` 2) and none for a coloured one. Saved and
    /// restored with the rest of the state. A backend without patterns
    /// accepts and ignores it.
    fn set_pattern(&mut self, pattern: &PatternInfo, components: &[f32]) -> Result<(), VmError> {
        let _ = (pattern, components);
        Ok(())
    }
    /// The instance `set_pattern` made the current colour, while the
    /// current space is still a pattern space and no other colour has
    /// replaced it; what `currentcolor` reports and what a painting
    /// operator asks to capture. A backend without patterns has none.
    fn current_pattern(&self) -> Option<PatternInfo> {
        None
    }

    // --- values kept for the VM ------------------------------------------------------

    /// Attaches `color` to the current colour: what `currentcolor` and
    /// `currentcolorspace` report while a CIE-based space is current.
    /// Cleared by `set_color_space`, kept by `set_color` and
    /// `set_pattern`, saved and restored with the state. A backend that
    /// keeps nothing accepts and ignores it.
    fn set_cie_color(&mut self, color: CieColor) -> Result<(), VmError> {
        let _ = color;
        Ok(())
    }
    fn current_cie_color(&self) -> Option<CieColor> {
        None
    }
    /// The colour rendering dictionary `setcolorrendering` installed,
    /// held by reference like a transfer function; `None` is the
    /// interpreter's default instance. Recorded, never applied.
    fn set_color_rendering(&mut self, dict: Option<ProcRef>) -> Result<(), VmError> {
        let _ = dict;
        Ok(())
    }
    fn color_rendering(&self) -> Option<ProcRef> {
        None
    }
    /// Starts capturing `pattern`'s cell: until `end_pattern_cell`, marks
    /// go into the pattern's own resource in pattern space, clipped to
    /// its box, and page operations are refused. Returns `false` and
    /// captures nothing when the page already holds the cell, in which
    /// case the VM does not run the paint procedure and does not call
    /// `end_pattern_cell`.
    fn begin_pattern_cell(&mut self, pattern: &PatternInfo) -> Result<bool, VmError> {
        let _ = pattern;
        Ok(false)
    }
    /// Ends the capture begun by `begin_pattern_cell`.
    fn end_pattern_cell(&mut self) -> Result<(), VmError> {
        Ok(())
    }
    /// Starts capturing `form`'s body: until `end_form`, marks go into
    /// the form's own resource in form space, clipped to its box, and
    /// page operations are refused. Returns `false` and captures nothing
    /// when the page already holds the body, in which case the VM does
    /// not run the paint procedure and does not call `end_form`.
    fn begin_form(&mut self, form: &FormInfo) -> Result<bool, VmError> {
        let _ = form;
        Ok(false)
    }
    /// Ends the capture begun by `begin_form`.
    fn end_form(&mut self) -> Result<(), VmError> {
        Ok(())
    }
    /// Places `form`'s body on the page (or in the enclosing capture)
    /// under `form.matrix`; called for every `execform`, after the
    /// capture when there was one.
    fn place_form(&mut self, form: &FormInfo) -> Result<(), VmError> {
        let _ = form;
        Ok(())
    }

    // --- page and device ---------------------------------------------------------

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError>;
    fn showpage(&mut self) -> Result<(), VmError>;
    fn copypage(&mut self) -> Result<(), VmError>;
    fn erasepage(&mut self) -> Result<(), VmError>;
    /// Installs a device that discards marks until the state that
    /// installed it is restored.
    fn nulldevice(&mut self) -> Result<(), VmError>;

    // --- document marks -----------------------------------------------------------

    /// A `pdfmark` of `kind` with the objects between the mark and the
    /// kind converted to values, in order; pairing keys with values is
    /// the backend's, since the kinds differ in shape. A backend without
    /// documents ignores marks.
    fn pdfmark(&mut self, kind: &[u8], entries: &[MarkValue]) -> Result<(), VmError> {
        let _ = (kind, entries);
        Ok(())
    }

    /// A `setdistillerparams` request, every entry as a value, in the
    /// request's order; the writer merges what it honours. A backend
    /// without documents ignores it.
    fn set_distiller_params(&mut self, entries: &[(Vec<u8>, MarkValue)]) -> Result<(), VmError> {
        let _ = entries;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-4 && (a.y - b.y).abs() < 1e-4
    }

    #[test]
    fn then_applies_left_first() {
        let m = Matrix::scaling(2.0, 3.0).then(Matrix::translation(10.0, 20.0));
        assert!(close(m.apply(Point::new(1.0, 1.0)), Point::new(12.0, 23.0)));
        let m = Matrix::translation(10.0, 20.0).then(Matrix::scaling(2.0, 3.0));
        assert!(close(m.apply(Point::new(1.0, 1.0)), Point::new(22.0, 63.0)));
        assert_eq!(Matrix::IDENTITY.then(Matrix::IDENTITY), Matrix::IDENTITY);
    }

    #[test]
    fn rotation_is_counter_clockwise() {
        let p = Matrix::rotation(90.0).apply(Point::new(1.0, 0.0));
        assert!(close(p, Point::new(0.0, 1.0)));
        assert!(close(
            Matrix::rotation(90.0).apply_delta(Point::new(0.0, 1.0)),
            Point::new(-1.0, 0.0)
        ));
    }

    #[test]
    fn inverse_round_trips_and_rejects_singular() {
        let m = Matrix([2.0, 1.0, 0.5, 3.0, 7.0, -4.0]);
        let inv = m.inverse().unwrap();
        let p = Point::new(3.5, -2.0);
        assert!(close(inv.apply(m.apply(p)), p));
        assert!(close(inv.apply_delta(m.apply_delta(p)), p));
        assert_eq!(Matrix::scaling(0.0, 1.0).inverse(), None);
        assert_eq!(Matrix([1.0, 2.0, 2.0, 4.0, 0.0, 0.0]).inverse(), None);
    }

    #[test]
    fn glyph_runs_sum_their_displacements() {
        let run = [
            Glyph::simple(72, 722.0, 0.0),
            Glyph::simple(105, 222.0, 5.0),
        ];
        assert_eq!(Glyph::total(&run), Point::new(944.0, 5.0));
        assert_eq!(Glyph::total(&[]), Point::default());
        assert_eq!(run[0].cid, 72);
        assert_eq!(run[0].len, 1);
        assert_eq!(run[0].code_bytes(), vec![72]);
        let wide = Glyph {
            code: 0x8140,
            len: 2,
            cid: 200,
            dx: 0.0,
            dy: 0.0,
        };
        assert_eq!(wide.code_bytes(), vec![0x81, 0x40]);
    }

    #[test]
    fn mark_values_answer_by_kind() {
        let name = MarkValue::Name(b"Title".to_vec());
        assert_eq!(name.as_name(), Some(&b"Title"[..]));
        assert_eq!(name.as_text(), Some(&b"Title"[..]));
        assert_eq!(name.as_string(), None);
        let s = MarkValue::String(b"x".to_vec());
        assert_eq!(s.as_string(), Some(&b"x"[..]));
        assert_eq!(s.as_text(), Some(&b"x"[..]));
        assert_eq!(MarkValue::Int(3).as_number(), Some(3.0));
        assert_eq!(MarkValue::Int(3).as_int(), Some(3));
        assert_eq!(MarkValue::Real(1.5).as_int(), None);
        assert_eq!(MarkValue::Real(1.5).as_number(), Some(1.5));
        assert_eq!(MarkValue::Bool(true).as_bool(), Some(true));
        assert!(MarkValue::Null.is_null());
        let array = MarkValue::Array(vec![MarkValue::Null]);
        assert_eq!(array.as_array().map(<[MarkValue]>::len), Some(1));
        let dict = MarkValue::Dict(vec![(b"k".to_vec(), MarkValue::Int(1))]);
        assert_eq!(dict.as_dict().map(<[_]>::len), Some(1));
        assert_eq!(dict.as_array(), None);
    }

    #[test]
    fn line_codes_round_trip() {
        for code in 0..3 {
            assert_eq!(LineCap::from_code(code).unwrap().code(), code);
            assert_eq!(LineJoin::from_code(code).unwrap().code(), code);
        }
        assert_eq!(LineCap::from_code(3), None);
        assert_eq!(LineJoin::from_code(-1), None);
    }

    #[test]
    fn space_arity_and_initial_colour() {
        assert_eq!(SpaceSpec::DeviceGray.components(), 1);
        assert_eq!(SpaceSpec::DeviceRGB.initial_color(), vec![0.0; 3]);
        assert_eq!(
            SpaceSpec::DeviceCMYK.initial_color(),
            vec![0.0, 0.0, 0.0, 1.0]
        );
        let sep = SpaceSpec::Separation {
            name: b"Spot".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceCMYK),
            tint_source: b"{}".to_vec(),
        };
        assert_eq!(sep.components(), 1);
        assert_eq!(sep.initial_color(), vec![1.0]);
        let n = SpaceSpec::DeviceN {
            names: vec![b"A".to_vec(), b"B".to_vec()],
            alternate: Box::new(SpaceSpec::DeviceGray),
            tint_source: Vec::new(),
        };
        assert_eq!(n.components(), 2);
        assert_eq!(n.family(), "DeviceN");
        let indexed = SpaceSpec::Indexed {
            base: Box::new(SpaceSpec::DeviceRGB),
            hival: 1,
            lookup: vec![0; 6],
        };
        assert_eq!(indexed.components(), 1);
        assert_eq!(indexed.initial_color(), vec![0.0]);
        let coloured = SpaceSpec::Pattern { base: None };
        assert_eq!(coloured.components(), 0);
        assert_eq!(coloured.family(), "Pattern");
        assert_eq!(coloured.initial_color(), Vec::<f32>::new());
        assert_eq!(coloured.component_space(), None);
        let uncoloured = SpaceSpec::Pattern {
            base: Some(Box::new(SpaceSpec::DeviceCMYK)),
        };
        assert_eq!(uncoloured.components(), 4);
        assert_eq!(uncoloured.family(), "Pattern");
        assert_eq!(uncoloured.initial_color(), vec![0.0, 0.0, 0.0, 1.0]);
        assert_eq!(uncoloured.component_space(), Some(&SpaceSpec::DeviceCMYK));
        assert_eq!(indexed.component_space(), Some(&indexed));
        assert_eq!(indexed.component_limits(0), (0.0, 1.0));
        assert_eq!(uncoloured.component_limits(3), (0.0, 1.0));
    }

    #[test]
    fn calibrated_spaces_have_pdf_arity_and_ranges() {
        let white = [0.95, 1.0, 1.07];
        let gray = SpaceSpec::CalGray {
            white,
            black: [0.0; 3],
            gamma: 1.8,
        };
        assert_eq!(gray.components(), 1);
        assert_eq!(gray.family(), "CalGray");
        assert_eq!(gray.initial_color(), vec![0.0]);
        assert_eq!(gray.component_limits(0), (0.0, 1.0));
        let rgb = SpaceSpec::CalRGB {
            white,
            black: [0.0; 3],
            gamma: [1.0; 3],
            matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        };
        assert_eq!(rgb.components(), 3);
        assert_eq!(rgb.family(), "CalRGB");
        assert_eq!(rgb.initial_color(), vec![0.0; 3]);
        let lab = SpaceSpec::Lab {
            white,
            black: [0.0; 3],
            range: [-100.0, 100.0, -100.0, 100.0],
        };
        assert_eq!(lab.components(), 3);
        assert_eq!(lab.family(), "Lab");
        assert_eq!(lab.initial_color(), vec![0.0; 3]);
        assert_eq!(lab.component_limits(0), (0.0, 100.0));
        assert_eq!(lab.component_limits(2), (-100.0, 100.0));
        let offset = SpaceSpec::Lab {
            white,
            black: [0.0; 3],
            range: [10.0, 20.0, -50.0, -40.0],
        };
        assert_eq!(offset.initial_color(), vec![0.0, 10.0, -40.0]);
        assert_eq!(
            SpaceSpec::Pattern {
                base: Some(Box::new(lab.clone()))
            }
            .component_limits(1),
            (-100.0, 100.0)
        );
        assert_eq!(lab.component_space(), Some(&lab));
    }

    /// The pattern and form hooks have defaults, so a backend without
    /// either keeps compiling and answers "nothing captured".
    #[test]
    fn pattern_and_form_hooks_default_to_nothing() {
        struct Bare;
        impl GraphicsBackend for Bare {
            fn gsave(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn grestore(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn gstate_depth(&self) -> usize {
                0
            }
            fn grestore_to(&mut self, _: usize) -> Result<(), VmError> {
                Ok(())
            }
            fn initgraphics(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn set_line_width(&mut self, _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn line_width(&self) -> f32 {
                1.0
            }
            fn set_line_cap(&mut self, _: LineCap) -> Result<(), VmError> {
                Ok(())
            }
            fn line_cap(&self) -> LineCap {
                LineCap::Butt
            }
            fn set_line_join(&mut self, _: LineJoin) -> Result<(), VmError> {
                Ok(())
            }
            fn line_join(&self) -> LineJoin {
                LineJoin::Miter
            }
            fn set_miter_limit(&mut self, _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn miter_limit(&self) -> f32 {
                10.0
            }
            fn set_dash(&mut self, _: &[f32], _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn dash(&self) -> (Vec<f32>, f32) {
                (Vec::new(), 0.0)
            }
            fn set_flatness(&mut self, _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn flatness(&self) -> f32 {
                1.0
            }
            fn concat(&mut self, _: Matrix) -> Result<(), VmError> {
                Ok(())
            }
            fn set_matrix(&mut self, _: Matrix) -> Result<(), VmError> {
                Ok(())
            }
            fn current_matrix(&self) -> Matrix {
                Matrix::IDENTITY
            }
            fn default_matrix(&self) -> Matrix {
                Matrix::IDENTITY
            }
            fn set_color_space(&mut self, _: &SpaceSpec) -> Result<(), VmError> {
                Ok(())
            }
            fn set_color(&mut self, _: &[f32]) -> Result<(), VmError> {
                Ok(())
            }
            fn current_color_space(&self) -> SpaceSpec {
                SpaceSpec::DeviceGray
            }
            fn current_color(&self) -> Vec<f32> {
                vec![0.0]
            }
            fn newpath(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn moveto(&mut self, _: Point) -> Result<(), VmError> {
                Ok(())
            }
            fn lineto(&mut self, _: Point) -> Result<(), VmError> {
                Ok(())
            }
            fn curveto(&mut self, _: Point, _: Point, _: Point) -> Result<(), VmError> {
                Ok(())
            }
            fn closepath(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn arc(&mut self, _: Point, _: f32, _: f32, _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn arcn(&mut self, _: Point, _: f32, _: f32, _: f32) -> Result<(), VmError> {
                Ok(())
            }
            fn arcto(&mut self, p1: Point, p2: Point, _: f32) -> Result<(Point, Point), VmError> {
                Ok((p1, p2))
            }
            fn current_point(&self) -> Result<Point, VmError> {
                Err(VmError::NoCurrentPoint)
            }
            fn path_bbox(&self) -> Result<Bounds, VmError> {
                Err(VmError::NoCurrentPoint)
            }
            fn fill(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn eofill(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn stroke(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn rectfill(&mut self, _: &[Rect]) -> Result<(), VmError> {
                Ok(())
            }
            fn rectstroke(&mut self, _: &[Rect]) -> Result<(), VmError> {
                Ok(())
            }
            fn rectclip(&mut self, _: &[Rect]) -> Result<(), VmError> {
                Ok(())
            }
            fn clip(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn eoclip(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn initclip(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn clippath(&mut self) -> Result<Vec<Seg>, VmError> {
                Ok(Vec::new())
            }
            fn image(&mut self, _: &ImageSpec, _: &[u8]) -> Result<(), VmError> {
                Ok(())
            }
            fn imagemask(&mut self, _: &ImageSpec, _: &[u8]) -> Result<(), VmError> {
                Ok(())
            }
            fn set_font(&mut self, _: Option<FontRef>) -> Result<(), VmError> {
                Ok(())
            }
            fn font(&self) -> Option<FontRef> {
                None
            }
            fn show(&mut self, _: &[Glyph]) -> Result<(), VmError> {
                Ok(())
            }
            fn begin_glyph(&mut self, _: FontRef, _: u8, _: &[u8], _: bool) -> Result<(), VmError> {
                Ok(())
            }
            fn end_glyph(&mut self, _: (f32, f32), _: Option<Bounds>) -> Result<(), VmError> {
                Ok(())
            }
            fn set_media_box(&mut self, _: Bounds) -> Result<(), VmError> {
                Ok(())
            }
            fn showpage(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn copypage(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn erasepage(&mut self) -> Result<(), VmError> {
                Ok(())
            }
            fn nulldevice(&mut self) -> Result<(), VmError> {
                Ok(())
            }
        }
        let mut bare = Bare;
        let pattern = PatternInfo {
            id: 1,
            matrix: Matrix::IDENTITY,
            bbox: Bounds::new(0.0, 0.0, 1.0, 1.0),
            xstep: 1.0,
            ystep: 1.0,
            paint_type: 1,
            tiling_type: 1,
        };
        assert_eq!(bare.set_pattern(&pattern, &[]), Ok(()));
        assert_eq!(bare.current_pattern(), None);
        assert_eq!(bare.begin_pattern_cell(&pattern), Ok(false));
        assert_eq!(bare.end_pattern_cell(), Ok(()));
        let form = FormInfo {
            id: 1,
            bbox: Bounds::new(0.0, 0.0, 1.0, 1.0),
            matrix: Matrix::IDENTITY,
        };
        assert_eq!(bare.begin_form(&form), Ok(false));
        assert_eq!(bare.end_form(), Ok(()));
        assert_eq!(bare.place_form(&form), Ok(()));
    }

    #[test]
    fn image_rows_pad_to_bytes() {
        let spec = ImageSpec {
            width: 10,
            height: 3,
            bits_per_component: 1,
            color_space: None,
            decode: vec![0.0, 1.0],
            matrix: Matrix::IDENTITY,
            interpolate: false,
            is_mask: true,
            encoded: None,
        };
        assert_eq!(spec.row_bytes(), Some(2));
        assert_eq!(spec.data_len(), Some(6));
        let rgb = ImageSpec {
            width: 3,
            height: 2,
            bits_per_component: 8,
            color_space: Some(SpaceSpec::DeviceRGB),
            decode: vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
            matrix: Matrix::IDENTITY,
            interpolate: false,
            is_mask: false,
            encoded: None,
        };
        assert_eq!(rgb.row_bytes(), Some(9));
        assert_eq!(rgb.data_len(), Some(18));
    }
}

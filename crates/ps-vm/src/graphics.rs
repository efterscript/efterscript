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

use ps_fonts::StdFont;

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
}

impl SpaceSpec {
    /// Number of colour components a colour in this space has.
    pub fn components(&self) -> usize {
        match self {
            SpaceSpec::DeviceGray | SpaceSpec::Separation { .. } | SpaceSpec::Indexed { .. } => 1,
            SpaceSpec::DeviceRGB => 3,
            SpaceSpec::DeviceCMYK => 4,
            SpaceSpec::DeviceN { names, .. } => names.len(),
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
        }
    }

    /// The colour selected when the space is set: black, which for
    /// Separation and DeviceN means full tint.
    pub fn initial_color(&self) -> Vec<f32> {
        match self {
            SpaceSpec::DeviceGray | SpaceSpec::DeviceRGB | SpaceSpec::Indexed { .. } => {
                vec![0.0; self.components()]
            }
            SpaceSpec::DeviceCMYK => vec![0.0, 0.0, 0.0, 1.0],
            SpaceSpec::Separation { .. } | SpaceSpec::DeviceN { .. } => {
                vec![1.0; self.components()]
            }
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

/// One glyph of a shown run: its character code and the displacement, in
/// glyph space, applied to the current point after it. The displacement is
/// the glyph's width plus whatever the show variant added, taken back
/// through the font matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glyph {
    pub code: u8,
    pub dx: f32,
    pub dy: f32,
}

impl Glyph {
    /// The sum of the displacements of a run, in glyph space.
    pub fn total(glyphs: &[Glyph]) -> Point {
        glyphs.iter().fold(Point::default(), |acc, g| {
            Point::new(acc.x + g.dx, acc.y + g.dy)
        })
    }
}

/// Where a font instance's glyphs come from, as a backend recording text
/// needs to know it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontSource {
    /// One of the standard fourteen; widths come from its metrics.
    Resident(StdFont),
    /// A Type 3 font, whose glyphs are procedures the VM runs between
    /// `begin_glyph` and `end_glyph`. `family` is the `FID` every derived
    /// instance shares; `font_matrix` is the matrix the font was defined
    /// with, before any scaling, and glyph procedures paint in its space.
    Type3 {
        family: u32,
        font_matrix: Matrix,
        font_bbox: Bounds,
    },
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

/// A path segment, in user space, as `clippath` reports the clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Seg {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
    Close,
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

    // --- page and device ---------------------------------------------------------

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError>;
    fn showpage(&mut self) -> Result<(), VmError>;
    fn copypage(&mut self) -> Result<(), VmError>;
    fn erasepage(&mut self) -> Result<(), VmError>;
    /// Installs a device that discards marks until the state that
    /// installed it is restored.
    fn nulldevice(&mut self) -> Result<(), VmError>;
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
            Glyph {
                code: 72,
                dx: 722.0,
                dy: 0.0,
            },
            Glyph {
                code: 105,
                dx: 222.0,
                dy: 5.0,
            },
        ];
        assert_eq!(Glyph::total(&run), Point::new(944.0, 5.0));
        assert_eq!(Glyph::total(&[]), Point::default());
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
        };
        assert_eq!(rgb.row_bytes(), Some(9));
        assert_eq!(rgb.data_len(), Some(18));
    }
}

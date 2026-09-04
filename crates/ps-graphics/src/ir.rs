// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The page IR: a PDF-shaped list of operations plus the resources they
//! reference, delivered to a [`PageSink`] one page at a time.
//!
//! Coordinates are in default user space — the backend applies the CTM as
//! paths are built — so the IR has no `cm`. The only place the CTM still
//! matters at paint time is a stroke, whose line width and dash lengths
//! are user-space quantities: a stroke records the CTM in effect so a
//! serializer can scale them or wrap the stroke in its own transform.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::rc::Rc;

use ps_fonts::{Program, ProgramKind, ResidentFace};
use ps_vm::{Bounds, Glyph, ImageSpec, LineCap, LineJoin, Matrix, Seg, SpaceSpec, Span};

pub use crate::state::FillRule;

/// Index into [`Resources::color_spaces`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpaceRef(pub usize);

/// Index into [`Resources::images`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageRef(pub usize);

/// Index into [`Resources::fonts`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontIndex(pub usize);

/// A glyph name, as the bytes of the PostScript name.
pub type GlyphName = Vec<u8>;

/// An encoding vector: the glyph name each code selects, `None` for a
/// code that selects no glyph.
pub type GlyphNames = Box<[Option<GlyphName>; 256]>;

/// Builds an encoding vector from the VM's names; `.notdef` is the absence
/// of a glyph.
pub fn glyph_names(names: &[Option<Vec<u8>>]) -> GlyphNames {
    let mut out: Vec<Option<GlyphName>> = names
        .iter()
        .take(256)
        .map(|name| name.clone().filter(|n| n != b".notdef"))
        .collect();
    out.resize(256, None);
    out.into_boxed_slice()
        .try_into()
        .expect("exactly 256 entries")
}

/// A captured Type 3 glyph: its procedure in glyph space, its
/// displacement, and the bounding box a `setcachedevice` glyph declared
/// (absent for a `setcharwidth` glyph, which may set its own colour).
#[derive(Clone, Debug, PartialEq)]
pub struct GlyphProc {
    pub ops: Vec<Op>,
    pub width: (f32, f32),
    pub bbox: Option<Bounds>,
}

/// The program snapshot of an embedded font, shared with the VM; two
/// references are equal when they are the same snapshot, which the VM
/// builds once per font family.
#[derive(Clone, Debug)]
pub struct ProgramRef(pub Rc<Program>);

impl PartialEq for ProgramRef {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Deref for ProgramRef {
    type Target = Program;

    fn deref(&self) -> &Program {
        &self.0
    }
}

/// A font a text operation draws with. Resident fonts carry no widths:
/// they come from the metrics in `ps-fonts` by glyph name; an embedded
/// font's come from its program.
#[derive(Clone, Debug, PartialEq)]
pub enum FontSpec {
    /// One of the thirty-five resident faces with the encoding in effect;
    /// glyph space is thousandths of the em.
    Resident {
        base: ResidentFace,
        encoding: GlyphNames,
    },
    /// A Type 3 font: `font_matrix` maps glyph space to text space, and
    /// every glyph shown on the page has its captured procedure here.
    Type3 {
        font_matrix: Matrix,
        font_bbox: Bounds,
        encoding: GlyphNames,
        glyphs: BTreeMap<GlyphName, GlyphProc>,
    },
    /// A Type 1 or Type 42 font the job defined, with its program.
    /// `font_matrix` maps glyph space to text space: charstring units for
    /// Type 1, the unit em for TrueType (the program's font units divided
    /// by its units per em), the space every displacement is in.
    Embedded {
        family: u32,
        kind: ProgramKind,
        font_name: Vec<u8>,
        font_matrix: Matrix,
        program: ProgramRef,
        encoding: GlyphNames,
    },
    /// A Type 0 font over a CID-keyed descendant: the run's glyphs carry
    /// the codes the CMap decoded and the CIDs it gave them, and the
    /// descendant — an `Embedded` snapshot addressed by CID, with an
    /// empty encoding — answers each CID with its glyph. `wmode` is the
    /// CMap's writing mode; in mode 1 the run's matrix places the first
    /// glyph's vertical origin and each glyph advances by the default
    /// vertical advance. `cid_to_code` records, for every CID shown on
    /// the page, the code (and its byte length) it came from, which a
    /// Unicode-based CMap makes a ToUnicode source.
    Composite {
        cmap_name: Vec<u8>,
        wmode: u8,
        unicode_based: bool,
        descendant: Box<FontSpec>,
        cid_to_code: BTreeMap<u16, (u32, u8)>,
    },
}

impl FontSpec {
    /// The encoding; a composite font answers with its descendant's,
    /// which is empty.
    pub fn encoding(&self) -> &GlyphNames {
        match self {
            FontSpec::Resident { encoding, .. }
            | FontSpec::Type3 { encoding, .. }
            | FontSpec::Embedded { encoding, .. } => encoding,
            FontSpec::Composite { descendant, .. } => descendant.encoding(),
        }
    }

    /// The matrix mapping glyph space to text space; for a composite
    /// font the descendant's, whose glyph space the run's displacements
    /// are in.
    pub fn font_matrix(&self) -> Matrix {
        match self {
            FontSpec::Resident { .. } => Matrix::scaling(0.001, 0.001),
            FontSpec::Type3 { font_matrix, .. } | FontSpec::Embedded { font_matrix, .. } => {
                *font_matrix
            }
            FontSpec::Composite { descendant, .. } => descendant.font_matrix(),
        }
    }

    /// Whether `self` and `other` are the same font apart from the CIDs
    /// recorded so far: a composite resource grows its `cid_to_code` as
    /// the page shows more, and equality of the rest is what interning
    /// and document-wide sharing go by. Other kinds compare as a whole.
    pub fn same_font(&self, other: &FontSpec) -> bool {
        match (self, other) {
            (
                FontSpec::Composite {
                    cmap_name,
                    wmode,
                    unicode_based,
                    descendant,
                    ..
                },
                FontSpec::Composite {
                    cmap_name: name2,
                    wmode: wmode2,
                    unicode_based: unicode2,
                    descendant: descendant2,
                    ..
                },
            ) => {
                cmap_name == name2
                    && wmode == wmode2
                    && unicode_based == unicode2
                    && descendant == descendant2
            }
            _ => self == other,
        }
    }

    /// The glyph a composite font's descendant draws for `cid`; `None`
    /// for a CID without a glyph and for other kinds of font.
    pub fn cid_glyph(&self, cid: u16) -> Option<Rc<ps_fonts::Glyph>> {
        let FontSpec::Composite { descendant, .. } = self else {
            return None;
        };
        let FontSpec::Embedded { program, .. } = &**descendant else {
            return None;
        };
        program.glyph_by_cid(cid).ok().flatten()
    }

    /// The displacement of `cid` in a composite font's glyph space, zero
    /// for a CID without a glyph.
    pub fn cid_width(&self, cid: u16) -> (f32, f32) {
        let FontSpec::Composite { descendant, .. } = self else {
            return (0.0, 0.0);
        };
        let scale = descendant.glyph_scale();
        self.cid_glyph(cid)
            .map_or((0.0, 0.0), |g| (g.advance.0 * scale, g.advance.1 * scale))
    }

    /// The displacement of a shown glyph as the font itself has it: by
    /// CID for a composite font, by the glyph's code otherwise.
    pub fn glyph_width(&self, glyph: &Glyph) -> (f32, f32) {
        match self {
            FontSpec::Composite { .. } => self.cid_width(glyph.cid),
            _ => self.width(u8::try_from(glyph.cid).unwrap_or(0)),
        }
    }

    /// The factor taking an embedded program's glyph units to the space
    /// the font matrix maps: one for charstring programs, the reciprocal
    /// of the units per em for TrueType.
    fn glyph_scale(&self) -> f32 {
        match self {
            FontSpec::Embedded { program, .. } => program
                .units_per_em()
                .map_or(1.0, |units| 1.0 / f32::from(units)),
            _ => 1.0,
        }
    }

    /// The glyph name `code` selects.
    pub fn glyph_name(&self, code: u8) -> Option<&GlyphName> {
        self.encoding()[usize::from(code)].as_ref()
    }

    /// The glyph an embedded font draws for `code`: the encoding's name
    /// when the program has it, else the program's `.notdef`; `None` for
    /// neither and for other kinds of font.
    pub fn program_glyph(&self, code: u8) -> Option<Rc<ps_fonts::Glyph>> {
        let FontSpec::Embedded { program, .. } = self else {
            return None;
        };
        let lookup = |name: &[u8]| program.glyph(name).ok().flatten();
        self.glyph_name(code)
            .and_then(|name| lookup(name))
            .or_else(|| lookup(b".notdef"))
    }

    /// The displacement of `code` in glyph space as the font itself has
    /// it, zero for a code without a glyph.
    pub fn width(&self, code: u8) -> (f32, f32) {
        if let FontSpec::Embedded { .. } = self {
            let scale = self.glyph_scale();
            return self
                .program_glyph(code)
                .map_or((0.0, 0.0), |g| (g.advance.0 * scale, g.advance.1 * scale));
        }
        if let FontSpec::Composite { .. } = self {
            return self.cid_width(u16::from(code));
        }
        let Some(name) = self.glyph_name(code) else {
            return (0.0, 0.0);
        };
        match self {
            FontSpec::Resident { base, .. } => std::str::from_utf8(name)
                .ok()
                .and_then(|name| base.width(name))
                .map_or((0.0, 0.0), |w| (f32::from(w), 0.0)),
            FontSpec::Type3 { glyphs, .. } => glyphs.get(name).map_or((0.0, 0.0), |g| g.width),
            FontSpec::Embedded { .. } | FontSpec::Composite { .. } => {
                unreachable!("handled above")
            }
        }
    }
}

/// An image with its raw sample data, exactly as acquired.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub spec: ImageSpec,
    /// The sample space interned in the page's resources; `None` for a
    /// mask.
    pub color_space: Option<SpaceRef>,
    pub data: Vec<u8>,
}

/// Everything the page's operations refer to by index. Colour spaces are
/// interned by structural equality, so a space set twice is one resource.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Resources {
    pub color_spaces: Vec<SpaceSpec>,
    pub images: Vec<Image>,
    pub fonts: Vec<FontSpec>,
}

impl Resources {
    /// The index of a font structurally equal to `spec`, added if there
    /// is none; how resident fonts are interned.
    pub fn intern_font(&mut self, spec: FontSpec) -> FontIndex {
        if let Some(i) = self.fonts.iter().position(|f| *f == spec) {
            return FontIndex(i);
        }
        self.add_font(spec)
    }

    pub fn add_font(&mut self, spec: FontSpec) -> FontIndex {
        self.fonts.push(spec);
        FontIndex(self.fonts.len() - 1)
    }

    pub fn intern_space(&mut self, space: &SpaceSpec) -> SpaceRef {
        if let Some(i) = self.color_spaces.iter().position(|s| s == space) {
            return SpaceRef(i);
        }
        self.color_spaces.push(space.clone());
        SpaceRef(self.color_spaces.len() - 1)
    }

    pub fn add_image(&mut self, spec: &ImageSpec, data: &[u8]) -> ImageRef {
        let color_space = spec.color_space.as_ref().map(|s| self.intern_space(s));
        self.images.push(Image {
            spec: spec.clone(),
            color_space,
            data: data.to_vec(),
        });
        ImageRef(self.images.len() - 1)
    }
}

/// One page operation. State settings appear only where they take effect
/// and differ from what the IR last set; `Save`/`Restore` bracket clips.
#[derive(Clone, Debug, PartialEq)]
pub enum IrOp {
    Save,
    Restore,
    LineWidth(f32),
    LineCap(LineCap),
    LineJoin(LineJoin),
    MiterLimit(f32),
    Dash(Vec<f32>, f32),
    Flatness(f32),
    SetColorSpace(SpaceRef),
    SetColor(Vec<f32>),
    Fill {
        path: Vec<Seg>,
        rule: FillRule,
    },
    Stroke {
        path: Vec<Seg>,
        /// The CTM at the stroke, which line width and dash lengths are
        /// measured in.
        ctm: Matrix,
    },
    Clip {
        path: Vec<Seg>,
        rule: FillRule,
    },
    /// Paints the image over the unit square mapped by `matrix` to
    /// default user space, with the first sample row at the top of the
    /// square.
    Image {
        image: ImageRef,
        matrix: Matrix,
    },
    /// Shows a run of glyphs. `matrix` maps glyph space to default user
    /// space at the first glyph; each glyph's displacement, in glyph
    /// space, is applied after it, so the run positions itself. In
    /// writing mode 1 (`wmode`, a composite font's) the matrix places
    /// the first glyph's vertical origin — half its width to the right
    /// of and 0.88 em above its own origin — and each displacement is
    /// the vertical advance; every other run has mode 0.
    Text {
        font: FontIndex,
        matrix: Matrix,
        glyphs: Vec<Glyph>,
        wmode: u8,
    },
}

/// An operation and the source span of the token that caused it. Spans
/// are not yet passed across the backend boundary, so they are `None`.
#[derive(Clone, Debug, PartialEq)]
pub struct Op {
    pub op: IrOp,
    pub span: Option<Span>,
}

impl From<IrOp> for Op {
    fn from(op: IrOp) -> Self {
        Op { op, span: None }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub media_box: Bounds,
    pub ops: Vec<Op>,
    pub resources: Resources,
}

impl Page {
    pub fn new(media_box: Bounds) -> Self {
        Page {
            media_box,
            ops: Vec::new(),
            resources: Resources::default(),
        }
    }

    /// The canonical text form; see [`crate::dump`].
    pub fn dump(&self) -> String {
        crate::dump::page(self)
    }
}

/// Receives each completed page.
pub trait PageSink {
    fn page(&mut self, page: Page);
}

/// Discards pages: what a run that only needs side effects installs.
impl PageSink for () {
    fn page(&mut self, _: Page) {}
}

/// Collects pages in order.
impl PageSink for Vec<Page> {
    fn page(&mut self, page: Page) {
        self.push(page);
    }
}

/// Lets the caller keep a handle on the collected pages while the backend
/// owns the sink.
impl<S: PageSink> PageSink for Rc<RefCell<S>> {
    fn page(&mut self, page: Page) {
        self.borrow_mut().page(page);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spaces_are_interned_by_value() {
        let mut resources = Resources::default();
        let sep = SpaceSpec::Separation {
            name: b"Spot".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceCMYK),
            tint_source: b"{}".to_vec(),
        };
        assert_eq!(resources.intern_space(&SpaceSpec::DeviceRGB), SpaceRef(0));
        assert_eq!(resources.intern_space(&sep), SpaceRef(1));
        assert_eq!(resources.intern_space(&SpaceSpec::DeviceRGB), SpaceRef(0));
        assert_eq!(resources.intern_space(&sep.clone()), SpaceRef(1));
        assert_eq!(resources.color_spaces.len(), 2);
    }

    #[test]
    fn embedded_fonts_measure_through_their_program_and_compare_by_snapshot() {
        use ps_fonts::testing::{TrueTypeFont, Type1Font, rectangle};
        let font = Type1Font::new("Syn")
            .glyph("a", 600, &rectangle(0.0, 0.0, 500.0, 500.0))
            .encode(97, "a");
        let program = ProgramRef(Rc::new(font.program()));
        let mut names: Vec<Option<Vec<u8>>> = vec![None; 256];
        names[97] = Some(b"a".to_vec());
        names[98] = Some(b"zz".to_vec());
        let spec = FontSpec::Embedded {
            family: 1,
            kind: ProgramKind::Type1,
            font_name: b"Syn".to_vec(),
            font_matrix: Matrix::scaling(0.001, 0.001),
            program: program.clone(),
            encoding: glyph_names(&names),
        };
        assert_eq!(spec.width(97), (600.0, 0.0));
        assert_eq!(spec.width(98), (0.0, 0.0), "an unknown name draws .notdef");
        assert_eq!(spec.width(99), (0.0, 0.0));
        assert_eq!(spec.font_matrix(), Matrix::scaling(0.001, 0.001));
        assert!(spec.program_glyph(97).is_some());
        let same = FontSpec::Embedded {
            family: 1,
            kind: ProgramKind::Type1,
            font_name: b"Syn".to_vec(),
            font_matrix: Matrix::scaling(0.001, 0.001),
            program: ProgramRef(program.0.clone()),
            encoding: glyph_names(&names),
        };
        assert_eq!(spec, same);
        let rebuilt = FontSpec::Embedded {
            family: 1,
            kind: ProgramKind::Type1,
            font_name: b"Syn".to_vec(),
            font_matrix: Matrix::scaling(0.001, 0.001),
            program: ProgramRef(Rc::new(font.program())),
            encoding: glyph_names(&names),
        };
        assert_ne!(spec, rebuilt, "a different snapshot is a different font");
        let mut resources = Resources::default();
        assert_eq!(resources.intern_font(spec.clone()), FontIndex(0));
        assert_eq!(resources.intern_font(same), FontIndex(0));
        assert_eq!(resources.intern_font(rebuilt), FontIndex(1));

        let tt = TrueTypeFont::new(2048)
            .glyph(
                "a",
                1024,
                vec![vec![(0, 0, true), (10, 0, true), (10, 10, true)]],
            )
            .map(97, 1);
        let spec = FontSpec::Embedded {
            family: 2,
            kind: ProgramKind::TrueType,
            font_name: b"SynTT".to_vec(),
            font_matrix: Matrix::IDENTITY,
            program: ProgramRef(Rc::new(tt.program().unwrap())),
            encoding: glyph_names(&names),
        };
        assert_eq!(spec.width(97), (0.5, 0.0), "units of the em");
        assert_eq!(spec.width(99), (0.5, 0.0), ".notdef advances half an em");
        assert_eq!(spec.program_glyph(200).unwrap().advance, (1024.0, 0.0));
        assert!(helvetica_like().program_glyph(72).is_none());
    }

    fn helvetica_like() -> FontSpec {
        FontSpec::Resident {
            base: ResidentFace::Helvetica,
            encoding: glyph_names(&[]),
        }
    }

    #[test]
    fn sinks_collect_or_discard() {
        let shared = Rc::new(RefCell::new(Vec::new()));
        let mut sink = shared.clone();
        sink.page(Page::new(Bounds::new(0.0, 0.0, 1.0, 1.0)));
        assert_eq!(shared.borrow().len(), 1);
        ().page(Page::new(Bounds::new(0.0, 0.0, 1.0, 1.0)));
    }
}

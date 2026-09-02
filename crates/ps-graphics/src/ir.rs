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
use std::rc::Rc;

use ps_fonts::StdFont;
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

/// A font a text operation draws with. Resident fonts carry no widths:
/// they come from the metrics in `ps-fonts` by glyph name.
#[derive(Clone, Debug, PartialEq)]
pub enum FontSpec {
    /// One of the standard fourteen with the encoding in effect; glyph
    /// space is thousandths of the em.
    Resident { base: StdFont, encoding: GlyphNames },
    /// A Type 3 font: `font_matrix` maps glyph space to text space, and
    /// every glyph shown on the page has its captured procedure here.
    Type3 {
        font_matrix: Matrix,
        font_bbox: Bounds,
        encoding: GlyphNames,
        glyphs: BTreeMap<GlyphName, GlyphProc>,
    },
}

impl FontSpec {
    pub fn encoding(&self) -> &GlyphNames {
        match self {
            FontSpec::Resident { encoding, .. } | FontSpec::Type3 { encoding, .. } => encoding,
        }
    }

    /// The matrix mapping glyph space to text space.
    pub fn font_matrix(&self) -> Matrix {
        match self {
            FontSpec::Resident { .. } => Matrix::scaling(0.001, 0.001),
            FontSpec::Type3 { font_matrix, .. } => *font_matrix,
        }
    }

    /// The glyph name `code` selects.
    pub fn glyph_name(&self, code: u8) -> Option<&GlyphName> {
        self.encoding()[usize::from(code)].as_ref()
    }

    /// The displacement of `code` in glyph space as the font itself has
    /// it, zero for a code without a glyph.
    pub fn width(&self, code: u8) -> (f32, f32) {
        let Some(name) = self.glyph_name(code) else {
            return (0.0, 0.0);
        };
        match self {
            FontSpec::Resident { base, .. } => std::str::from_utf8(name)
                .ok()
                .and_then(|name| base.width(name))
                .map_or((0.0, 0.0), |w| (f32::from(w), 0.0)),
            FontSpec::Type3 { glyphs, .. } => glyphs.get(name).map_or((0.0, 0.0), |g| g.width),
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
    /// space, is applied after it, so the run positions itself.
    Text {
        font: FontIndex,
        matrix: Matrix,
        glyphs: Vec<Glyph>,
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
    fn sinks_collect_or_discard() {
        let shared = Rc::new(RefCell::new(Vec::new()));
        let mut sink = shared.clone();
        sink.page(Page::new(Bounds::new(0.0, 0.0, 1.0, 1.0)));
        assert_eq!(shared.borrow().len(), 1);
        ().page(Page::new(Bounds::new(0.0, 0.0, 1.0, 1.0)));
    }
}

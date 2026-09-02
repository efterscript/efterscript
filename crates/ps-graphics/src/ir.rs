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
use std::rc::Rc;

use ps_vm::{Bounds, ImageSpec, LineCap, LineJoin, Matrix, Seg, SpaceSpec, Span};

pub use crate::state::FillRule;

/// Index into [`Resources::color_spaces`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpaceRef(pub usize);

/// Index into [`Resources::images`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageRef(pub usize);

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
}

impl Resources {
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

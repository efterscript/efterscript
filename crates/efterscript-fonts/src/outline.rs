// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Glyph outlines and the glyph a program yields for a name, in the
//! program's own glyph space (see [`crate::Program`] for the units).

/// One path segment of an outline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OutlineOp {
    MoveTo(f32, f32),
    LineTo(f32, f32),
    /// Two control points, then the end point.
    CurveTo(f32, f32, f32, f32, f32, f32),
    Close,
}

impl OutlineOp {
    /// The segment with every coordinate mapped through `f`.
    pub fn map(self, mut f: impl FnMut(f32, f32) -> (f32, f32)) -> OutlineOp {
        match self {
            OutlineOp::MoveTo(x, y) => {
                let (x, y) = f(x, y);
                OutlineOp::MoveTo(x, y)
            }
            OutlineOp::LineTo(x, y) => {
                let (x, y) = f(x, y);
                OutlineOp::LineTo(x, y)
            }
            OutlineOp::CurveTo(x1, y1, x2, y2, x, y) => {
                let (x1, y1) = f(x1, y1);
                let (x2, y2) = f(x2, y2);
                let (x, y) = f(x, y);
                OutlineOp::CurveTo(x1, y1, x2, y2, x, y)
            }
            OutlineOp::Close => OutlineOp::Close,
        }
    }
}

/// A glyph outline: subpaths of lines and cubic curves.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outline {
    pub ops: Vec<OutlineOp>,
}

impl Outline {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.ops.push(OutlineOp::MoveTo(x, y));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.ops.push(OutlineOp::LineTo(x, y));
    }

    pub fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.ops.push(OutlineOp::CurveTo(x1, y1, x2, y2, x, y));
    }

    pub fn close(&mut self) {
        self.ops.push(OutlineOp::Close);
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// The outline with every coordinate mapped through `f`.
    pub fn map(&self, mut f: impl FnMut(f32, f32) -> (f32, f32)) -> Outline {
        Outline {
            ops: self.ops.iter().map(|op| op.map(&mut f)).collect(),
        }
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Outline {
        self.map(|x, y| (x + dx, y + dy))
    }

    /// The box enclosing every point, control points included; `None`
    /// for an outline without points.
    pub fn control_box(&self) -> Option<[f32; 4]> {
        let mut bbox: Option<[f32; 4]> = None;
        let mut extend = |x: f32, y: f32| {
            bbox = Some(match bbox {
                None => [x, y, x, y],
                Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x), y1.max(y)],
            });
        };
        for op in &self.ops {
            match *op {
                OutlineOp::MoveTo(x, y) | OutlineOp::LineTo(x, y) => extend(x, y),
                OutlineOp::CurveTo(x1, y1, x2, y2, x, y) => {
                    extend(x1, y1);
                    extend(x2, y2);
                    extend(x, y);
                }
                OutlineOp::Close => {}
            }
        }
        bbox
    }
}

/// What a program yields for a glyph name: the advance and the outline,
/// both in the program's glyph space.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub advance: (f32, f32),
    pub outline: Outline,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_box_covers_control_points() {
        let mut o = Outline::new();
        assert_eq!(o.control_box(), None);
        o.move_to(1.0, 2.0);
        o.line_to(5.0, -1.0);
        o.curve_to(9.0, 9.0, -3.0, 4.0, 2.0, 2.0);
        o.close();
        assert_eq!(o.control_box(), Some([-3.0, -1.0, 9.0, 9.0]));
        let t = o.translated(10.0, 20.0);
        assert_eq!(t.ops[0], OutlineOp::MoveTo(11.0, 22.0));
        assert_eq!(
            t.ops[2],
            OutlineOp::CurveTo(19.0, 29.0, 7.0, 24.0, 12.0, 22.0)
        );
        assert_eq!(t.ops[3], OutlineOp::Close);
        assert!(!t.is_empty());
        assert!(Outline::new().is_empty());
    }
}

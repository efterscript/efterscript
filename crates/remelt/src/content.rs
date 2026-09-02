// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The content stream of one page: the IR's operations in PDF's operator
//! syntax (ISO 32000-1 §8.4, §8.5, §8.6, §8.9), one operation per line,
//! operands before the operator, numbers in the writer's canonical form.
//! The text mirrors the `ir/1` dump line for line, so an uncompressed
//! stream diffs the way the dump does.
//!
//! Paths arrive in default user space and go out unchanged, except under
//! a stroke recorded with a CTM: PDF measures line width and dash lengths
//! in the space current at the stroke, so the stroke is wrapped in its
//! matrix and the path taken back through the inverse — exact for any
//! invertible matrix, including skew and non-uniform scale. A singular
//! matrix has no inverse and would collapse the page's transform; that
//! stroke is written unwrapped with the width as recorded.

use pdf_out::fmt_real;
use ps_graphics::{FillRule, IrOp, Page, Resources, SpaceRef};
use ps_vm::{Matrix, Point, Seg, SpaceSpec};

use crate::resources::{image_name, space_name};

/// Which colour operator the space in effect takes: the device families
/// have direct operators, everything else is selected by resource name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColorOp {
    Gray,
    Rgb,
    Cmyk,
    Named,
}

impl ColorOp {
    fn of(space: &SpaceSpec) -> Self {
        match space {
            SpaceSpec::DeviceGray => ColorOp::Gray,
            SpaceSpec::DeviceRGB => ColorOp::Rgb,
            SpaceSpec::DeviceCMYK => ColorOp::Cmyk,
            _ => ColorOp::Named,
        }
    }

    fn operator(self) -> &'static str {
        match self {
            ColorOp::Gray => "g",
            ColorOp::Rgb => "rg",
            ColorOp::Cmyk => "k",
            ColorOp::Named => "scn",
        }
    }
}

fn reals(values: &[f32]) -> String {
    values
        .iter()
        .map(|&v| fmt_real(v))
        .collect::<Vec<_>>()
        .join(" ")
}

fn matrix(m: Matrix) -> String {
    reals(&m.0)
}

struct Writer<'a> {
    out: String,
    resources: &'a Resources,
    /// The colour operator in effect, one entry per open `q` plus the
    /// base: `Q` restores the colour space with the rest of the state.
    color_ops: Vec<ColorOp>,
}

impl Writer<'_> {
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    fn color_op(&self) -> ColorOp {
        *self
            .color_ops
            .last()
            .expect("the base entry is never popped")
    }

    fn segments(&mut self, path: &[Seg], map: impl Fn(Point) -> Point) {
        for seg in path {
            match *seg {
                Seg::Move(p) => {
                    let p = map(p);
                    self.line(&format!("{} m", reals(&[p.x, p.y])));
                }
                Seg::Line(p) => {
                    let p = map(p);
                    self.line(&format!("{} l", reals(&[p.x, p.y])));
                }
                Seg::Curve(a, b, c) => {
                    let (a, b, c) = (map(a), map(b), map(c));
                    self.line(&format!("{} c", reals(&[a.x, a.y, b.x, b.y, c.x, c.y])));
                }
                Seg::Close => self.line("h"),
            }
        }
    }

    fn set_color_space(&mut self, space: SpaceRef) {
        let spec = &self.resources.color_spaces[space.0];
        let op = ColorOp::of(spec);
        match op {
            ColorOp::Named => self.line(&format!("/{} cs", space_name(space))),
            _ => self.line(&format!("/{} cs", spec.family())),
        }
        *self
            .color_ops
            .last_mut()
            .expect("the base entry is never popped") = op;
    }

    fn op(&mut self, op: &IrOp) {
        match op {
            IrOp::Save => {
                self.color_ops.push(self.color_op());
                self.line("q");
            }
            IrOp::Restore => {
                if self.color_ops.len() > 1 {
                    self.color_ops.pop();
                }
                self.line("Q");
            }
            IrOp::LineWidth(w) => self.line(&format!("{} w", fmt_real(*w))),
            IrOp::LineCap(cap) => self.line(&format!("{} J", cap.code())),
            IrOp::LineJoin(join) => self.line(&format!("{} j", join.code())),
            IrOp::MiterLimit(m) => self.line(&format!("{} M", fmt_real(*m))),
            IrOp::Dash(lengths, phase) => {
                self.line(&format!("[{}] {} d", reals(lengths), fmt_real(*phase)));
            }
            IrOp::Flatness(f) => self.line(&format!("{} i", fmt_real(*f))),
            IrOp::SetColorSpace(space) => self.set_color_space(*space),
            IrOp::SetColor(components) => {
                let operator = self.color_op().operator();
                self.line(&format!("{} {operator}", reals(components)));
            }
            IrOp::Fill { path, rule } => {
                self.segments(path, |p| p);
                self.line(match rule {
                    FillRule::NonZero => "f",
                    FillRule::EvenOdd => "f*",
                });
            }
            IrOp::Stroke { path, ctm } => {
                let inverse = if *ctm == Matrix::IDENTITY {
                    None
                } else {
                    ctm.inverse()
                };
                match inverse {
                    Some(inverse) => {
                        self.line("q");
                        self.line(&format!("{} cm", matrix(*ctm)));
                        self.segments(path, |p| inverse.apply(p));
                        self.line("S");
                        self.line("Q");
                    }
                    None => {
                        self.segments(path, |p| p);
                        self.line("S");
                    }
                }
            }
            IrOp::Clip { path, rule } => {
                self.segments(path, |p| p);
                self.line(match rule {
                    FillRule::NonZero => "W n",
                    FillRule::EvenOdd => "W* n",
                });
            }
            IrOp::Image { image, matrix: m } => {
                self.line(&format!("q {} cm /{} Do Q", matrix(*m), image_name(*image)));
            }
        }
    }
}

/// The content stream of `page`, as text bytes.
pub(crate) fn content(page: &Page) -> Vec<u8> {
    let mut writer = Writer {
        out: String::new(),
        resources: &page.resources,
        color_ops: vec![ColorOp::Gray],
    };
    for entry in &page.ops {
        writer.op(&entry.op);
    }
    writer.out.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    use ps_graphics::{ImageRef, Op};
    use ps_vm::{Bounds, LineCap, LineJoin};

    fn page(ops: Vec<IrOp>) -> Page {
        let mut page = Page::new(Bounds::new(0.0, 0.0, 612.0, 792.0));
        page.ops = ops.into_iter().map(Op::from).collect();
        page
    }

    fn text(page: &Page) -> String {
        String::from_utf8(content(page)).unwrap()
    }

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    #[test]
    fn state_operators_take_pdf_names() {
        let page = page(vec![
            IrOp::LineWidth(2.0),
            IrOp::LineCap(LineCap::Round),
            IrOp::LineJoin(LineJoin::Bevel),
            IrOp::MiterLimit(4.0),
            IrOp::Dash(vec![3.0, 1.0], 0.5),
            IrOp::Dash(Vec::new(), 0.0),
            IrOp::Flatness(0.5),
        ]);
        assert_eq!(
            text(&page),
            "2 w\n1 J\n2 j\n4 M\n[3 1] 0.5 d\n[] 0 d\n0.5 i\n"
        );
    }

    #[test]
    fn paths_paint_with_their_rule() {
        let path = vec![
            Seg::Move(p(0.0, 0.0)),
            Seg::Line(p(10.0, 0.0)),
            Seg::Curve(p(10.0, 5.0), p(5.0, 10.0), p(0.0, 10.0)),
            Seg::Close,
        ];
        let page = page(vec![
            IrOp::Save,
            IrOp::Clip {
                path: path.clone(),
                rule: FillRule::EvenOdd,
            },
            IrOp::Fill {
                path: path.clone(),
                rule: FillRule::NonZero,
            },
            IrOp::Restore,
            IrOp::Clip {
                path: path.clone(),
                rule: FillRule::NonZero,
            },
            IrOp::Fill {
                path,
                rule: FillRule::EvenOdd,
            },
        ]);
        let segs = "0 0 m\n10 0 l\n10 5 5 10 0 10 c\nh\n";
        assert_eq!(
            text(&page),
            format!("q\n{segs}W* n\n{segs}f\nQ\n{segs}W n\n{segs}f*\n")
        );
    }

    #[test]
    fn strokes_wrap_their_ctm_unless_identity_or_singular() {
        let path = vec![Seg::Move(p(10.0, 10.0)), Seg::Line(p(100.0, 10.0))];
        let plain = page(vec![IrOp::Stroke {
            path: path.clone(),
            ctm: Matrix::IDENTITY,
        }]);
        assert_eq!(text(&plain), "10 10 m\n100 10 l\nS\n");
        let scaled = page(vec![IrOp::Stroke {
            path: path.clone(),
            ctm: Matrix::scaling(2.0, 2.0),
        }]);
        assert_eq!(text(&scaled), "q\n2 0 0 2 0 0 cm\n5 5 m\n50 5 l\nS\nQ\n");
        let singular = page(vec![IrOp::Stroke {
            path,
            ctm: Matrix::scaling(0.0, 2.0),
        }]);
        assert_eq!(text(&singular), "10 10 m\n100 10 l\nS\n");
    }

    #[test]
    fn colour_operators_follow_the_space_across_save_and_restore() {
        let mut page = page(Vec::new());
        let rgb = page.resources.intern_space(&SpaceSpec::DeviceRGB);
        let sep = page.resources.intern_space(&SpaceSpec::Separation {
            name: b"Spot".to_vec(),
            alternate: Box::new(SpaceSpec::DeviceCMYK),
            tint_source: b"{}".to_vec(),
        });
        let cmyk = page.resources.intern_space(&SpaceSpec::DeviceCMYK);
        page.ops = vec![
            IrOp::SetColor(vec![0.5]),
            IrOp::SetColorSpace(rgb),
            IrOp::SetColor(vec![0.2, 0.4, 0.6]),
            IrOp::Save,
            IrOp::SetColorSpace(sep),
            IrOp::SetColor(vec![0.6]),
            IrOp::Restore,
            IrOp::SetColor(vec![1.0, 1.0, 1.0]),
            IrOp::SetColorSpace(cmyk),
            IrOp::SetColor(vec![0.0, 0.0, 0.0, 1.0]),
        ]
        .into_iter()
        .map(Op::from)
        .collect();
        assert_eq!(
            text(&page),
            "0.5 g\n/DeviceRGB cs\n0.2 0.4 0.6 rg\nq\n/CS1 cs\n0.6 scn\nQ\n1 1 1 rg\n/DeviceCMYK cs\n0 0 0 1 k\n"
        );
    }

    #[test]
    fn images_paint_inside_their_matrix() {
        let page = page(vec![IrOp::Image {
            image: ImageRef(3),
            matrix: Matrix([50.0, 0.0, 0.0, 50.0, 100.0, 100.0]),
        }]);
        assert_eq!(text(&page), "q 50 0 0 50 100 100 cm /Im3 Do Q\n");
    }

    #[test]
    fn an_unbalanced_restore_does_not_panic() {
        let page = page(vec![IrOp::Restore, IrOp::SetColor(vec![1.0])]);
        assert_eq!(text(&page), "Q\n1 g\n");
    }
}

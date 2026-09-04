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
//!
//! A text run becomes one text object (ISO 32000-1 §9.4): the font at
//! size 1, a text matrix derived from the run's glyph matrix, and the
//! glyph codes. A glyph whose recorded displacement is the font's own
//! width needs nothing more; a difference along the writing direction
//! is a `TJ` adjustment, and any difference across it moves the line
//! with `Td`. The text matrix takes the run's glyph-to-page matrix back
//! through the font's own matrix: thousandths for a resident font, the
//! Type 3 or embedded font's `FontMatrix` otherwise (a composite font's
//! descendant's), so the PDF font's glyph space lands where the
//! program's did. A composite run's string holds two-byte CIDs, written
//! in hexadecimal; in writing mode 1 the font's `Identity-V` encoding
//! has the viewer place each glyph at its vertical origin and advance
//! downward, as the run was recorded. A composite font written as a
//! Type 3 fallback has its CIDs re-encoded to the one-byte codes the
//! font object assigned (`Recode`), and in vertical mode its glyphs are
//! drawn pre-shifted with zero width, so every advance is a `Td` move.

use std::collections::BTreeMap;

use pdf_out::fmt_real;
use ps_graphics::{FillRule, FontIndex, FontSpec, IrOp, Op, Page, Resources, SpaceRef};
use ps_vm::{Glyph, Matrix, Point, Seg, SpaceSpec};

use crate::resources::{font_name, image_name, space_name};

/// The one-byte code each CID takes in a composite font written as a
/// Type 3 fallback, by the page's font index; fonts absent here are
/// written with their CIDs.
pub(crate) type Recode = BTreeMap<usize, BTreeMap<u16, u8>>;

/// Content-stream text and what could not be written into it.
pub(crate) struct Rendered {
    pub bytes: Vec<u8>,
    pub notes: Vec<String>,
}

/// A string operand: literal when every byte is printable ASCII,
/// hexadecimal otherwise or when `hex` asks for it.
fn pdf_string(bytes: &[u8], hex: bool) -> String {
    if !hex && bytes.iter().all(|b| (0x20..=0x7E).contains(b)) {
        let mut out = String::from("(");
        for &b in bytes {
            if matches!(b, b'(' | b')' | b'\\') {
                out.push('\\');
            }
            out.push(b as char);
        }
        out.push(')');
        out
    } else {
        let mut out = String::from("<");
        for b in bytes {
            out.push_str(&format!("{b:02X}"));
        }
        out.push('>');
        out
    }
}

/// One element of a `TJ` array.
enum Piece {
    Codes(Vec<u8>),
    Adjust(f32),
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-5
}

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

/// `matrix` taken back through `font_matrix`, computed in double
/// precision; `None` for a singular font matrix.
fn through_inverse(font_matrix: Matrix, matrix: Matrix) -> Option<Matrix> {
    let [a, b, c, d, tx, ty] = font_matrix.0.map(f64::from);
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    let (itx, ity) = (-(tx * ia + ty * ic), -(tx * ib + ty * id));
    let [a2, b2, c2, d2, tx2, ty2] = matrix.0.map(f64::from);
    Some(Matrix(
        [
            ia * a2 + ib * c2,
            ia * b2 + ib * d2,
            ic * a2 + id * c2,
            ic * b2 + id * d2,
            itx * a2 + ity * c2 + tx2,
            itx * b2 + ity * d2 + ty2,
        ]
        .map(|v| v as f32 + 0.0),
    ))
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
    recode: &'a Recode,
    /// The colour operator in effect, one entry per open `q` plus the
    /// base: `Q` restores the colour space with the rest of the state.
    color_ops: Vec<ColorOp>,
    notes: Vec<String>,
}

/// How a run's glyphs are encoded in the content stream.
enum Codes<'a> {
    /// One byte per glyph: the code, which is the CID for a simple font.
    Byte,
    /// Two bytes per glyph: the CID of a composite font.
    Cid,
    /// The Type 3 fallback's one-byte codes by CID; a CID it lacks is
    /// code 0.
    Recoded(&'a BTreeMap<u16, u8>),
}

impl Writer<'_> {
    fn line(&mut self, text: &str) {
        self.out.push_str(text);
        self.out.push('\n');
    }

    /// Writes the pieces of a run gathered since the last positioning:
    /// `Tj` for a plain string, `TJ` when adjustments are among them.
    fn show(&mut self, pieces: &mut Vec<Piece>, hex: bool) {
        match pieces.as_slice() {
            [] => {}
            [Piece::Codes(codes)] => {
                let text = format!("{} Tj", pdf_string(codes, hex));
                self.line(&text);
            }
            _ => {
                let items: Vec<String> = pieces
                    .iter()
                    .map(|piece| match piece {
                        Piece::Codes(codes) => pdf_string(codes, hex),
                        Piece::Adjust(v) => fmt_real(*v),
                    })
                    .collect();
                let text = format!("[{}] TJ", items.join(" "));
                self.line(&text);
            }
        }
        pieces.clear();
    }

    /// One text object for the run. Positions are tracked in text space:
    /// where PDF's own advance leaves the pen after each glyph against
    /// where the recorded displacement puts the next one.
    fn text(&mut self, font: FontIndex, matrix: Matrix, glyphs: &[Glyph], wmode: u8) {
        let spec = &self.resources.fonts[font.0];
        let glyph_to_text = spec.font_matrix();
        let tm = match spec {
            FontSpec::Resident { .. } => {
                let [a, b, c, d, tx, ty] = matrix.0;
                Matrix([a * 1000.0, b * 1000.0, c * 1000.0, d * 1000.0, tx, ty])
            }
            FontSpec::Type3 { font_matrix, .. } => match font_matrix.inverse() {
                Some(inverse) => inverse.then(matrix),
                None => {
                    self.notes.push(format!(
                        "text in font {} skipped: its font matrix is singular",
                        font_name(font)
                    ));
                    return;
                }
            },
            // In double precision: the inverse of a thousandths matrix in
            // single precision would print as 999.99994.
            FontSpec::Embedded { .. } | FontSpec::Composite { .. } => {
                match through_inverse(glyph_to_text, matrix) {
                    Some(tm) => tm,
                    None => {
                        self.notes.push(format!(
                            "text in font {} skipped: its font matrix is singular",
                            font_name(font)
                        ));
                        return;
                    }
                }
            }
        };
        let codes = match (spec, self.recode.get(&font.0)) {
            (FontSpec::Composite { .. }, Some(map)) => Codes::Recoded(map),
            (FontSpec::Composite { .. }, None) => Codes::Cid,
            _ => Codes::Byte,
        };
        // A vertical composite font advances the pen down by the default
        // vertical advance, one em, and its adjustments run along that
        // axis; the fallback and every simple font write horizontally.
        let vertical = wmode == 1 && matches!(codes, Codes::Cid);
        let (main, cross) = if vertical { (1, 0) } else { (0, 1) };
        self.line("BT");
        let select = format!("/{} 1 Tf", font_name(font));
        self.line(&select);
        let place = format!("{} Tm", reals(&tm.0));
        self.line(&place);
        let mut pieces: Vec<Piece> = Vec::new();
        // Positions accumulate in double precision so an adjustment of
        // whole glyph units prints as one.
        let mut line_start = [0.0f64; 2];
        let mut pen = [0.0f64; 2];
        let mut wanted = [0.0f64; 2];
        let hex = matches!(codes, Codes::Cid);
        for glyph in glyphs {
            if !same(pen[cross], wanted[cross]) {
                self.show(&mut pieces, hex);
                let step = format!(
                    "{} Td",
                    reals(&[
                        (wanted[0] - line_start[0]) as f32,
                        (wanted[1] - line_start[1]) as f32
                    ])
                );
                self.line(&step);
                line_start = wanted;
                pen = wanted;
            } else if !same(pen[main], wanted[main]) {
                pieces.push(Piece::Adjust(((pen[main] - wanted[main]) * 1000.0) as f32));
                pen[main] = wanted[main];
            }
            let bytes: Vec<u8> = match codes {
                Codes::Byte => vec![u8::try_from(glyph.cid).unwrap_or(0)],
                Codes::Cid => glyph.cid.to_be_bytes().to_vec(),
                Codes::Recoded(map) => vec![map.get(&glyph.cid).copied().unwrap_or(0)],
            };
            match pieces.last_mut() {
                Some(Piece::Codes(codes)) => codes.extend(bytes),
                _ => pieces.push(Piece::Codes(bytes)),
            }
            let (wx, _) = spec.glyph_width(glyph);
            let own = match codes {
                Codes::Cid if vertical => -1.0,
                Codes::Recoded(_) if wmode == 1 => 0.0,
                _ => f64::from(wx) * f64::from(glyph_to_text.0[0]),
            };
            pen[main] += own;
            let advance = glyph_to_text.apply_delta(Point::new(glyph.dx, glyph.dy));
            wanted = [
                wanted[0] + f64::from(advance.x),
                wanted[1] + f64::from(advance.y),
            ];
        }
        self.show(&mut pieces, hex);
        self.line("ET");
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
            IrOp::Text {
                font,
                matrix,
                glyphs,
                wmode,
            } => self.text(*font, *matrix, glyphs, *wmode),
        }
    }
}

/// `ops` as content-stream text against `resources`: a page's stream or
/// a glyph procedure's; `recode` names the composite fonts written as
/// Type 3 fallbacks and their codes.
pub(crate) fn render(ops: &[Op], resources: &Resources, recode: &Recode) -> Rendered {
    let mut writer = Writer {
        out: String::new(),
        resources,
        recode,
        color_ops: vec![ColorOp::Gray],
        notes: Vec::new(),
    };
    for entry in ops {
        writer.op(&entry.op);
    }
    Rendered {
        bytes: writer.out.into_bytes(),
        notes: writer.notes,
    }
}

/// The content stream of `page`.
pub(crate) fn content(page: &Page, recode: &Recode) -> Rendered {
    render(&page.ops, &page.resources, recode)
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
        String::from_utf8(content(page, &Recode::default()).bytes).unwrap()
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
    fn the_inverse_font_matrix_is_exact_in_double_precision() {
        let tm = through_inverse(
            Matrix::scaling(0.001, 0.001),
            Matrix([0.01, 0.0, 0.0, 0.01, 5.0, 7.0]),
        )
        .unwrap();
        assert_eq!(reals(&tm.0), "10 0 0 10 5 7");
        assert_eq!(
            through_inverse(Matrix::scaling(0.0, 1.0), Matrix::IDENTITY),
            None
        );
        let shifted = through_inverse(Matrix::translation(3.0, 4.0), Matrix::IDENTITY).unwrap();
        assert_eq!(reals(&shifted.0), "1 0 0 1 -3 -4");
    }

    #[test]
    fn an_unbalanced_restore_does_not_panic() {
        let page = page(vec![IrOp::Restore, IrOp::SetColor(vec![1.0])]);
        assert_eq!(text(&page), "Q\n1 g\n");
    }
}

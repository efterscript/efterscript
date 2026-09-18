// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Property tests: a stroke path mapped back through its CTM's inverse
//! comes out where the program drew it, and the identity and singular
//! cases never wrap.

mod support;

use efterscript_graphics::{IrOp, Op, Page};
use efterscript_remelt::Options;
use efterscript_vm::{Bounds, Matrix, Point, Seg};
use proptest::prelude::*;
use support::{check, content, distil_pages};

/// Matrices whose inverse is well conditioned, so the round trip through
/// `f32` arithmetic and six-digit reals stays within a fixed tolerance.
fn invertible() -> impl Strategy<Value = Matrix> {
    let coefficient = -10.0f32..10.0;
    let offset = -100.0f32..100.0;
    (
        coefficient.clone(),
        coefficient.clone(),
        coefficient.clone(),
        coefficient,
        offset.clone(),
        offset,
    )
        .prop_map(|(a, b, c, d, tx, ty)| Matrix([a, b, c, d, tx, ty]))
        .prop_filter("well conditioned", |m| {
            let [a, b, c, d, ..] = m.0;
            (a * d - b * c).abs() > 1.0
        })
}

fn point() -> impl Strategy<Value = Point> {
    (-100.0f32..100.0, -100.0f32..100.0).prop_map(|(x, y)| Point::new(x, y))
}

fn near(got: f32, want: f32) -> bool {
    (got - want).abs() <= 1e-2 * (1.0 + want.abs())
}

/// A page holding one stroke of `user` points drawn under `ctm`, the way
/// the backend records it: the path already through the matrix.
fn stroke_page(user: &[Point], ctm: Matrix) -> Page {
    let mut page = Page::new(Bounds::new(0.0, 0.0, 612.0, 792.0));
    let path = user
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            let p = ctm.apply(p);
            if i == 0 { Seg::Move(p) } else { Seg::Line(p) }
        })
        .collect();
    page.ops.push(Op::from(IrOp::Stroke { path, ctm }));
    page
}

/// The `x y m` / `x y l` operands of a content stream.
fn path_points(text: &str) -> Vec<Point> {
    text.lines()
        .filter(|line| line.ends_with(" m") || line.ends_with(" l"))
        .map(|line| {
            let mut fields = line.split(' ');
            let x: f32 = fields.next().unwrap().parse().unwrap();
            let y: f32 = fields.next().unwrap().parse().unwrap();
            Point::new(x, y)
        })
        .collect()
}

proptest! {
    #[test]
    fn a_wrapped_stroke_path_returns_to_user_space(
        ctm in invertible(),
        user in proptest::collection::vec(point(), 1..6),
    ) {
        let pdf = check(&distil_pages(
            vec![stroke_page(&user, ctm)],
            Options::compress(false),
        ));
        let text = content(&pdf, 0);
        prop_assert!(text.starts_with("q\n"), "{text}");
        prop_assert!(text.ends_with("S\nQ\n"), "{text}");
        let got = path_points(&text);
        prop_assert_eq!(got.len(), user.len());
        for (g, w) in got.iter().zip(&user) {
            prop_assert!(near(g.x, w.x) && near(g.y, w.y), "{g:?} vs {w:?} under {ctm:?}");
        }
    }

    #[test]
    fn identity_and_singular_strokes_are_never_wrapped(
        user in proptest::collection::vec(point(), 1..6),
        scale in -10.0f32..10.0,
    ) {
        for ctm in [Matrix::IDENTITY, Matrix::scaling(scale, 0.0), Matrix([1.0, 2.0, 2.0, 4.0, 3.0, 3.0])] {
            let pdf = check(&distil_pages(
                vec![stroke_page(&user, ctm)],
                Options::compress(false),
            ));
            let text = content(&pdf, 0);
            prop_assert!(!text.contains(" cm\n"), "{text}");
            prop_assert!(text.ends_with(" l\nS\n") || text.ends_with(" m\nS\n"), "{text}");
            let want: Vec<Point> = user.iter().map(|&p| ctm.apply(p)).collect();
            for (g, w) in path_points(&text).iter().zip(&want) {
                prop_assert!(near(g.x, w.x) && near(g.y, w.y), "{g:?} vs {w:?}");
            }
        }
    }
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Property tests: the CTM round trip, the emptiness of the IR without
//! paints, dump determinism, and the agreement of the dump's real syntax
//! with the PDF writer's.

use std::cell::RefCell;
use std::rc::Rc;

use efterscript_graphics::{Graphics, Page, dump, fmt_real};
use efterscript_vm::{
    Config, GraphicsBackend, Interp, Io, Matrix, Outcome, Point, Rect, SliceSource,
};
use proptest::prelude::*;

type Pages = Rc<RefCell<Vec<Page>>>;

fn backend() -> (Graphics<Pages>, Pages) {
    let pages: Pages = Rc::new(RefCell::new(Vec::new()));
    (Graphics::new(pages.clone()), pages)
}

/// Matrices whose inverse is well conditioned, so the round trip through
/// `f32` arithmetic stays within a fixed tolerance.
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

/// One backend call, as the determinism and no-paint properties drive it.
#[derive(Clone, Debug)]
enum Call {
    GSave,
    GRestore,
    LineWidth(f32),
    Gray(f32),
    Rgb(f32, f32, f32),
    Translate(f32, f32),
    Scale(f32, f32),
    MoveTo(Point),
    LineTo(Point),
    Arc(Point, f32, f32, f32),
    Close,
    Fill,
    Stroke,
    Clip,
    RectFill(Rect),
    InitClip,
    ShowPage,
}

impl Call {
    fn paints(&self) -> bool {
        matches!(
            self,
            Call::Fill | Call::Stroke | Call::RectFill(_) | Call::ShowPage
        )
    }

    /// Applies the call; errors (no current point, and the like) are the
    /// program's problem, not a determinism concern.
    fn apply(&self, g: &mut Graphics<Pages>) {
        let _ = match *self {
            Call::GSave => g.gsave(),
            Call::GRestore => g.grestore(),
            Call::LineWidth(w) => g.set_line_width(w),
            Call::Gray(v) => g
                .set_color_space(&efterscript_vm::SpaceSpec::DeviceGray)
                .and_then(|()| g.set_color(&[v])),
            Call::Rgb(r, gg, b) => g
                .set_color_space(&efterscript_vm::SpaceSpec::DeviceRGB)
                .and_then(|()| g.set_color(&[r, gg, b])),
            Call::Translate(x, y) => g.concat(Matrix::translation(x, y)),
            Call::Scale(x, y) => g.concat(Matrix::scaling(x, y)),
            Call::MoveTo(p) => g.moveto(p),
            Call::LineTo(p) => g.lineto(p),
            Call::Arc(c, r, s, e) => g.arc(c, r, s, e),
            Call::Close => g.closepath(),
            Call::Fill => g.fill(),
            Call::Stroke => g.stroke(),
            Call::Clip => g.clip(),
            Call::RectFill(r) => g.rectfill(&[r]),
            Call::InitClip => g.initclip(),
            Call::ShowPage => g.showpage(),
        };
    }
}

fn call() -> impl Strategy<Value = Call> {
    let unit = 0.0f32..=1.0;
    let scale = 0.5f32..3.0;
    prop_oneof![
        Just(Call::GSave),
        Just(Call::GRestore),
        (0.0f32..10.0).prop_map(Call::LineWidth),
        unit.clone().prop_map(Call::Gray),
        (unit.clone(), unit.clone(), unit).prop_map(|(r, g, b)| Call::Rgb(r, g, b)),
        point().prop_map(|p| Call::Translate(p.x, p.y)),
        (scale.clone(), scale).prop_map(|(x, y)| Call::Scale(x, y)),
        point().prop_map(Call::MoveTo),
        point().prop_map(Call::LineTo),
        (point(), 1.0f32..50.0, -360.0f32..360.0, -360.0f32..360.0)
            .prop_map(|(c, r, s, e)| Call::Arc(c, r, s, e)),
        Just(Call::Close),
        Just(Call::Fill),
        Just(Call::Stroke),
        Just(Call::Clip),
        (point(), 1.0f32..50.0, 1.0f32..50.0).prop_map(|(p, w, h)| Call::RectFill(Rect {
            x: p.x,
            y: p.y,
            width: w,
            height: h,
        })),
        Just(Call::InitClip),
        Just(Call::ShowPage),
    ]
}

fn run_calls(calls: &[Call]) -> (Graphics<Pages>, Pages) {
    let (mut g, pages) = backend();
    for call in calls {
        call.apply(&mut g);
    }
    (g, pages)
}

/// Every numeric token of a dump line is a plain decimal that parses:
/// no exponent, no `inf`, no `NaN`.
fn numbers_are_canonical(line: &str) -> bool {
    line.split_whitespace()
        .map(|token| token.trim_matches(['[', ']']))
        .filter(|token| {
            token.starts_with(['-', '.']) || token.starts_with(|c: char| c.is_ascii_digit())
        })
        .all(|token| !token.contains(['e', 'E']) && token.parse::<f32>().is_ok_and(f32::is_finite))
}

fn dump_all(g: &Graphics<Pages>, pages: &Pages) -> String {
    let mut text = dump::pages(&pages.borrow());
    let mut open = Page::new(g.state().media_box);
    open.ops = g.ops().to_vec();
    text.push_str(&open.dump());
    text
}

proptest! {
    #[test]
    fn current_point_round_trips_through_the_ctm(m in invertible(), p in point()) {
        let (mut g, _) = backend();
        g.set_matrix(m).unwrap();
        g.moveto(p).unwrap();
        let got = g.current_point().unwrap();
        prop_assert!(near(got.x, p.x) && near(got.y, p.y), "{p:?} came back as {got:?}");
        // A later CTM change moves the answer, not the point.
        g.concat(Matrix::translation(3.0, -7.0)).unwrap();
        let moved = g.current_point().unwrap();
        prop_assert!(near(moved.x, p.x - 3.0) && near(moved.y, p.y + 7.0));
    }

    #[test]
    fn transform_and_itransform_are_inverses(m in invertible(), p in point()) {
        let [a, b, c, d, tx, ty] = m.0;
        let program = format!(
            "[{a} {b} {c} {d} {tx} {ty}] setmatrix {} {} transform itransform = =",
            p.x, p.y
        );
        let (io, out, _) = Io::capture();
        let mut interp = Interp::with_config(Config {
            io,
            ..Default::default()
        });
        interp.set_graphics_backend(Box::new(Graphics::new(())));
        prop_assert_eq!(interp.run(&mut SliceSource::new(program.as_bytes())), Outcome::Ok);
        let text = out.text();
        let mut values = text.lines().map(|l| l.parse::<f32>().unwrap());
        let y = values.next().unwrap();
        let x = values.next().unwrap();
        prop_assert!(near(x, p.x) && near(y, p.y), "{p:?} came back as {x} {y}");
    }

    #[test]
    fn nothing_painted_means_nothing_recorded(calls in prop::collection::vec(call(), 0..40)) {
        let quiet: Vec<Call> = calls.into_iter().filter(|c| !c.paints()).collect();
        let (g, pages) = run_calls(&quiet);
        prop_assert!(g.ops().is_empty());
        prop_assert!(pages.borrow().is_empty());
    }

    #[test]
    fn dumps_are_deterministic_and_well_formed(calls in prop::collection::vec(call(), 0..60)) {
        let (g1, p1) = run_calls(&calls);
        let (g2, p2) = run_calls(&calls);
        let first = dump_all(&g1, &p1);
        prop_assert_eq!(&first, &dump_all(&g2, &p2));
        prop_assert_eq!(&first, &dump_all(&g1, &p1));
        for line in first.lines() {
            prop_assert!(numbers_are_canonical(line), "{line}");
        }
        for page in p1.borrow().iter() {
            prop_assert!(page.dump().starts_with("ir/1\npage 612 792\nresources:\n"));
        }
    }

    #[test]
    fn real_syntax_agrees_with_the_pdf_writer(v in any::<f32>()) {
        prop_assert_eq!(fmt_real(v), efterscript_pdf::fmt_real(v));
    }

    #[test]
    fn real_syntax_round_trips_to_six_digits(v in -1.0e6f32..1.0e6) {
        let text = fmt_real(v);
        let back: f32 = text.parse().unwrap();
        prop_assert!((back - v).abs() <= 1e-5 * (1.0 + v.abs()), "{v} -> {text}");
        prop_assert!(!text.contains(['e', 'E']));
        prop_assert!(!text.ends_with('.'));
        prop_assert!(text == "0" || !text.ends_with('0') || !text.contains('.'));
    }
}

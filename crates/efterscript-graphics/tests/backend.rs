// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The backend driven directly, without an interpreter: path building
//! through the CTM, arcs, emission and its deduplication, clips, and page
//! delivery.

use std::cell::RefCell;
use std::rc::Rc;

use efterscript_graphics::{
    FillRule, Graphics, IrOp, Page, PatternIndex, PatternSpec, ShadingIndex, SpaceRef,
};
use efterscript_vm::{
    Bounds, FormInfo, FunctionSpec, GraphicsBackend, ImageSpec, LineCap, Matrix, PatternInfo,
    PatternKind, Point, Rect, Seg, ShadingKind, ShadingSpec, SpaceSpec, VmError,
};

type Pages = Rc<RefCell<Vec<Page>>>;

fn backend() -> (Graphics<Pages>, Pages) {
    let pages: Pages = Rc::new(RefCell::new(Vec::new()));
    (Graphics::new(pages.clone()), pages)
}

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn ops(g: &Graphics<Pages>) -> Vec<IrOp> {
    g.ops().iter().map(|o| o.op.clone()).collect()
}

fn close(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
}

fn line(g: &mut Graphics<Pages>, from: Point, to: Point) {
    g.moveto(from).unwrap();
    g.lineto(to).unwrap();
}

// --- paths ---------------------------------------------------------------------

#[test]
fn points_are_transformed_when_added_not_later() {
    let (mut g, _) = backend();
    g.concat(Matrix::translation(72.0, 72.0)).unwrap();
    line(&mut g, p(0.0, 0.0), p(72.0, 0.0));
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    g.lineto(p(0.0, 10.0)).unwrap();
    g.stroke().unwrap();
    assert_eq!(
        ops(&g),
        [IrOp::Stroke {
            path: vec![
                Seg::Move(p(72.0, 72.0)),
                Seg::Line(p(144.0, 72.0)),
                Seg::Line(p(72.0, 92.0)),
            ],
            ctm: Matrix([2.0, 0.0, 0.0, 2.0, 72.0, 72.0]),
        }]
    );
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
}

#[test]
fn a_move_replaces_a_pending_move_but_not_a_closed_subpath() {
    let (mut g, _) = backend();
    g.moveto(p(1.0, 1.0)).unwrap();
    g.moveto(p(2.0, 2.0)).unwrap();
    assert_eq!(g.current_point(), Ok(p(2.0, 2.0)));
    g.lineto(p(3.0, 3.0)).unwrap();
    g.closepath().unwrap();
    g.moveto(p(4.0, 4.0)).unwrap();
    g.moveto(p(5.0, 5.0)).unwrap();
    g.closepath().unwrap();
    g.fill().unwrap();
    assert_eq!(
        ops(&g),
        [IrOp::Fill {
            path: vec![
                Seg::Move(p(2.0, 2.0)),
                Seg::Line(p(3.0, 3.0)),
                Seg::Close,
                Seg::Move(p(5.0, 5.0)),
                Seg::Close,
            ],
            rule: FillRule::NonZero,
        }]
    );
    let (mut g, _) = backend();
    g.gsave().unwrap();
    g.moveto(p(1.0, 1.0)).unwrap();
    g.gsave().unwrap();
    g.moveto(p(2.0, 2.0)).unwrap();
    g.grestore().unwrap();
    assert_eq!(
        g.current_point(),
        Ok(p(1.0, 1.0)),
        "the saved path is untouched"
    );
}

#[test]
fn current_point_and_bbox_answer_in_user_space() {
    let (mut g, _) = backend();
    g.concat(Matrix::translation(10.0, 20.0)).unwrap();
    g.moveto(p(1.0, 2.0)).unwrap();
    assert_eq!(g.current_point(), Ok(p(1.0, 2.0)));
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    assert_eq!(g.current_point(), Ok(p(0.5, 1.0)));
    g.lineto(p(5.0, -1.0)).unwrap();
    assert_eq!(g.path_bbox(), Ok(Bounds::new(0.5, -1.0, 5.0, 1.0)));
    g.closepath().unwrap();
    assert_eq!(g.current_point(), Ok(p(0.5, 1.0)));
    g.newpath().unwrap();
    assert_eq!(g.path_bbox(), Err(VmError::NoCurrentPoint));
    assert_eq!(g.lineto(p(0.0, 0.0)), Err(VmError::NoCurrentPoint));
    assert_eq!(
        g.curveto(p(0.0, 0.0), p(0.0, 0.0), p(0.0, 0.0)),
        Err(VmError::NoCurrentPoint)
    );
}

#[test]
fn arcs_become_curves_with_a_leading_line_or_move() {
    let (mut g, _) = backend();
    g.arc(p(0.0, 0.0), 10.0, 0.0, 90.0).unwrap();
    g.fill().unwrap();
    let recorded = ops(&g);
    let [IrOp::Fill { path, .. }] = recorded.as_slice() else {
        panic!("one fill");
    };
    assert_eq!(path.len(), 2);
    assert_eq!(path[0], Seg::Move(p(10.0, 0.0)));
    let Seg::Curve(c1, c2, end) = path[1] else {
        panic!("a curve");
    };
    assert!(close(c1, p(10.0, 5.5228)));
    assert!(close(c2, p(5.5228, 10.0)));
    assert!(close(end, p(0.0, 10.0)));

    let (mut g, _) = backend();
    g.moveto(p(0.0, 0.0)).unwrap();
    g.arc(p(0.0, 0.0), 10.0, 0.0, 360.0).unwrap();
    assert!(close(g.current_point().unwrap(), p(10.0, 0.0)));
    g.fill().unwrap();
    let recorded = ops(&g);
    let [IrOp::Fill { path, .. }] = recorded.as_slice() else {
        panic!("one fill");
    };
    assert_eq!(path.len(), 6);
    assert_eq!(path[1], Seg::Line(p(10.0, 0.0)));
    assert!(matches!(
        path[2..],
        [
            Seg::Curve(..),
            Seg::Curve(..),
            Seg::Curve(..),
            Seg::Curve(..)
        ]
    ));

    // arcn sweeps the other way and the CTM applies to control points.
    let (mut g, _) = backend();
    g.concat(Matrix::scaling(2.0, 1.0)).unwrap();
    g.arcn(p(0.0, 0.0), 10.0, 90.0, 0.0).unwrap();
    g.fill().unwrap();
    let recorded = ops(&g);
    let [IrOp::Fill { path, .. }] = recorded.as_slice() else {
        panic!("one fill");
    };
    assert_eq!(path[0], Seg::Move(p(0.0, 10.0)));
    let Seg::Curve(c1, _, end) = path[1] else {
        panic!("a curve");
    };
    assert!(close(c1, p(11.0457, 10.0)));
    assert!(close(end, p(20.0, 0.0)));
}

#[test]
fn arcto_rounds_a_corner_and_reports_tangents() {
    let (mut g, _) = backend();
    g.moveto(p(0.0, 0.0)).unwrap();
    let (t1, t2) = g.arcto(p(100.0, 0.0), p(100.0, 100.0), 10.0).unwrap();
    assert!(close(t1, p(90.0, 0.0)));
    assert!(close(t2, p(100.0, 10.0)));
    assert!(close(g.current_point().unwrap(), p(100.0, 10.0)));
    g.stroke().unwrap();
    let recorded = ops(&g);
    let [IrOp::Stroke { path, .. }] = recorded.as_slice() else {
        panic!("one stroke");
    };
    assert_eq!(path.len(), 3);
    assert!(matches!(path[1], Seg::Line(q) if close(q, p(90.0, 0.0))));
    assert!(matches!(path[2], Seg::Curve(_, _, q) if close(q, p(100.0, 10.0))));

    // Collinear points degrade to a line; a negative radius is an error;
    // no current point is an error.
    let (mut g, _) = backend();
    g.moveto(p(0.0, 0.0)).unwrap();
    assert_eq!(
        g.arcto(p(50.0, 0.0), p(100.0, 0.0), 10.0),
        Ok((p(50.0, 0.0), p(50.0, 0.0)))
    );
    assert_eq!(g.current_point(), Ok(p(50.0, 0.0)));
    assert_eq!(
        g.arcto(p(60.0, 0.0), p(60.0, 60.0), -1.0),
        Err(VmError::UndefinedResult)
    );
    let (mut g, _) = backend();
    assert_eq!(
        g.arcto(p(1.0, 0.0), p(1.0, 1.0), 1.0),
        Err(VmError::NoCurrentPoint)
    );
}

#[test]
fn rect_operators_leave_the_current_path_alone() {
    let (mut g, _) = backend();
    g.moveto(p(1.0, 1.0)).unwrap();
    let rects = [Rect {
        x: 10.0,
        y: 20.0,
        width: 30.0,
        height: 40.0,
    }];
    g.rectfill(&rects).unwrap();
    g.rectstroke(&rects).unwrap();
    assert_eq!(g.current_point(), Ok(p(1.0, 1.0)));
    let expected = vec![
        Seg::Move(p(10.0, 20.0)),
        Seg::Line(p(40.0, 20.0)),
        Seg::Line(p(40.0, 60.0)),
        Seg::Line(p(10.0, 60.0)),
        Seg::Close,
    ];
    assert_eq!(
        ops(&g),
        [
            IrOp::Fill {
                path: expected.clone(),
                rule: FillRule::NonZero
            },
            IrOp::Stroke {
                path: expected.clone(),
                ctm: Matrix::IDENTITY
            },
        ]
    );
    g.rectclip(&rects).unwrap();
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
    assert_eq!(g.state().clip.len(), 1);
    assert_eq!(*g.state().clip[0].path, expected);
}

// --- emission ------------------------------------------------------------------------

#[test]
fn settings_are_emitted_once_and_only_when_needed() {
    let (mut g, _) = backend();
    g.set_line_width(2.0).unwrap();
    g.set_line_cap(LineCap::Round).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.stroke().unwrap();
    g.set_line_width(2.0).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.stroke().unwrap();
    let kinds: Vec<String> = ops(&g)
        .iter()
        .map(|o| {
            format!("{o:?}")
                .split(['(', ' '])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds, ["Fill", "LineWidth", "LineCap", "Stroke", "Stroke"]);
}

#[test]
fn colour_space_changes_intern_resources_and_reset_colour() {
    let (mut g, _) = backend();
    let spot = SpaceSpec::Separation {
        name: b"Spot".to_vec(),
        alternate: Box::new(SpaceSpec::DeviceCMYK),
        tint_source: b"{dup 0 0 0}".to_vec(),
    };
    g.set_color_space(&spot).unwrap();
    assert_eq!(g.current_color(), vec![1.0]);
    g.set_color(&[0.6]).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    // Setting the same space and colour again emits nothing new.
    g.set_color_space(&spot.clone()).unwrap();
    g.set_color(&[0.6]).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    // The initial colour of a space needs no `sc`.
    g.set_color_space(&SpaceSpec::DeviceCMYK).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    g.set_color_space(&spot).unwrap();
    g.set_color(&[1.5]).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    let emitted: Vec<IrOp> = ops(&g)
        .into_iter()
        .filter(|o| !matches!(o, IrOp::Fill { .. }))
        .collect();
    assert_eq!(
        emitted,
        [
            IrOp::SetColorSpace(SpaceRef(0)),
            IrOp::SetColor(vec![0.6]),
            IrOp::SetColorSpace(SpaceRef(1)),
            IrOp::SetColorSpace(SpaceRef(0)),
        ]
    );
    assert_eq!(g.set_color(&[0.1, 0.2]), Err(VmError::RangeCheck));
    g.showpage().unwrap();
    let pages = g.sink().borrow();
    assert_eq!(
        pages[0].resources.color_spaces,
        [spot, SpaceSpec::DeviceCMYK]
    );
}

#[test]
fn gsave_grestore_without_paint_emits_nothing() {
    let (mut g, _) = backend();
    for _ in 0..50 {
        g.gsave().unwrap();
        g.set_line_width(7.0).unwrap();
        g.grestore().unwrap();
    }
    assert_eq!(g.gstate_depth(), 0);
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    g.fill().unwrap();
    assert_eq!(ops(&g).len(), 1);
    g.grestore().unwrap();
    assert_eq!(g.gstate_depth(), 0);
}

#[test]
fn clips_open_and_close_lazily_around_paints() {
    let (mut g, _) = backend();
    g.gsave().unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    }])
    .unwrap();
    g.set_line_width(3.0).unwrap();
    line(&mut g, p(0.0, 0.0), p(5.0, 5.0));
    g.stroke().unwrap();
    g.grestore().unwrap();
    // Nothing yet: the restore is deferred to the next paint.
    assert!(!ops(&g).contains(&IrOp::Restore));
    line(&mut g, p(0.0, 0.0), p(5.0, 5.0));
    g.stroke().unwrap();
    let kinds: Vec<String> = ops(&g)
        .iter()
        .map(|o| {
            format!("{o:?}")
                .split(['(', ' '])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    // The line width set inside the clip does not survive the `Q`, so
    // the outer stroke (width 1) needs no setting.
    assert_eq!(
        kinds,
        ["Save", "Clip", "LineWidth", "Stroke", "Restore", "Stroke"]
    );

    // A clip that nothing paints under leaves no trace, and initclip
    // ends an open one.
    let (mut g, _) = backend();
    g.gsave().unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }])
    .unwrap();
    g.grestore().unwrap();
    line(&mut g, p(0.0, 0.0), p(5.0, 5.0));
    g.eoclip().unwrap();
    g.fill().unwrap();
    g.initclip().unwrap();
    line(&mut g, p(0.0, 0.0), p(5.0, 5.0));
    g.fill().unwrap();
    g.showpage().unwrap();
    let pages = g.sink().borrow();
    let kinds: Vec<String> = pages[0]
        .ops
        .iter()
        .map(|o| {
            format!("{:?}", o.op)
                .split(['(', ' '])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(kinds, ["Save", "Clip", "Fill", "Restore", "Fill"]);
    assert!(matches!(
        pages[0].ops[1].op,
        IrOp::Clip {
            rule: FillRule::EvenOdd,
            ..
        }
    ));
}

#[test]
fn open_clips_are_closed_at_page_end() {
    let (mut g, _) = backend();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }])
    .unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 2.0,
    }])
    .unwrap();
    line(&mut g, p(0.0, 0.0), p(5.0, 5.0));
    g.fill().unwrap();
    g.copypage().unwrap();
    g.showpage().unwrap();
    let pages = g.sink().borrow();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0], pages[1]);
    let tail: Vec<&IrOp> = pages[0].ops.iter().rev().take(2).map(|o| &o.op).collect();
    assert_eq!(tail, [&IrOp::Restore, &IrOp::Restore]);
    assert!(pages[0].ops.iter().filter(|o| o.op == IrOp::Save).count() == 2);
}

#[test]
fn clippath_reports_the_clip_or_the_media_box() {
    let (mut g, _) = backend();
    g.set_media_box(Bounds::new(0.0, 0.0, 100.0, 50.0)).unwrap();
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    let segs = g.clippath().unwrap();
    assert_eq!(segs[0], Seg::Move(p(0.0, 0.0)));
    assert_eq!(segs[2], Seg::Line(p(50.0, 25.0)));
    assert_eq!(g.current_point(), Ok(p(0.0, 0.0)));
    g.newpath().unwrap();
    line(&mut g, p(1.0, 1.0), p(4.0, 1.0));
    g.clip().unwrap();
    // `clip` keeps the current path.
    assert_eq!(g.current_point(), Ok(p(4.0, 1.0)));
    let segs = g.clippath().unwrap();
    assert_eq!(segs, [Seg::Move(p(1.0, 1.0)), Seg::Line(p(4.0, 1.0))]);
}

// --- pages -----------------------------------------------------------------------------

#[test]
fn showpage_delivers_and_reinitializes_keeping_the_media_box() {
    let (mut g, pages) = backend();
    g.set_media_box(Bounds::new(0.0, 0.0, 200.0, 100.0))
        .unwrap();
    g.set_line_width(4.0).unwrap();
    g.concat(Matrix::translation(5.0, 5.0)).unwrap();
    g.gsave().unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    g.stroke().unwrap();
    g.showpage().unwrap();
    assert_eq!(g.gstate_depth(), 1);
    assert_eq!(g.line_width(), 1.0);
    assert_eq!(g.current_matrix(), Matrix::IDENTITY);
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    g.stroke().unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].media_box, Bounds::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(pages[1].media_box, Bounds::new(0.0, 0.0, 200.0, 100.0));
    assert_eq!(pages[0].ops.len(), 2);
    // The second page starts from PDF defaults again.
    assert_eq!(pages[1].ops.len(), 1);
    assert_eq!(g.ops().len(), 0);
}

#[test]
fn erasepage_clears_and_nulldevice_swallows() {
    let (mut g, pages) = backend();
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    g.fill().unwrap();
    g.erasepage().unwrap();
    assert!(g.ops().is_empty());

    g.gsave().unwrap();
    g.concat(Matrix::scaling(3.0, 3.0)).unwrap();
    g.nulldevice().unwrap();
    assert_eq!(g.current_matrix(), Matrix::IDENTITY);
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    assert_eq!(g.current_point(), Ok(p(1.0, 1.0)));
    g.fill().unwrap();
    g.showpage().unwrap();
    g.copypage().unwrap();
    assert!(pages.borrow().is_empty());
    // The page ended all the same: the path is gone and the device stays.
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
    assert!(g.state().null_device);
    g.grestore().unwrap();
    assert!(!g.state().null_device);
    line(&mut g, p(0.0, 0.0), p(1.0, 1.0));
    g.fill().unwrap();
    g.showpage().unwrap();
    assert_eq!(pages.borrow().len(), 1);
    assert_eq!(pages.borrow()[0].ops.len(), 1);
}

#[test]
fn images_carry_a_unit_square_matrix_and_masks_take_the_colour() {
    let (mut g, pages) = backend();
    g.concat(Matrix::translation(100.0, 200.0)).unwrap();
    g.concat(Matrix::scaling(50.0, 20.0)).unwrap();
    let spec = ImageSpec {
        width: 4,
        height: 2,
        bits_per_component: 8,
        color_space: Some(SpaceSpec::DeviceRGB),
        decode: vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0],
        matrix: Matrix([4.0, 0.0, 0.0, -2.0, 0.0, 2.0]),
        interpolate: false,
        is_mask: false,
        encoded: None,
    };
    g.image(&spec, &[0; 24]).unwrap();
    let mask = ImageSpec {
        width: 2,
        height: 1,
        bits_per_component: 1,
        color_space: None,
        decode: vec![1.0, 0.0],
        matrix: Matrix([2.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
        interpolate: false,
        is_mask: true,
        encoded: None,
    };
    g.set_color_space(&SpaceSpec::DeviceRGB).unwrap();
    g.set_color(&[1.0, 0.0, 0.0]).unwrap();
    g.imagemask(&mask, &[0x80]).unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    let ops: Vec<&IrOp> = pages[0].ops.iter().map(|o| &o.op).collect();
    let IrOp::Image { matrix, .. } = ops[0] else {
        panic!("an image");
    };
    assert_eq!(*matrix, Matrix([50.0, 0.0, 0.0, 20.0, 100.0, 200.0]));
    assert!(matches!(ops[1], IrOp::SetColorSpace(SpaceRef(0))));
    assert_eq!(*ops[2], IrOp::SetColor(vec![1.0, 0.0, 0.0]));
    let IrOp::Image { matrix, .. } = ops[3] else {
        panic!("a mask");
    };
    // An unflipped image matrix puts the first row at the bottom.
    assert_eq!(*matrix, Matrix([50.0, 0.0, 0.0, -20.0, 100.0, 220.0]));
    assert_eq!(pages[0].resources.images.len(), 2);
    assert_eq!(pages[0].resources.images[0].color_space, Some(SpaceRef(0)));
    assert_eq!(pages[0].resources.images[1].color_space, None);
    assert_eq!(pages[0].resources.color_spaces, [SpaceSpec::DeviceRGB]);
}

#[test]
fn parameters_are_validated_and_flatness_clamped() {
    let (mut g, _) = backend();
    assert_eq!(g.set_miter_limit(0.5), Err(VmError::RangeCheck));
    assert_eq!(g.set_dash(&[0.0], 0.0), Err(VmError::RangeCheck));
    assert_eq!(g.set_dash(&[1.0, -1.0], 0.0), Err(VmError::RangeCheck));
    assert_eq!(g.set_dash(&[], 0.0), Ok(()));
    g.set_flatness(0.01).unwrap();
    assert_eq!(g.flatness(), 0.2);
    g.set_flatness(1000.0).unwrap();
    assert_eq!(g.flatness(), 100.0);
    assert_eq!(g.set_line_width(f32::NAN), Err(VmError::RangeCheck));
    assert_eq!(
        g.arc(p(0.0, 0.0), f32::INFINITY, 0.0, 1.0),
        Err(VmError::RangeCheck)
    );
    g.initgraphics().unwrap();
    assert_eq!(g.flatness(), 1.0);
    assert_eq!(g.default_matrix(), Matrix::IDENTITY);
    assert_eq!(g.current_color_space(), SpaceSpec::DeviceGray);
}

// --- text ------------------------------------------------------------------------------

use efterscript_fonts::{ResidentFace, STANDARD_ENCODING};
use efterscript_graphics::{FontIndex, FontSpec, GlyphProc, glyph_names};
use efterscript_vm::{FontInfo, FontRef, FontSource, Glyph};

fn standard_names() -> Vec<Option<Vec<u8>>> {
    STANDARD_ENCODING
        .iter()
        .map(|name| name.map(|n| n.as_bytes().to_vec()))
        .collect()
}

fn helvetica() -> FontInfo {
    FontInfo {
        source: FontSource::Resident(ResidentFace::Helvetica),
        encoding: standard_names(),
    }
}

fn square_font(family: u32) -> FontInfo {
    FontInfo {
        source: FontSource::Type3 {
            family,
            font_matrix: Matrix::scaling(0.001, 0.001),
            font_bbox: Bounds::new(0.0, 0.0, 1000.0, 1000.0),
        },
        encoding: standard_names(),
    }
}

fn font(instance: u32, size: f32) -> FontRef {
    FontRef {
        instance,
        matrix: Matrix::scaling(0.001 * size, 0.001 * size),
    }
}

fn glyph(code: u8, dx: f32) -> Glyph {
    Glyph::simple(code, dx, 0.0)
}

/// Runs a square glyph procedure of `size` glyph units through the
/// capture, declared with `setcachedevice` unless `charwidth`.
fn capture_square(g: &mut Graphics<Pages>, font: FontRef, name: &[u8], measure: bool) {
    g.gsave().unwrap();
    let origin = g.current_point().unwrap();
    let ctm = font
        .matrix
        .then(Matrix::translation(origin.x, origin.y))
        .then(g.current_matrix());
    g.set_matrix(ctm).unwrap();
    g.newpath().unwrap();
    g.begin_glyph(font, name[0], name, measure).unwrap();
    g.rectfill(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 750.0,
        height: 750.0,
    }])
    .unwrap();
    g.end_glyph((1000.0, 0.0), Some(Bounds::new(0.0, 0.0, 750.0, 750.0)))
        .unwrap();
    g.grestore().unwrap();
}

#[test]
fn a_run_is_one_text_op_with_colour_before_it_and_advances_the_point() {
    let (mut g, _) = backend();
    g.define_font(0, &helvetica()).unwrap();
    assert_eq!(g.show(&[glyph(72, 722.0)]), Err(VmError::NoCurrentPoint));
    g.moveto(p(100.0, 700.0)).unwrap();
    assert_eq!(g.show(&[glyph(72, 722.0)]), Err(VmError::InvalidFont));
    g.set_font(Some(font(0, 12.0))).unwrap();
    g.set_color(&[0.5]).unwrap();
    g.show(&[glyph(72, 722.0), glyph(105, 222.0)]).unwrap();
    let after = g.current_point().unwrap();
    assert!(close(after, p(100.0 + 0.944 * 12.0, 700.0)));
    let recorded = ops(&g);
    assert_eq!(recorded[0], IrOp::SetColor(vec![0.5]));
    let IrOp::Text {
        font,
        matrix,
        glyphs,
        ..
    } = &recorded[1]
    else {
        panic!("a text op, got {:?}", recorded[1]);
    };
    assert_eq!(*font, FontIndex(0));
    assert!(
        matrix
            .0
            .iter()
            .zip([0.012, 0.0, 0.0, 0.012, 100.0, 700.0])
            .all(|(a, b)| (a - b).abs() < 1e-6)
    );
    assert_eq!(glyphs, &[glyph(72, 722.0), glyph(105, 222.0)]);
    assert_eq!(recorded.len(), 2);
    // A second run in the same colour needs no setting; an empty run
    // records nothing but is not an error.
    g.show(&[]).unwrap();
    g.show(&[glyph(72, 722.0)]).unwrap();
    assert_eq!(ops(&g).len(), 3);
    g.showpage().unwrap();
    let pages = g.sink().borrow();
    assert_eq!(
        pages[0].resources.fonts,
        [FontSpec::Resident {
            base: ResidentFace::Helvetica,
            encoding: glyph_names(&standard_names()),
        }]
    );
    assert!(pages[0].dump().contains(
        "font 0 Helvetica\nops:\nsc 0.5\ntext 0 0.012 0 0 0.012 100 700 (Hi) 722 0 222 0\n"
    ));
}

#[test]
fn resident_fonts_intern_by_base_and_encoding_not_by_instance() {
    let (mut g, pages) = backend();
    g.define_font(0, &helvetica()).unwrap();
    g.define_font(1, &helvetica()).unwrap();
    let mut reencoded = helvetica();
    reencoded.encoding[65] = Some(b"W".to_vec());
    reencoded.encoding[66] = None;
    g.define_font(2, &reencoded).unwrap();
    g.define_font(
        3,
        &FontInfo {
            source: FontSource::Resident(ResidentFace::Symbol),
            encoding: ResidentFace::Symbol
                .builtin_encoding()
                .iter()
                .map(|n| Some(n.unwrap_or(".notdef").as_bytes().to_vec()))
                .collect(),
        },
    )
    .unwrap();
    for (instance, size) in [(0, 12.0), (1, 24.0), (2, 12.0), (3, 12.0), (0, 8.0)] {
        g.moveto(p(0.0, 0.0)).unwrap();
        g.set_font(Some(font(instance, size))).unwrap();
        g.show(&[glyph(65, 667.0)]).unwrap();
    }
    g.showpage().unwrap();
    let pages = pages.borrow();
    let fonts: Vec<FontIndex> = pages[0]
        .ops
        .iter()
        .filter_map(|o| match &o.op {
            IrOp::Text { font, .. } => Some(*font),
            _ => None,
        })
        .collect();
    assert_eq!(
        fonts,
        [
            FontIndex(0),
            FontIndex(0),
            FontIndex(1),
            FontIndex(2),
            FontIndex(0)
        ]
    );
    assert_eq!(pages[0].resources.fonts.len(), 3);
    let dump = pages[0].dump();
    assert!(dump.contains(
        "font 0 Helvetica\nfont 1 Helvetica diff=[65 /W 66 /.notdef]\nfont 2 Symbol\nops:\n"
    ));
    // A redefined instance starts over.
    assert!(
        pages[0].resources.fonts[2]
            .glyph_name(97)
            .is_some_and(|n| n == b"alpha")
    );
}

#[test]
fn glyphs_are_captured_in_glyph_space_and_shown_as_a_run() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    g.set_font(Some(font(0, 20.0))).unwrap();
    g.moveto(p(10.0, 10.0)).unwrap();
    g.set_color(&[0.5]).unwrap();
    capture_square(&mut g, font(0, 20.0), b"a", false);
    capture_square(&mut g, font(0, 20.0), b"a", false);
    g.show(&[glyph(97, 1000.0), glyph(97, 1000.0)]).unwrap();
    assert_eq!(g.gstate_depth(), 0);
    g.showpage().unwrap();
    let pages = pages.borrow();
    let page = &pages[0];
    let square = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(750.0, 0.0)),
        Seg::Line(p(750.0, 750.0)),
        Seg::Line(p(0.0, 750.0)),
        Seg::Close,
    ];
    let FontSpec::Type3 {
        font_matrix,
        glyphs,
        ..
    } = &page.resources.fonts[0]
    else {
        panic!("a Type 3 resource");
    };
    assert_eq!(*font_matrix, Matrix::scaling(0.001, 0.001));
    assert_eq!(glyphs.len(), 1);
    let proc_ = &glyphs[b"a".as_slice()];
    assert_eq!(proc_.width, (1000.0, 0.0));
    assert_eq!(proc_.bbox, Some(Bounds::new(0.0, 0.0, 750.0, 750.0)));
    // The inherited colour is not part of the procedure.
    assert_eq!(proc_.ops.len(), 1);
    let IrOp::Fill { path, .. } = &proc_.ops[0].op else {
        panic!("a fill");
    };
    assert!(path.iter().zip(&square).all(|(a, b)| match (a, b) {
        (Seg::Move(a), Seg::Move(b)) | (Seg::Line(a), Seg::Line(b)) => close(*a, *b),
        (Seg::Close, Seg::Close) => true,
        _ => false,
    }));
    let recorded: Vec<&IrOp> = page.ops.iter().map(|o| &o.op).collect();
    assert_eq!(*recorded[0], IrOp::SetColor(vec![0.5]));
    let IrOp::Text { matrix, glyphs, .. } = recorded[1] else {
        panic!("a text op");
    };
    assert!(
        matrix
            .0
            .iter()
            .zip([0.04, 0.0, 0.0, 0.04, 20.0, 20.0])
            .all(|(a, b)| (a - b).abs() < 1e-5)
    );
    assert_eq!(glyphs.len(), 2);
    assert_eq!(recorded.len(), 2);
    assert!(page.dump().contains(
        "font 0 type3 0.001 0 0 0.001 0 0 bbox=[0 0 1000 1000] enc=[97 /a]\n\
         glyph /a 1000 0 [0 0 750 750] {\n  m 0 0\n  l 750 0\n  l 750 750\n  l 0 750\n  h\n  f\n}\nops:\n"
    ));
}

#[test]
fn embedded_fonts_intern_by_snapshot_and_encoding_and_dump_without_bytes() {
    use efterscript_fonts::ProgramKind;
    use efterscript_fonts::testing::corpus_type1;
    use std::rc::Rc;
    let program = Rc::new(corpus_type1().program());
    let mut names: Vec<Option<Vec<u8>>> = vec![None; 256];
    names[97] = Some(b"a".to_vec());
    names[233] = Some(b"eacute".to_vec());
    let embedded = |encoding: Vec<Option<Vec<u8>>>| FontInfo {
        source: FontSource::Embedded {
            family: 7,
            kind: ProgramKind::Type1,
            program: program.clone(),
            font_matrix: Matrix::scaling(0.001, 0.001),
            font_name: b"Syn".to_vec(),
        },
        encoding,
    };
    let (mut g, pages) = backend();
    g.define_font(0, &embedded(names.clone())).unwrap();
    g.define_font(1, &embedded(names.clone())).unwrap();
    let mut other = names.clone();
    other[98] = Some(b"e".to_vec());
    g.define_font(2, &embedded(other)).unwrap();
    g.moveto(p(100.0, 100.0)).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.show(&[glyph(97, 600.0)]).unwrap();
    g.set_font(Some(font(1, 20.0))).unwrap();
    g.show(&[glyph(233, 500.0)]).unwrap();
    g.set_font(Some(font(2, 10.0))).unwrap();
    g.show(&[glyph(98, 500.0)]).unwrap();
    assert!(close(
        g.current_point().unwrap(),
        p(100.0 + 6.0 + 10.0 + 5.0, 100.0)
    ));
    g.showpage().unwrap();
    let pages = pages.borrow();
    let fonts = &pages[0].resources.fonts;
    assert_eq!(
        fonts.len(),
        2,
        "two instances of one snapshot share a resource"
    );
    let FontSpec::Embedded {
        family,
        kind,
        font_name,
        program: shared,
        ..
    } = &fonts[0]
    else {
        panic!("an embedded resource, got {:?}", fonts[0]);
    };
    assert_eq!((*family, *kind), (7, ProgramKind::Type1));
    assert_eq!(font_name, b"Syn");
    assert!(Rc::ptr_eq(&shared.0, &program));
    assert_eq!(fonts[0].width(97), (600.0, 0.0));
    assert_eq!(fonts[1].width(98), (500.0, 0.0));
    let dump = pages[0].dump();
    assert!(
        dump.contains(
            "font 0 embedded type1 Syn glyphs=6 enc=[97 /a 233 /eacute]\n\
         font 1 embedded type1 Syn glyphs=6 enc=[97 /a 98 /e 233 /eacute]\nops:\n\
         text 0 0.01 0 0 0.01 100 100 (a) 600 0\n\
         text 0 0.02 0 0 0.02 106 100 (\\351) 500 0\n\
         text 1 0.01 0 0 0.01 116 100 (b) 500 0\n"
        ),
        "{dump}"
    );
    assert!(!dump.contains("eexec") && !dump.contains("RD"));
}

#[test]
fn type3_fonts_intern_by_family_and_encoding() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.define_font(1, &square_font(7)).unwrap();
    let mut other = square_font(7);
    other.encoding[97] = Some(b"b".to_vec());
    g.define_font(2, &other).unwrap();
    g.define_font(3, &square_font(8)).unwrap();
    for instance in 0..4 {
        g.moveto(p(0.0, 0.0)).unwrap();
        g.set_font(Some(font(instance, 10.0))).unwrap();
        capture_square(&mut g, font(instance, 10.0), b"a", false);
        g.show(&[glyph(97, 1000.0)]).unwrap();
    }
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(pages[0].resources.fonts.len(), 3);
    let fonts: Vec<usize> = pages[0]
        .ops
        .iter()
        .filter_map(|o| match &o.op {
            IrOp::Text { font, .. } => Some(font.0),
            _ => None,
        })
        .collect();
    assert_eq!(fonts, [0, 0, 1, 2]);
}

#[test]
fn glyph_matrix_is_the_ctm_at_begin_glyph_of_the_innermost_glyph() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    assert_eq!(g.glyph_matrix(), None);
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    g.set_font(Some(font(0, 20.0))).unwrap();
    g.moveto(p(10.0, 10.0)).unwrap();
    g.gsave().unwrap();
    let outer = font(0, 20.0)
        .matrix
        .then(Matrix::translation(10.0, 10.0))
        .then(g.current_matrix());
    g.set_matrix(outer).unwrap();
    g.begin_glyph(font(0, 20.0), 97, b"a", false).unwrap();
    assert_eq!(g.glyph_matrix(), Some(outer));
    // A change inside the procedure leaves the record alone.
    g.concat(Matrix::scaling(0.5, 0.5)).unwrap();
    assert_eq!(g.glyph_matrix(), Some(outer));
    assert_ne!(g.current_matrix(), outer);
    // A glyph shown from inside the procedure has its own record.
    g.gsave().unwrap();
    let inner = Matrix::translation(3.0, 4.0).then(g.current_matrix());
    g.set_matrix(inner).unwrap();
    g.begin_glyph(font(0, 20.0), 98, b"b", true).unwrap();
    assert_eq!(g.glyph_matrix(), Some(inner));
    g.end_glyph((0.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    assert_eq!(g.glyph_matrix(), Some(outer));
    g.end_glyph((1000.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    assert_eq!(g.glyph_matrix(), None);
    // The outer glyph painted nothing but declared a width: a blank
    // procedure, so the font has an entry for the code.
    g.show(&[glyph(97, 1000.0)]).unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    let FontSpec::Type3 { glyphs, .. } = &pages[0].resources.fonts[0] else {
        panic!("a Type 3 resource");
    };
    assert_eq!(glyphs.len(), 1);
    let blank = &glyphs[b"a".as_slice()];
    assert!(blank.ops.is_empty());
    assert_eq!((blank.width, blank.bbox), ((1000.0, 0.0), None));
    assert!(pages[0].dump().contains("glyph /a 1000 0 {\n}\n"));
}

#[test]
fn measured_and_abandoned_glyphs_store_nothing() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.moveto(p(0.0, 0.0)).unwrap();
    capture_square(&mut g, font(0, 10.0), b"a", true);
    assert!(g.ops().is_empty());
    // An abandoned procedure: the VM ends it with no width and no box.
    g.gsave().unwrap();
    g.begin_glyph(font(0, 10.0), 98, b"b", false).unwrap();
    g.rectfill(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }])
    .unwrap();
    g.end_glyph((0.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    assert!(g.ops().is_empty());
    assert_eq!(g.end_glyph((1.0, 0.0), None), Err(VmError::InvalidAccess));
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert!(pages[0].resources.fonts.is_empty());
    assert!(pages[0].ops.is_empty());
}

#[test]
fn a_glyph_keeps_its_own_settings_clips_and_nested_text_only() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.define_font(1, &helvetica()).unwrap();
    g.set_color_space(&SpaceSpec::DeviceRGB).unwrap();
    g.set_color(&[1.0, 0.0, 0.0]).unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 500.0,
        height: 500.0,
    }])
    .unwrap();
    g.set_line_width(3.0).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.moveto(p(10.0, 10.0)).unwrap();
    // The glyph: a colour of its own in the inherited space, a clip, a
    // stroke, and a nested run in a resident font.
    g.gsave().unwrap();
    let ctm = font(0, 10.0)
        .matrix
        .then(Matrix::translation(10.0, 10.0))
        .then(g.current_matrix());
    g.set_matrix(ctm).unwrap();
    g.newpath().unwrap();
    g.begin_glyph(font(0, 10.0), 97, b"a", false).unwrap();
    g.set_color(&[0.0, 0.0, 1.0]).unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 600.0,
        height: 600.0,
    }])
    .unwrap();
    line(&mut g, p(0.0, 0.0), p(500.0, 500.0));
    g.stroke().unwrap();
    g.set_font(Some(font(1, 400.0))).unwrap();
    g.moveto(p(100.0, 100.0)).unwrap();
    g.show(&[glyph(72, 722.0)]).unwrap();
    g.end_glyph((1000.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    g.show(&[glyph(97, 1000.0)]).unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    let dump = pages[0].dump();
    assert_eq!(
        dump,
        "ir/1\npage 612 792\nresources:\ncs 0 DeviceRGB\nfont 0 Helvetica\n\
         font 1 type3 0.001 0 0 0.001 0 0 bbox=[0 0 1000 1000] enc=[97 /a]\n\
         glyph /a 1000 0 {\n  q\n  m 0 0\n  l 600 0\n  l 600 600\n  l 0 600\n  h\n  W n\n\
         \x20 cs 0\n  sc 0 0 1\n  m 0 0\n  l 500 500\n  S\n\
         \x20 text 0 0.4 0 0 0.4 100 100 (H) 722 0\n  Q\n}\n\
         ops:\nq\nm 0 0\nl 500 0\nl 500 500\nl 0 500\nh\nW n\ncs 0\nsc 1 0 0\nw 3\n\
         text 1 0.01 0 0 0.01 10 10 (a) 1000 0\nQ\n"
    );
}

#[test]
fn page_operations_are_refused_while_capturing() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.moveto(p(0.0, 0.0)).unwrap();
    g.gsave().unwrap();
    g.begin_glyph(font(0, 10.0), 97, b"a", false).unwrap();
    assert_eq!(g.showpage(), Err(VmError::InvalidAccess));
    assert_eq!(g.copypage(), Err(VmError::InvalidAccess));
    assert_eq!(g.erasepage(), Err(VmError::InvalidAccess));
    assert_eq!(
        g.set_media_box(Bounds::new(0.0, 0.0, 1.0, 1.0)),
        Err(VmError::InvalidAccess)
    );
    g.end_glyph((0.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    g.showpage().unwrap();
    assert_eq!(pages.borrow().len(), 1);
}

#[test]
fn a_glyph_name_keeps_its_first_procedure() {
    let (mut g, pages) = backend();
    g.define_font(0, &square_font(7)).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.moveto(p(0.0, 0.0)).unwrap();
    capture_square(&mut g, font(0, 10.0), b"a", false);
    g.gsave().unwrap();
    g.begin_glyph(font(0, 10.0), 97, b"a", false).unwrap();
    g.end_glyph((500.0, 0.0), None).unwrap();
    g.grestore().unwrap();
    g.show(&[glyph(97, 1000.0), glyph(97, 500.0)]).unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    let FontSpec::Type3 { glyphs, .. } = &pages[0].resources.fonts[0] else {
        panic!("a Type 3 resource");
    };
    assert_eq!(glyphs.len(), 1);
    assert_eq!(glyphs[b"a".as_slice()].width, (1000.0, 0.0));
    assert_eq!(pages[0].resources.fonts[0].width(97), (1000.0, 0.0));
    assert_eq!(pages[0].resources.fonts[0].width(0), (0.0, 0.0));
    let _ = GlyphProc {
        ops: Vec::new(),
        width: (0.0, 0.0),
        bbox: None,
    };
}

#[test]
fn composite_fonts_intern_by_cmap_and_descendant_and_record_their_cids() {
    use efterscript_fonts::ProgramKind;
    use efterscript_fonts::cmap::CMapBuilder;
    use efterscript_fonts::testing::corpus_cid_cff;
    use std::rc::Rc;
    let program = Rc::new(corpus_cid_cff().program().unwrap());
    let descendant = FontSource::Embedded {
        family: 9,
        kind: ProgramKind::Cff,
        program: program.clone(),
        font_matrix: Matrix::scaling(0.001, 0.001),
        font_name: b"SynCID".to_vec(),
    };
    let cmap = |name: &[u8], wmode: u8| {
        let mut builder = CMapBuilder::new();
        builder
            .name(name)
            .wmode(wmode)
            .codespace(&[0, 0], &[0xff, 0xff])
            .unwrap()
            .cid_range(&[0, 0], &[0xff, 0xff], 0)
            .unwrap();
        Rc::new(builder.build())
    };
    let composite = |family: u32, name: &[u8], wmode: u8| FontInfo {
        source: FontSource::Composite {
            family,
            cmap_name: name.to_vec(),
            wmode,
            unicode_based: false,
            cmap: cmap(name, wmode),
            descendant: Box::new(descendant.clone()),
        },
        encoding: vec![None; 256],
    };
    let cid = |cid: u16, dx: f32, dy: f32| Glyph {
        code: u32::from(cid),
        len: 2,
        cid,
        dx,
        dy,
    };
    let (mut g, pages) = backend();
    g.define_font(0, &composite(20, b"Identity-H", 0)).unwrap();
    g.define_font(1, &composite(20, b"Identity-H", 0)).unwrap();
    g.define_font(2, &composite(21, b"Identity-V", 1)).unwrap();
    g.moveto(p(100.0, 100.0)).unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.show(&[cid(1, 500.0, 0.0), cid(2, 700.0, 0.0)]).unwrap();
    assert!(close(g.current_point().unwrap(), p(112.0, 100.0)));
    g.set_font(Some(font(1, 20.0))).unwrap();
    g.show(&[cid(34, 500.0, 0.0)]).unwrap();
    g.moveto(p(50.0, 200.0)).unwrap();
    g.set_font(Some(font(2, 10.0))).unwrap();
    g.show(&[cid(1, 0.0, -1000.0)]).unwrap();
    assert!(close(g.current_point().unwrap(), p(50.0, 190.0)));
    g.showpage().unwrap();
    let pages = pages.borrow();
    let fonts = &pages[0].resources.fonts;
    assert_eq!(
        fonts.len(),
        2,
        "two instances of one CMap and descendant share"
    );
    let FontSpec::Composite {
        cmap_name,
        wmode,
        descendant: inner,
        cid_to_code,
        ..
    } = &fonts[0]
    else {
        panic!("a composite resource, got {:?}", fonts[0]);
    };
    assert_eq!(cmap_name, b"Identity-H");
    assert_eq!(*wmode, 0);
    assert!(matches!(
        &**inner,
        FontSpec::Embedded { kind: ProgramKind::Cff, font_name, program: shared, .. }
            if font_name == b"SynCID" && Rc::ptr_eq(&shared.0, &program)
    ));
    assert_eq!(
        cid_to_code
            .iter()
            .map(|(c, v)| (*c, *v))
            .collect::<Vec<_>>(),
        [(1, (1, 2)), (2, (2, 2)), (34, (34, 2))]
    );
    assert_eq!(fonts[0].cid_width(2), (700.0, 0.0));
    assert_eq!(fonts[0].glyph_width(&cid(34, 0.0, 0.0)), (500.0, 0.0));
    assert_eq!(fonts[0].cid_width(99), (0.0, 0.0), "no glyph, no width");
    assert!(fonts[0].same_font(&fonts[0]));
    assert!(!fonts[0].same_font(&fonts[1]));
    assert_eq!(
        pages[0].dump(),
        "ir/1\npage 612 792\nresources:\n\
         font 0 composite Identity-H wmode=0 cff SynCID glyphs=6\n\
         font 1 composite Identity-V wmode=1 cff SynCID glyphs=6\n\
         ops:\n\
         text 0 0.01 0 0 0.01 100 100 <00010002> 500 0 700 0\n\
         text 0 0.02 0 0 0.02 112 100 <0022> 500 0\n\
         text 1 0.01 0 0 0.01 50 200 <0001> 0 -1000 wmode=1\n"
    );
}

#[test]
fn a_simple_descendant_is_recorded_as_its_own_font() {
    use efterscript_fonts::cmap::CMapBuilder;
    use std::rc::Rc;
    let mut builder = CMapBuilder::new();
    builder
        .name(b"Identity-H")
        .codespace(&[0, 0], &[0xff, 0xff])
        .unwrap()
        .cid_range(&[0, 0], &[0xff, 0xff], 0)
        .unwrap();
    let info = FontInfo {
        source: FontSource::Composite {
            family: 30,
            cmap_name: b"Identity-H".to_vec(),
            wmode: 0,
            unicode_based: false,
            cmap: Rc::new(builder.build()),
            descendant: Box::new(FontSource::Resident(ResidentFace::Helvetica)),
        },
        encoding: standard_names(),
    };
    let (mut g, pages) = backend();
    g.define_font(0, &info).unwrap();
    g.define_font(1, &helvetica()).unwrap();
    g.moveto(p(100.0, 700.0)).unwrap();
    g.set_font(Some(font(0, 12.0))).unwrap();
    g.show(&[Glyph {
        code: 0x48,
        len: 2,
        cid: 0x48,
        dx: 722.0,
        dy: 0.0,
    }])
    .unwrap();
    g.set_font(Some(font(1, 12.0))).unwrap();
    g.show(&[glyph(105, 222.0)]).unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(
        pages[0].resources.fonts.len(),
        1,
        "Helvetica with its encoding"
    );
    assert!(pages[0].dump().contains(
        "font 0 Helvetica\nops:\n\
         text 0 0.012 0 0 0.012 100 700 <0048> 722 0\n\
         text 0 0.012 0 0 0.012 108.664 700 (i) 222 0\n"
    ));
}

// --- patterns and forms ---------------------------------------------------------

fn pattern(id: u64, matrix: Matrix, paint_type: u8) -> PatternInfo {
    PatternInfo {
        id,
        matrix,
        kind: PatternKind::Tiling {
            bbox: Bounds::new(0.0, 0.0, 10.0, 10.0),
            xstep: 10.0,
            ystep: 10.0,
            paint_type,
            tiling_type: 1,
        },
    }
}

fn form(id: u64, matrix: Matrix) -> FormInfo {
    FormInfo {
        id,
        bbox: Bounds::new(0.0, 0.0, 100.0, 100.0),
        matrix,
    }
}

fn square(g: &mut Graphics<Pages>, x: f32, y: f32, size: f32) {
    g.rectfill(&[Rect {
        x,
        y,
        width: size,
        height: size,
    }])
    .unwrap();
}

/// Runs a cell that fills a five-unit square, as the VM would: the
/// backend saves the state at `begin`, the caller restores it after.
fn capture_cell(g: &mut Graphics<Pages>, info: &PatternInfo) -> bool {
    let depth = g.gstate_depth();
    if !g.begin_pattern_cell(info).unwrap() {
        return false;
    }
    square(g, 0.0, 0.0, 5.0);
    g.end_pattern_cell().unwrap();
    g.grestore_to(depth).unwrap();
    true
}

const CELL: &str = "  q\n  m 0 0\n  l 10 0\n  l 10 10\n  l 0 10\n  h\n  W n\n  m 0 0\n  l 5 0\n  l 5 5\n  l 0 5\n  h\n  f\n  Q\n}\n";

#[test]
fn a_cell_is_captured_once_per_page_in_pattern_space_and_names_its_fills() {
    let (mut g, pages) = backend();
    let info = pattern(7, Matrix([2.0, 0.0, 0.0, 2.0, 30.0, 50.0]), 1);
    g.concat(Matrix::scaling(3.0, 3.0)).unwrap();
    g.set_line_width(4.0).unwrap();
    g.set_color_space(&SpaceSpec::Pattern {
        base: Some(Box::new(SpaceSpec::DeviceGray)),
    })
    .unwrap();
    g.set_pattern(&info, &[]).unwrap();
    assert_eq!(g.current_pattern(), Some(info.clone()));
    assert!(capture_cell(&mut g, &info));
    // The cell ran in pattern space from the initial state; the page's
    // scale and line width are back afterwards.
    assert_eq!(g.current_matrix(), Matrix::scaling(3.0, 3.0));
    assert_eq!(g.line_width(), 4.0);
    assert_eq!(g.current_pattern(), Some(info.clone()));
    square(&mut g, 0.0, 0.0, 10.0);
    assert!(!capture_cell(&mut g, &info), "the page holds the cell");
    square(&mut g, 20.0, 0.0, 10.0);
    g.showpage().unwrap();
    assert!(capture_cell(&mut g, &info), "a new page captures again");
    g.erasepage().unwrap();
    assert!(capture_cell(&mut g, &info), "and so does an erased one");
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(
        pages[0].dump(),
        format!(
            "ir/1\npage 612 792\nresources:\ncs 0 Pattern base=DeviceGray\n\
             pattern 0 matrix 2 0 0 2 30 50 bbox 0 0 10 10 step 10 10 paint 1 tiling 1 {{\n{CELL}\
             ops:\ncs 0\npattern 0\nm 0 0\nl 30 0\nl 30 30\nl 0 30\nh\nf\n\
             m 60 0\nl 90 0\nl 90 30\nl 60 30\nh\nf\n"
        )
    );
    assert_eq!(pages[1].resources.patterns.len(), 1);
}

#[test]
fn the_colour_dedup_key_is_the_pattern_and_its_components() {
    let (mut g, _) = backend();
    let a = pattern(1, Matrix::IDENTITY, 2);
    let b = pattern(2, Matrix::IDENTITY, 2);
    g.set_color_space(&SpaceSpec::Pattern {
        base: Some(Box::new(SpaceSpec::DeviceRGB)),
    })
    .unwrap();
    g.set_pattern(&a, &[1.0, 0.0, 0.0]).unwrap();
    capture_cell(&mut g, &a);
    square(&mut g, 0.0, 0.0, 10.0);
    g.set_pattern(&a, &[1.0, 0.0, 0.0]).unwrap();
    square(&mut g, 0.0, 0.0, 10.0);
    g.set_pattern(&a, &[0.0, 1.0, 0.0]).unwrap();
    square(&mut g, 0.0, 0.0, 10.0);
    g.set_pattern(&b, &[0.0, 1.0, 0.0]).unwrap();
    capture_cell(&mut g, &b);
    square(&mut g, 0.0, 0.0, 10.0);
    assert_eq!(g.set_pattern(&a, &[1.0]), Err(VmError::RangeCheck));
    g.set_color_space(&SpaceSpec::DeviceGray).unwrap();
    assert_eq!(g.current_pattern(), None);
    square(&mut g, 0.0, 0.0, 10.0);
    let colours: Vec<IrOp> = ops(&g)
        .into_iter()
        .filter(|op| {
            matches!(
                op,
                IrOp::SetPattern { .. } | IrOp::SetColor(_) | IrOp::SetColorSpace(_)
            )
        })
        .collect();
    assert_eq!(
        colours,
        [
            IrOp::SetColorSpace(SpaceRef(0)),
            IrOp::SetPattern {
                pattern: PatternIndex(0),
                components: vec![1.0, 0.0, 0.0]
            },
            IrOp::SetPattern {
                pattern: PatternIndex(0),
                components: vec![0.0, 1.0, 0.0]
            },
            IrOp::SetPattern {
                pattern: PatternIndex(1),
                components: vec![0.0, 1.0, 0.0]
            },
            IrOp::SetColorSpace(SpaceRef(1)),
        ]
    );
}

#[test]
fn the_null_pattern_paints_nothing() {
    let (mut g, _) = backend();
    g.define_font(0, &helvetica()).unwrap();
    g.set_color_space(&SpaceSpec::Pattern { base: None })
        .unwrap();
    square(&mut g, 0.0, 0.0, 10.0);
    line(&mut g, p(0.0, 0.0), p(10.0, 10.0));
    g.stroke().unwrap();
    g.set_font(Some(font(0, 10.0))).unwrap();
    g.moveto(p(0.0, 0.0)).unwrap();
    g.show(&[glyph(72, 722.0)]).unwrap();
    let mask = ImageSpec {
        width: 1,
        height: 1,
        bits_per_component: 1,
        color_space: None,
        decode: vec![0.0, 1.0],
        matrix: Matrix::IDENTITY,
        interpolate: false,
        is_mask: true,
        encoded: None,
    };
    g.imagemask(&mask, &[0]).unwrap();
    assert!(ops(&g).is_empty(), "{:?}", ops(&g));
    // An image carries its own colours.
    let image = ImageSpec {
        color_space: Some(SpaceSpec::DeviceGray),
        bits_per_component: 8,
        is_mask: false,
        ..mask
    };
    g.image(&image, &[0]).unwrap();
    assert_eq!(ops(&g).len(), 1);
    // The current point still advanced under the null pattern.
    assert!(close(g.current_point().unwrap(), p(7.22, 0.0)));
}

#[test]
fn a_pattern_inside_a_form_is_relative_to_form_space() {
    let (mut g, pages) = backend();
    let info = pattern(3, Matrix::IDENTITY, 1);
    let outer = form(11, Matrix::translation(50.0, 50.0));
    g.set_line_width(2.0).unwrap();
    let depth = g.gstate_depth();
    assert!(g.begin_form(&outer).unwrap());
    assert_eq!(g.current_matrix(), outer.matrix);
    g.set_color_space(&SpaceSpec::Pattern { base: None })
        .unwrap();
    g.set_pattern(&info, &[]).unwrap();
    assert!(capture_cell(&mut g, &info));
    square(&mut g, 0.0, 0.0, 100.0);
    assert!(
        !g.begin_form(&outer).unwrap(),
        "a form does not nest itself"
    );
    assert_eq!(g.end_pattern_cell(), Err(VmError::InvalidAccess));
    g.end_form().unwrap();
    g.grestore_to(depth).unwrap();
    assert_eq!(g.line_width(), 2.0);
    g.place_form(&outer).unwrap();
    assert!(!g.begin_form(&outer).unwrap());
    // A second execution under a scaled CTM: the VM computes the
    // placement matrix from the form's matrix and the CTM.
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    let scaled = FormInfo {
        matrix: Matrix::translation(50.0, 50.0).then(g.current_matrix()),
        ..outer
    };
    assert!(!g.begin_form(&scaled).unwrap());
    g.place_form(&scaled).unwrap();
    // The same instance on the page: a second resource over the same
    // cell, with the page's matrix.
    g.set_color_space(&SpaceSpec::Pattern { base: None })
        .unwrap();
    g.set_pattern(&info, &[]).unwrap();
    assert!(!capture_cell(&mut g, &info));
    square(&mut g, 0.0, 0.0, 10.0);
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(
        pages[0].dump(),
        format!(
            "ir/1\npage 612 792\nresources:\ncs 0 Pattern\n\
             pattern 0 matrix 1 0 0 1 -50 -50 bbox 0 0 10 10 step 10 10 paint 1 tiling 1 {{\n{CELL}\
             pattern 1 matrix 1 0 0 1 0 0 bbox 0 0 10 10 step 10 10 paint 1 tiling 1 {{\n{CELL}\
             form 0 bbox 0 0 100 100 {{\n  q\n  m 0 0\n  l 100 0\n  l 100 100\n  l 0 100\n  h\n  W n\n\
             \x20 cs 0\n  pattern 0\n  m 0 0\n  l 100 0\n  l 100 100\n  l 0 100\n  h\n  f\n  Q\n}}\n\
             ops:\nw 2\nform 0 1 0 0 1 50 50\nform 0 2 0 0 2 100 100\ncs 0\npattern 1\n\
             m 0 0\nl 20 0\nl 20 20\nl 0 20\nh\nf\n"
        )
    );
}

#[test]
fn forms_nest_and_page_operations_are_refused_inside() {
    let (mut g, pages) = backend();
    let inner = form(1, Matrix::translation(20.0, 30.0));
    let outer = form(2, Matrix::translation(200.0, 200.0));
    g.set_color_space(&SpaceSpec::DeviceRGB).unwrap();
    g.set_color(&[1.0, 0.0, 0.0]).unwrap();
    let depth = g.gstate_depth();
    assert!(g.begin_form(&outer).unwrap());
    assert_eq!(g.showpage(), Err(VmError::InvalidAccess));
    assert_eq!(g.end_pattern_cell(), Err(VmError::InvalidAccess));
    let inner_depth = g.gstate_depth();
    g.concat(inner.matrix).unwrap();
    let placed = inner.matrix.then(g.current_matrix());
    let placed = FormInfo {
        matrix: placed,
        ..inner
    };
    assert!(g.begin_form(&placed).unwrap());
    square(&mut g, 0.0, 0.0, 10.0);
    g.end_form().unwrap();
    g.grestore_to(inner_depth).unwrap();
    g.place_form(&placed).unwrap();
    g.end_form().unwrap();
    g.grestore_to(depth).unwrap();
    g.place_form(&outer).unwrap();
    assert_eq!(g.end_form(), Err(VmError::InvalidAccess));
    g.showpage().unwrap();
    let dump = pages.borrow()[0].dump();
    assert!(
        dump.contains("form 1 bbox 0 0 100 100 {\n  q\n  m 0 0\n  l 100 0\n  l 100 100\n  l 0 100\n  h\n  W n\n  form 0 1 0 0 1 40 60\n  Q\n}\n"),
        "{dump}"
    );
    assert!(
        dump.ends_with("ops:\ncs 0\nsc 1 0 0\nform 1 1 0 0 1 200 200\n"),
        "{dump}"
    );
    // A form placed on a page that never captured it is an empty body;
    // the null device captures and places nothing.
    let (mut g, pages) = backend();
    g.place_form(&outer).unwrap();
    assert_eq!(ops(&g).len(), 1);
    g.gsave().unwrap();
    g.nulldevice().unwrap();
    assert!(!g.begin_form(&inner).unwrap());
    assert!(
        !g.begin_pattern_cell(&pattern(1, Matrix::IDENTITY, 1))
            .unwrap()
    );
    g.place_form(&inner).unwrap();
    assert_eq!(ops(&g).len(), 1);
    g.grestore().unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].resources.forms.len(), 1);
    assert!(pages[0].resources.forms[0].ops.is_empty());
    assert_eq!(pages[0].ops.len(), 1);
}

// --- shadings ------------------------------------------------------------------

fn axial(background: Option<Vec<f32>>) -> ShadingSpec {
    ShadingSpec {
        kind: ShadingKind::Axial {
            coords: [0.0, 0.0, 10.0, 0.0],
            domain: [0.0, 1.0],
            function: vec![FunctionSpec::Exponential {
                domain: vec![0.0, 1.0],
                range: Vec::new(),
                c0: vec![0.0],
                c1: vec![1.0],
                n: 1.0,
            }],
            extend: [false, true],
        },
        space: SpaceSpec::DeviceGray,
        background,
        bbox: None,
        antialias: false,
    }
}

#[test]
fn shadings_intern_by_value_and_shade_ops_carry_the_ctm_through_captures() {
    let (mut g, pages) = backend();
    let shading = axial(Some(vec![0.5]));
    g.concat(Matrix::translation(10.0, 20.0)).unwrap();
    g.set_color_space(&SpaceSpec::DeviceRGB).unwrap();
    g.set_color(&[1.0, 0.0, 0.0]).unwrap();
    g.moveto(p(1.0, 1.0)).unwrap();
    // Neither the colour nor the path is touched, and nothing about the
    // colour is emitted for the shade.
    g.shade(&shading).unwrap();
    g.shade(&shading).unwrap();
    assert_eq!(g.current_point(), Ok(p(1.0, 1.0)));
    assert_eq!(g.current_color(), vec![1.0, 0.0, 0.0]);
    g.newpath().unwrap();
    // A different value is another resource; the same one again is not.
    g.shade(&axial(None)).unwrap();
    // Inside a form body the matrix is relative to form space.
    let outer = form(4, Matrix::scaling(2.0, 2.0));
    let depth = g.gstate_depth();
    assert!(g.begin_form(&outer).unwrap());
    g.concat(Matrix::translation(1.0, 1.0)).unwrap();
    g.shade(&shading).unwrap();
    g.end_form().unwrap();
    g.grestore_to(depth).unwrap();
    g.place_form(&outer).unwrap();
    // A shading pattern has no cell; its resource names the shading.
    let info = PatternInfo {
        id: 9,
        matrix: Matrix::scaling(3.0, 3.0),
        kind: PatternKind::Shading(Rc::new(shading.clone())),
    };
    g.set_color_space(&SpaceSpec::Pattern { base: None })
        .unwrap();
    g.set_pattern(&info, &[]).unwrap();
    assert_eq!(g.current_pattern(), Some(info.clone()));
    assert!(!g.begin_pattern_cell(&info).unwrap());
    square(&mut g, 0.0, 0.0, 10.0);
    // Under the null device nothing is recorded.
    g.gsave().unwrap();
    g.nulldevice().unwrap();
    g.shade(&shading).unwrap();
    g.grestore().unwrap();
    g.showpage().unwrap();
    let pages = pages.borrow();
    let page = &pages[0];
    assert_eq!(page.resources.shadings.len(), 2);
    assert_eq!(
        page.resources.patterns,
        [PatternSpec::Shading {
            matrix: Matrix::scaling(3.0, 3.0),
            shading: ShadingIndex(0),
        }]
    );
    // The shades emit nothing about the colour; the form placement is
    // what flushes the red set before them.
    assert_eq!(
        page.dump(),
        "ir/1\npage 612 792\nresources:\ncs 0 DeviceRGB\ncs 1 Pattern\n\
         shading 0 type 2 space DeviceGray background [0.5] coords [0 0 10 0] domain [0 1] extend [false true] {\n\
         \x20 function type 2 domain [0 1] c0 [0] c1 [1] n 1\n}\n\
         shading 1 type 2 space DeviceGray coords [0 0 10 0] domain [0 1] extend [false true] {\n\
         \x20 function type 2 domain [0 1] c0 [0] c1 [1] n 1\n}\n\
         pattern 0 shading 0 matrix 3 0 0 3 0 0\n\
         form 0 bbox 0 0 100 100 {\n  q\n  m 0 0\n  l 100 0\n  l 100 100\n  l 0 100\n  h\n  W n\n\
         \x20 sh 0 1 0 0 1 1 1\n  Q\n}\n\
         ops:\nsh 0 1 0 0 1 10 20\nsh 0 1 0 0 1 10 20\nsh 1 1 0 0 1 10 20\ncs 0\nsc 1 0 0\n\
         form 0 2 0 0 2 0 0\ncs 1\npattern 0\nm 10 20\nl 20 20\nl 20 30\nl 10 30\nh\nf\n"
    );
}

#[test]
fn smoothness_is_kept_clamped_and_reset_by_initgraphics() {
    let (mut g, _) = backend();
    assert_eq!(g.smoothness(), 0.02);
    g.set_smoothness(0.5).unwrap();
    g.gsave().unwrap();
    g.set_smoothness(2.0).unwrap();
    assert_eq!(g.smoothness(), 1.0);
    g.set_smoothness(-1.0).unwrap();
    assert_eq!(g.smoothness(), 0.0);
    assert_eq!(g.set_smoothness(f32::NAN), Err(VmError::RangeCheck));
    g.grestore().unwrap();
    assert_eq!(g.smoothness(), 0.5);
    g.initgraphics().unwrap();
    assert_eq!(g.smoothness(), 0.02);
}

// --- readings in double precision ---------------------------------------------------

/// Six significant digits: what a reading must agree to.
fn six_digits(got: f32, want: f64) -> bool {
    let scale = 10f64.powi(5 - want.abs().log10().floor() as i32);
    ((f64::from(got) * scale).round() - (want * scale).round()).abs() <= 1.0
}

// pathbbox-control-points.ps
#[test]
fn pathbbox_encloses_control_points_and_ignores_a_trailing_moveto() {
    let (mut g, _) = backend();
    g.moveto(p(0.0, 0.0)).unwrap();
    g.curveto(p(0.0, 100.0), p(100.0, 100.0), p(100.0, 0.0))
        .unwrap();
    assert_eq!(g.path_bbox(), Ok(Bounds::new(0.0, 0.0, 100.0, 100.0)));
    g.moveto(p(50.0, 50.0)).unwrap();
    assert_eq!(
        g.path_bbox(),
        Ok(Bounds::new(0.0, 0.0, 100.0, 100.0)),
        "a moveto ending the path is not considered"
    );
    g.moveto(p(-50.0, 300.0)).unwrap();
    assert_eq!(
        g.path_bbox(),
        Ok(Bounds::new(0.0, 0.0, 100.0, 100.0)),
        "the replacing moveto neither"
    );
    g.lineto(p(-50.0, 300.0)).unwrap();
    assert_eq!(g.path_bbox(), Ok(Bounds::new(-50.0, 0.0, 100.0, 300.0)));
    g.newpath().unwrap();
    g.moveto(p(50.0, 50.0)).unwrap();
    assert_eq!(
        g.path_bbox(),
        Ok(Bounds::new(50.0, 50.0, 50.0, 50.0)),
        "a lone moveto is the whole path"
    );
    g.closepath().unwrap();
    assert_eq!(g.path_bbox(), Ok(Bounds::new(50.0, 50.0, 50.0, 50.0)));
    g.newpath().unwrap();
    assert_eq!(g.path_bbox(), Err(VmError::NoCurrentPoint));
}

// pathbbox-rules.ps
#[test]
fn pathbbox_is_the_envelope_of_the_device_box_corners() {
    let (mut g, _) = backend();
    g.concat(Matrix::rotation(45.0)).unwrap();
    g.moveto(p(0.0, 0.0)).unwrap();
    g.lineto(p(100.0, 0.0)).unwrap();
    g.lineto(p(100.0, 100.0)).unwrap();
    g.lineto(p(0.0, 100.0)).unwrap();
    g.closepath().unwrap();
    // The device box of the rotated square is the diamond's box, ±70.71
    // by 0..141.42; its corners taken back to user space span more than
    // the square.
    let Bounds { llx, lly, urx, ury } = g.path_bbox().unwrap();
    assert!(
        six_digits(llx, -50.0) && six_digits(urx, 150.0),
        "{llx} {urx}"
    );
    assert!(
        six_digits(lly, -50.0) && six_digits(ury, 150.0),
        "{lly} {ury}"
    );
}

// arcto-acute-tangent.ps
#[test]
fn current_point_and_arcto_readings_are_rounded_once() {
    // A point read back through the inverse of a rotation is the point
    // that was set, to the last digit.
    let (mut g, _) = backend();
    g.concat(Matrix::rotation(-45.0)).unwrap();
    g.concat(Matrix::scaling(0.80, 0.89)).unwrap();
    g.moveto(p(12.3, 45.6)).unwrap();
    assert_eq!(g.current_point(), Ok(p(12.3, 45.6)));

    // The generator's acute corner: tangent points some 930 units out,
    // compared with the same construction done wholly in f64.
    let (mut g, _) = backend();
    g.concat(Matrix::rotation(-45.0)).unwrap();
    g.moveto(p(440.0, 404.0)).unwrap();
    let (t1, t2) = g.arcto(p(464.0, 404.0), p(371.0, 414.0), 50.0).unwrap();
    let exact = exact_tangents((440.0, 404.0), (464.0, 404.0), (371.0, 414.0), 50.0);
    assert!(six_digits(t1.x, exact.0.0), "{t1:?} {exact:?}");
    assert!(six_digits(t1.y, exact.0.1), "{t1:?} {exact:?}");
    assert!(six_digits(t2.x, exact.1.0), "{t2:?} {exact:?}");
    assert!(six_digits(t2.y, exact.1.1), "{t2:?} {exact:?}");
    assert!(
        six_digits(t1.x, -468.680) && six_digits(t1.y, 404.0),
        "{t1:?}"
    );
    assert!(
        six_digits(t2.x, -463.335) && six_digits(t2.y, 503.713),
        "{t2:?}"
    );

    // The generator's quarter-turn corner under an anisotropic scale:
    // the sweep is a right angle to the last digit, one Bezier piece.
    let (mut g, _) = backend();
    g.concat(Matrix::rotation(-45.0)).unwrap();
    g.concat(Matrix::scaling(0.80, 0.89)).unwrap();
    g.concat(Matrix::scaling(1.50, 1.16)).unwrap();
    g.moveto(p(7.0, 8.0)).unwrap();
    let (t1, t2) = g.arcto(p(23.0, 8.0), p(23.0, 57.0), 45.0).unwrap();
    assert_eq!((t1, t2), (p(-22.0, 8.0), p(23.0, 53.0)));
    g.stroke().unwrap();
    let recorded = ops(&g);
    let [IrOp::Stroke { path, .. }] = recorded.as_slice() else {
        panic!("one stroke");
    };
    assert_eq!(path.len(), 3, "{path:?}");
}

/// The tangent points of `arcto` for the corner `p0 → p1 → p2` computed
/// independently in f64: the corner's half angle from the unit vectors,
/// the tangent distance r / tan(θ/2) along each edge.
fn exact_tangents(
    p0: (f64, f64),
    p1: (f64, f64),
    p2: (f64, f64),
    r: f64,
) -> ((f64, f64), (f64, f64)) {
    let (ux, uy) = (p0.0 - p1.0, p0.1 - p1.1);
    let (vx, vy) = (p2.0 - p1.0, p2.1 - p1.1);
    let (lu, lv) = ((ux * ux + uy * uy).sqrt(), (vx * vx + vy * vy).sqrt());
    let (ux, uy, vx, vy) = (ux / lu, uy / lu, vx / lv, vy / lv);
    let half = (ux * vx + uy * vy).acos() / 2.0;
    let d = r / half.tan();
    (
        (p1.0 + ux * d, p1.1 + uy * d),
        (p1.0 + vx * d, p1.1 + vy * d),
    )
}

// --- overprint and stroke outlines ---------------------------------------------------

#[test]
fn overprint_is_emitted_where_it_changes_and_restored_with_the_clip() {
    let (mut g, _) = backend();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    g.set_overprint(true).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    // Set again to the same value: nothing new.
    g.set_overprint(true).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.stroke().unwrap();
    g.gsave().unwrap();
    g.rectclip(&[Rect {
        x: 0.0,
        y: 0.0,
        width: 5.0,
        height: 5.0,
    }])
    .unwrap();
    g.set_overprint(false).unwrap();
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    g.grestore().unwrap();
    assert!(g.state().overprint);
    // The restore brings the earlier setting back on both sides.
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    g.set_overprint(false).unwrap();
    let spec = ImageSpec {
        width: 1,
        height: 1,
        bits_per_component: 8,
        color_space: Some(SpaceSpec::DeviceGray),
        decode: vec![0.0, 1.0],
        matrix: Matrix::IDENTITY,
        interpolate: false,
        is_mask: false,
        encoded: None,
    };
    g.image(&spec, &[0]).unwrap();
    let kinds: Vec<String> = ops(&g)
        .iter()
        .map(|o| {
            format!("{o:?}")
                .split(['(', ' ', '{'])
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "Fill",
            "Overprint",
            "Fill",
            "Stroke",
            "Save",
            "Clip",
            "Overprint",
            "Fill",
            "Restore",
            "Fill",
            "Overprint",
            "Image",
        ]
    );
    let flags: Vec<bool> = ops(&g)
        .into_iter()
        .filter_map(|o| match o {
            IrOp::Overprint(on) => Some(on),
            _ => None,
        })
        .collect();
    assert_eq!(flags, [true, false, false]);
    // initgraphics and showpage leave the setting, the next page's
    // emitter starts afresh and records it again.
    g.initgraphics().unwrap();
    assert!(!g.state().overprint);
    g.set_overprint(true).unwrap();
    g.showpage().unwrap();
    assert!(g.state().overprint);
    line(&mut g, p(0.0, 0.0), p(1.0, 0.0));
    g.fill().unwrap();
    assert_eq!(ops(&g).first(), Some(&IrOp::Overprint(true)));
}

#[test]
fn stroke_outline_replaces_the_path_with_closed_subpaths() {
    let (mut g, _) = backend();
    g.set_line_width(2.0).unwrap();
    line(&mut g, p(0.0, 0.0), p(10.0, 0.0));
    g.stroke_outline().unwrap();
    let segs: Vec<Seg> = g.state().path.segs.to_vec();
    assert_eq!(segs.len(), 5);
    assert!(matches!(segs[0], Seg::Move(_)));
    assert_eq!(segs[4], Seg::Close);
    assert_eq!(g.path_bbox(), Ok(Bounds::new(0.0, -1.0, 10.0, 1.0)));
    let current = g.current_point().unwrap();
    assert!(current.x == 0.0 || current.x == 10.0);
    g.fill().unwrap();
    assert!(matches!(ops(&g).as_slice(), [IrOp::Fill { .. }]));
    // The outline is built under the CTM at the call, in default user
    // space like every stored path, and a stroke of it would not be
    // wanted: the fill's segments carry no CTM.
    g.concat(Matrix::scaling(2.0, 2.0)).unwrap();
    line(&mut g, p(0.0, 0.0), p(10.0, 0.0));
    g.stroke_outline().unwrap();
    assert_eq!(g.path_bbox(), Ok(Bounds::new(0.0, -1.0, 10.0, 1.0)));
    // An empty path or a lone moveto outlines to nothing, leaving no
    // current point.
    g.newpath().unwrap();
    g.stroke_outline().unwrap();
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
    g.moveto(p(3.0, 3.0)).unwrap();
    g.stroke_outline().unwrap();
    assert_eq!(g.current_point(), Err(VmError::NoCurrentPoint));
    assert!(g.state().path.is_empty());
}

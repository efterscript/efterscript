// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The backend driven directly, without an interpreter: path building
//! through the CTM, arcs, emission and its deduplication, clips, and page
//! delivery.

use std::cell::RefCell;
use std::rc::Rc;

use ps_graphics::{FillRule, Graphics, IrOp, Page, SpaceRef};
use ps_vm::{
    Bounds, GraphicsBackend, ImageSpec, LineCap, Matrix, Point, Rect, Seg, SpaceSpec, VmError,
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

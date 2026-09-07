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

// --- text ------------------------------------------------------------------------------

use ps_fonts::{ResidentFace, STANDARD_ENCODING};
use ps_graphics::{FontIndex, FontSpec, GlyphProc, glyph_names};
use ps_vm::{FontInfo, FontRef, FontSource, Glyph};

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
    use ps_fonts::ProgramKind;
    use ps_fonts::testing::corpus_type1;
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
    use ps_fonts::ProgramKind;
    use ps_fonts::cmap::CMapBuilder;
    use ps_fonts::testing::corpus_cid_cff;
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
    use ps_fonts::cmap::CMapBuilder;
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

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The stroke outline on its own: caps, joins, the mitre limit, dashes
//! and their phase, closed subpaths, degenerate pieces, the pen under a
//! non-uniform CTM, and the flatness. The outline is checked by what a
//! nonzero fill of it would cover — points inside and outside — and by
//! the rings it is made of.

use efterscript_graphics::outline::{StrokeStyle, outline};
use efterscript_vm::{LineCap, LineJoin, Matrix, Point, Seg};

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

fn style(width: f32, cap: LineCap, join: LineJoin) -> StrokeStyle {
    StrokeStyle {
        width,
        cap,
        join,
        miter_limit: 10.0,
        dash: (Vec::new(), 0.0),
        flatness: 1.0,
        ctm: Matrix::IDENTITY,
    }
}

/// The closed rings of an outline, each a list of corners.
fn rings(segs: &[Seg]) -> Vec<Vec<Point>> {
    let mut rings = Vec::new();
    let mut current: Vec<Point> = Vec::new();
    for seg in segs {
        match *seg {
            Seg::Move(q) => {
                assert!(current.is_empty(), "a ring must be closed before the next");
                current.push(q);
            }
            Seg::Line(q) => current.push(q),
            Seg::Curve(..) => panic!("an outline has no curves"),
            Seg::Close => rings.push(std::mem::take(&mut current)),
        }
    }
    assert!(current.is_empty(), "the last ring must be closed");
    rings
}

fn signed_area(ring: &[Point]) -> f64 {
    ring.iter()
        .zip(ring.iter().cycle().skip(1))
        .map(|(a, b)| f64::from(a.x) * f64::from(b.y) - f64::from(b.x) * f64::from(a.y))
        .sum::<f64>()
        / 2.0
}

/// The nonzero winding number of `q` over `rings`.
fn winding(rings: &[Vec<Point>], q: Point) -> i32 {
    let mut total = 0;
    for ring in rings {
        for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
            let (ay, by) = (f64::from(a.y), f64::from(b.y));
            let (ax, bx) = (f64::from(a.x), f64::from(b.x));
            let (qx, qy) = (f64::from(q.x), f64::from(q.y));
            if ay <= qy {
                if by > qy && (bx - ax) * (qy - ay) - (qx - ax) * (by - ay) > 0.0 {
                    total += 1;
                }
            } else if by <= qy && (bx - ax) * (qy - ay) - (qx - ax) * (by - ay) < 0.0 {
                total -= 1;
            }
        }
    }
    total
}

fn covers(segs: &[Seg], q: Point) -> bool {
    winding(&rings(segs), q) != 0
}

fn bbox(segs: &[Seg]) -> (f32, f32, f32, f32) {
    let mut b = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for ring in rings(segs) {
        for q in ring {
            b.0 = b.0.min(q.x);
            b.1 = b.1.min(q.y);
            b.2 = b.2.max(q.x);
            b.3 = b.3.max(q.y);
        }
    }
    b
}

fn near(a: (f32, f32, f32, f32), b: (f32, f32, f32, f32)) -> bool {
    (a.0 - b.0).abs() < 1e-3
        && (a.1 - b.1).abs() < 1e-3
        && (a.2 - b.2).abs() < 1e-3
        && (a.3 - b.3).abs() < 1e-3
}

/// Two segments meeting at a right angle: east, then north.
fn corner() -> Vec<Seg> {
    vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(100.0, 0.0)),
        Seg::Line(p(100.0, 100.0)),
    ]
}

#[test]
fn butt_caps_stop_at_the_ends() {
    let out = outline(&corner(), &style(20.0, LineCap::Butt, LineJoin::Miter));
    assert!(near(bbox(&out), (0.0, -10.0, 110.0, 100.0)));
    assert!(covers(&out, p(50.0, 9.0)));
    assert!(covers(&out, p(50.0, -9.0)));
    assert!(!covers(&out, p(-1.0, 0.0)));
    assert!(!covers(&out, p(100.0, 101.0)));
}

#[test]
fn round_caps_reach_half_the_width_past_the_ends() {
    let out = outline(&corner(), &style(20.0, LineCap::Round, LineJoin::Miter));
    let b = bbox(&out);
    assert!(
        (b.0 + 10.0).abs() < 0.5 && (b.3 - 110.0).abs() < 0.5,
        "{b:?}"
    );
    assert!(covers(&out, p(-9.0, 0.0)));
    assert!(covers(&out, p(100.0, 109.0)));
    // Outside the disc around the end, though inside a square cap.
    assert!(!covers(&out, p(-9.0, -9.0)));
    assert!(!covers(&out, p(109.0, 109.0)));
}

#[test]
fn square_caps_reach_half_the_width_past_the_ends() {
    let out = outline(&corner(), &style(20.0, LineCap::Square, LineJoin::Miter));
    assert!(near(bbox(&out), (-10.0, -10.0, 110.0, 110.0)));
    assert!(covers(&out, p(-9.0, -9.0)));
    assert!(covers(&out, p(109.0, 109.0)));
    assert!(!covers(&out, p(-11.0, 0.0)));
}

#[test]
fn joins_shape_the_outer_corner() {
    let miter = outline(&corner(), &style(20.0, LineCap::Butt, LineJoin::Miter));
    let round = outline(&corner(), &style(20.0, LineCap::Butt, LineJoin::Round));
    let bevel = outline(&corner(), &style(20.0, LineCap::Butt, LineJoin::Bevel));
    // The outer corner of the mitre is the full square corner.
    assert!(covers(&miter, p(109.0, -9.0)));
    assert!(!covers(&round, p(109.0, -9.0)));
    assert!(!covers(&bevel, p(109.0, -9.0)));
    // Inside the arc of a round join, beyond the bevel's chord.
    assert!(covers(&miter, p(107.0, -7.0)));
    assert!(covers(&round, p(107.0, -7.0)));
    assert!(!covers(&bevel, p(107.0, -7.0)));
    // Inside all three: on the bevel's own side of the chord.
    assert!(covers(&bevel, p(103.0, -3.0)));
    // The inner corner is covered by every join.
    for out in [&miter, &round, &bevel] {
        assert!(covers(out, p(91.0, 9.0)));
        assert!(!covers(out, p(89.0, 11.0)));
    }
}

#[test]
fn the_miter_limit_turns_a_sharp_corner_into_a_bevel() {
    // A corner of about 11 degrees: the mitre would run 1/sin(5.7°),
    // some ten line widths, past the corner.
    let sharp = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(100.0, 0.0)),
        Seg::Line(p(0.0, 20.0)),
    ];
    let mut limited = style(10.0, LineCap::Butt, LineJoin::Miter);
    limited.miter_limit = 4.0;
    let bevelled = outline(&sharp, &limited);
    let mut generous = limited.clone();
    generous.miter_limit = 20.0;
    let spiked = outline(&sharp, &generous);
    let (_, _, bevel_reach, _) = bbox(&bevelled);
    let (_, _, spike_reach, _) = bbox(&spiked);
    assert!(bevel_reach < 106.0, "{bevel_reach}");
    assert!(spike_reach > 140.0, "{spike_reach}");
    let bevel_only = outline(&sharp, &style(10.0, LineCap::Butt, LineJoin::Bevel));
    assert!(near(bbox(&bevel_only), bbox(&bevelled)));
}

#[test]
fn dashes_cut_a_line_into_pieces_from_the_phase() {
    let line = vec![Seg::Move(p(0.0, 0.0)), Seg::Line(p(100.0, 0.0))];
    let mut dashed = style(2.0, LineCap::Butt, LineJoin::Miter);
    dashed.dash = (vec![20.0, 10.0], 0.0);
    let out = outline(&line, &dashed);
    assert_eq!(rings(&out).len(), 4);
    assert!(covers(&out, p(10.0, 0.0)));
    assert!(!covers(&out, p(25.0, 0.0)));
    assert!(covers(&out, p(95.0, 0.0)));
    dashed.dash.1 = 5.0;
    let shifted = outline(&line, &dashed);
    assert_eq!(rings(&shifted).len(), 4);
    assert!(covers(&shifted, p(14.0, 0.0)));
    assert!(!covers(&shifted, p(16.0, 0.0)));
    dashed.dash.1 = 20.0;
    let in_a_gap = outline(&line, &dashed);
    assert_eq!(rings(&in_a_gap).len(), 3);
    assert!(!covers(&in_a_gap, p(5.0, 0.0)));
    assert!(covers(&in_a_gap, p(15.0, 0.0)));
    // A dash pattern longer than the line leaves one solid piece.
    dashed.dash = (vec![500.0, 1.0], 0.0);
    assert_eq!(rings(&outline(&line, &dashed)).len(), 1);
}

#[test]
fn a_dashed_curve_starts_its_pattern_at_the_subpath_start() {
    let arch = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Curve(p(0.0, 100.0), p(100.0, 100.0), p(100.0, 0.0)),
    ];
    let mut dashed = style(2.0, LineCap::Butt, LineJoin::Miter);
    dashed.dash = (vec![5.0, 1000.0], 0.0);
    let first_dash = outline(&arch, &dashed);
    let (_, y0, _, y1) = bbox(&first_dash);
    assert!(y0 >= -1.1 && y1 <= 6.1, "{y0} {y1}");
    assert!(rings(&first_dash).len() <= 3);
    dashed.dash.1 = 1000.0;
    let second = outline(&arch, &dashed);
    let (_, y0, _, y1) = bbox(&second);
    assert!(y0 >= 3.9 && y1 <= 11.1, "{y0} {y1}");
    dashed.dash = (vec![30.0, 10.0], 0.0);
    let many = outline(&arch, &dashed);
    assert!(rings(&many).len() > rings(&first_dash).len());
    assert!(covers(&many, p(0.0, 15.0)));
}

#[test]
fn a_closed_rectangle_leaves_a_hole_and_has_no_caps() {
    let square = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(100.0, 0.0)),
        Seg::Line(p(100.0, 100.0)),
        Seg::Line(p(0.0, 100.0)),
        Seg::Close,
    ];
    let out = outline(&square, &style(20.0, LineCap::Round, LineJoin::Miter));
    assert!(near(bbox(&out), (-10.0, -10.0, 110.0, 110.0)));
    assert!(
        covers(&out, p(-9.0, -9.0)),
        "the start corner is joined, not capped"
    );
    assert!(covers(&out, p(5.0, 5.0)));
    assert!(!covers(&out, p(50.0, 50.0)));
    assert!(!covers(&out, p(11.0, 11.0)));
    assert!(covers(&out, p(109.0, 109.0)));
    // A dash pattern that is on across the start continues round the
    // corner rather than capping there.
    let mut dashed = style(20.0, LineCap::Butt, LineJoin::Miter);
    dashed.dash = (vec![150.0, 50.0], 100.0);
    let out = outline(&square, &dashed);
    assert!(covers(&out, p(-9.0, -9.0)));
    assert!(!covers(&out, p(75.0, 0.0)));
}

#[test]
fn degenerate_subpaths_are_dots_with_round_caps_and_nothing_otherwise() {
    let dot = vec![Seg::Move(p(5.0, 5.0)), Seg::Line(p(5.0, 5.0))];
    let round = outline(&dot, &style(10.0, LineCap::Round, LineJoin::Miter));
    assert_eq!(rings(&round).len(), 1);
    // An inscribed polygon: within the flatness of the disc's edge.
    let b = bbox(&round);
    assert!(
        (b.0 - 0.0).abs() <= 1.0 && (b.2 - 10.0).abs() <= 1.0,
        "{b:?}"
    );
    assert!(covers(&round, p(5.0, 5.0)));
    assert!(outline(&dot, &style(10.0, LineCap::Butt, LineJoin::Miter)).is_empty());
    assert!(outline(&dot, &style(10.0, LineCap::Square, LineJoin::Miter)).is_empty());
    let closed_dot = vec![Seg::Move(p(5.0, 5.0)), Seg::Close];
    assert_eq!(
        rings(&outline(
            &closed_dot,
            &style(10.0, LineCap::Round, LineJoin::Miter)
        ))
        .len(),
        1
    );
    let lone_move = vec![Seg::Move(p(5.0, 5.0))];
    for cap in [LineCap::Butt, LineCap::Round, LineCap::Square] {
        assert!(outline(&lone_move, &style(10.0, cap, LineJoin::Miter)).is_empty());
    }
    let trailing_move = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Line(p(10.0, 0.0)),
        Seg::Move(p(50.0, 50.0)),
    ];
    let out = outline(
        &trailing_move,
        &style(10.0, LineCap::Round, LineJoin::Miter),
    );
    assert!(!covers(&out, p(50.0, 50.0)));
    assert!(covers(&out, p(5.0, 0.0)));
}

#[test]
fn zero_length_dashes_are_dots_or_squares() {
    let line = vec![Seg::Move(p(0.0, 0.0)), Seg::Line(p(100.0, 0.0))];
    let mut dotted = style(6.0, LineCap::Round, LineJoin::Miter);
    dotted.dash = (vec![0.0, 25.0], 0.0);
    let dots = outline(&line, &dotted);
    assert_eq!(rings(&dots).len(), 5);
    assert!(covers(&dots, p(50.0, 0.0)));
    assert!(!covers(&dots, p(40.0, 0.0)));
    dotted.cap = LineCap::Square;
    let squares = outline(&line, &dotted);
    assert_eq!(rings(&squares).len(), 5);
    assert!(covers(&squares, p(52.9, 2.9)));
    assert!(!covers(&squares, p(53.1, 0.0)));
    dotted.cap = LineCap::Butt;
    assert!(outline(&line, &dotted).is_empty());
}

#[test]
fn the_pen_is_round_in_the_strokes_user_space() {
    let mut tall = style(10.0, LineCap::Butt, LineJoin::Miter);
    tall.ctm = Matrix::scaling(1.0, 4.0);
    let across = vec![Seg::Move(p(0.0, 0.0)), Seg::Line(p(100.0, 0.0))];
    assert!(near(
        bbox(&outline(&across, &tall)),
        (0.0, -20.0, 100.0, 20.0)
    ));
    let up = vec![Seg::Move(p(0.0, 0.0)), Seg::Line(p(0.0, 100.0))];
    assert!(near(bbox(&outline(&up, &tall)), (-5.0, 0.0, 5.0, 100.0)));
    // Dash lengths are user-space distances: 10 along y is 40 on the page.
    tall.dash = (vec![10.0, 10.0], 0.0);
    let dashed = outline(&up, &tall);
    assert!(covers(&dashed, p(0.0, 39.0)));
    assert!(!covers(&dashed, p(0.0, 41.0)));
    let mut singular = tall.clone();
    singular.ctm = Matrix::scaling(0.0, 1.0);
    assert!(outline(&across, &singular).is_empty());
}

#[test]
fn flatness_sets_how_finely_curves_and_pens_are_cut() {
    let arch = vec![
        Seg::Move(p(0.0, 0.0)),
        Seg::Curve(p(0.0, 100.0), p(100.0, 100.0), p(100.0, 0.0)),
    ];
    let mut fine = style(2.0, LineCap::Butt, LineJoin::Bevel);
    fine.flatness = 0.2;
    let mut coarse = fine.clone();
    coarse.flatness = 10.0;
    let fine_rings = rings(&outline(&arch, &fine)).len();
    let coarse_rings = rings(&outline(&arch, &coarse)).len();
    assert!(fine_rings > coarse_rings, "{fine_rings} vs {coarse_rings}");
    let dot = vec![Seg::Move(p(0.0, 0.0)), Seg::Line(p(0.0, 0.0))];
    let mut round = style(40.0, LineCap::Round, LineJoin::Miter);
    round.flatness = 0.2;
    let fine_dot = rings(&outline(&dot, &round))[0].len();
    round.flatness = 10.0;
    let coarse_dot = rings(&outline(&dot, &round))[0].len();
    assert!(fine_dot > coarse_dot, "{fine_dot} vs {coarse_dot}");
}

#[test]
fn every_ring_is_closed_and_turns_the_same_way() {
    let mut flipped = style(20.0, LineCap::Round, LineJoin::Round);
    flipped.ctm = Matrix::scaling(1.0, -1.0);
    for style in [style(20.0, LineCap::Round, LineJoin::Round), flipped] {
        let out = outline(&corner(), &style);
        assert!(matches!(out.last(), Some(Seg::Close)));
        for ring in rings(&out) {
            assert!(ring.len() >= 3);
            assert!(signed_area(&ring) > 0.0, "{ring:?}");
        }
    }
}

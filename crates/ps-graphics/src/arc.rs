// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Circular arcs as cubic Bézier curves. A sweep is cut into pieces of at
//! most a quarter turn, each approximated by one cubic whose control
//! points sit at `4/3·tan(θ/4)` radii along the tangents; the error of
//! that construction is far below the precision of an `f32` coordinate.
//! The geometry is computed in double precision and handed out that
//! way; the backend takes the points through the CTM and narrows them
//! once.

/// A point in double precision.
pub type P64 = (f64, f64);

/// One cubic piece: the two control points and the end point.
pub type Curve = (P64, P64, P64);

/// Degrees `end − start` swept counter-clockwise for `ccw`, otherwise
/// clockwise, with the end angle brought to the correct side of the start
/// angle the way `arc` and `arcn` do it. The result is signed: positive
/// for a counter-clockwise sweep.
pub fn sweep(start: f64, end: f64, ccw: bool) -> f64 {
    let mut end = end;
    if ccw {
        while end < start {
            end += 360.0;
        }
    } else {
        while end > start {
            end -= 360.0;
        }
    }
    end - start
}

/// Sine and cosine of an angle in degrees, exact at multiples of a
/// quarter turn so axis-aligned arc points carry no rounding residue
/// into the IR.
fn sin_cos_deg(degrees: f64) -> (f64, f64) {
    let turn = degrees.rem_euclid(360.0);
    if turn == 0.0 {
        (0.0, 1.0)
    } else if turn == 90.0 {
        (1.0, 0.0)
    } else if turn == 180.0 {
        (0.0, -1.0)
    } else if turn == 270.0 {
        (-1.0, 0.0)
    } else {
        degrees.to_radians().sin_cos()
    }
}

/// A circle centre in double precision.
pub type Center = P64;

/// The point at `degrees` on the circle.
pub fn point_at(center: Center, radius: f64, degrees: f64) -> P64 {
    let (sin, cos) = sin_cos_deg(degrees);
    (center.0 + radius * cos, center.1 + radius * sin)
}

/// The Bézier pieces of the arc from `start` degrees sweeping `sweep`
/// degrees (signed, as [`sweep`] returns it). An empty sweep yields no
/// pieces. A sweep within rounding above a multiple of a quarter turn
/// takes as many pieces as the exact multiple.
pub fn curves(center: Center, radius: f64, start: f64, sweep: f64) -> Vec<Curve> {
    if sweep == 0.0 || !sweep.is_finite() {
        return Vec::new();
    }
    let pieces = (sweep.abs() / 90.0 - 1e-9).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let kappa = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    let (cx, cy, r) = (center.0, center.1, radius);
    let mut out = Vec::with_capacity(pieces);
    let mut angle = start;
    for _ in 0..pieces {
        let next = angle + step;
        let (s0, c0) = sin_cos_deg(angle);
        let (s1, c1) = sin_cos_deg(next);
        let p = |x: f64, y: f64| (cx + r * x, cy + r * y);
        out.push((
            p(c0 - kappa * s0, s0 + kappa * c0),
            p(c1 + kappa * s1, s1 - kappa * c1),
            p(c1, s1),
        ));
        angle = next;
    }
    out
}

/// The tangent construction behind `arcto`: for the corner `p0 → p1 → p2`
/// and a radius, the two tangent points, the circle centre, and the start
/// and (signed) sweep of the arc between them. `None` when no arc exists —
/// coincident points or collinear lines — which the caller turns into a
/// straight line to `p1`.
pub struct Tangent {
    pub t1: P64,
    pub t2: P64,
    pub center: Center,
    pub start: f64,
    pub sweep: f64,
}

pub fn tangent(p0: P64, p1: P64, p2: P64, r: f64) -> Option<Tangent> {
    let (x0, y0) = p0;
    let (x1, y1) = p1;
    let (x2, y2) = p2;
    let (ux, uy) = (x0 - x1, y0 - y1);
    let (vx, vy) = (x2 - x1, y2 - y1);
    let lu = (ux * ux + uy * uy).sqrt();
    let lv = (vx * vx + vy * vy).sqrt();
    if lu == 0.0 || lv == 0.0 {
        return None;
    }
    let (ux, uy, vx, vy) = (ux / lu, uy / lu, vx / lv, vy / lv);
    let cross = ux * vy - uy * vx;
    if cross.abs() < 1e-9 {
        return None;
    }
    // θ is the angle at the corner; the tangent points lie r/tan(θ/2)
    // from it and the centre r/sin(θ/2) along the bisector.
    let cos_theta = (ux * vx + uy * vy).clamp(-1.0, 1.0);
    let half = cos_theta.acos() / 2.0;
    let d = r / half.tan();
    let (t1x, t1y) = (x1 + ux * d, y1 + uy * d);
    let (t2x, t2y) = (x1 + vx * d, y1 + vy * d);
    let (bx, by) = (ux + vx, uy + vy);
    let lb = (bx * bx + by * by).sqrt();
    let h = r / half.sin();
    let (cx, cy) = (x1 + bx / lb * h, y1 + by / lb * h);
    let start = (t1y - cy).atan2(t1x - cx).to_degrees();
    let end = (t2y - cy).atan2(t2x - cx).to_degrees();
    // A left turn is a counter-clockwise arc; `u` points back along the
    // incoming edge, so a left turn has a negative cross product.
    let sweep = sweep(start, end, cross < 0.0);
    Some(Tangent {
        t1: (t1x, t1y),
        t2: (t2x, t2y),
        center: (cx, cy),
        start,
        sweep,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: P64, b: P64) -> bool {
        (a.0 - b.0).abs() < 1e-3 && (a.1 - b.1).abs() < 1e-3
    }

    fn exact(a: P64, b: P64) -> bool {
        a.0 == b.0 && a.1 == b.1
    }

    #[test]
    fn sweeps_wrap_to_the_right_side() {
        assert_eq!(sweep(0.0, 90.0, true), 90.0);
        assert_eq!(sweep(90.0, 0.0, true), 270.0);
        assert_eq!(sweep(90.0, 0.0, false), -90.0);
        assert_eq!(sweep(0.0, 90.0, false), -270.0);
        assert_eq!(sweep(0.0, 360.0, true), 360.0);
        assert_eq!(sweep(0.0, 0.0, true), 0.0);
    }

    #[test]
    fn full_circle_is_four_pieces_ending_at_the_start() {
        let c = curves((0.0, 0.0), 10.0, 0.0, 360.0);
        assert_eq!(c.len(), 4);
        // Quarter-turn points are exact, not within rounding.
        assert!(exact(c[3].2, (10.0, 0.0)));
        assert!(exact(c[0].2, (0.0, 10.0)));
        assert!(exact(c[1].2, (-10.0, 0.0)));
        assert!(exact(point_at((1.0, 1.0), 2.0, -90.0), (1.0, -1.0)));
        assert!(curves((0.0, 0.0), 10.0, 0.0, 0.0).is_empty());
        assert_eq!(curves((0.0, 0.0), 10.0, 0.0, 91.0).len(), 2);
    }

    #[test]
    fn a_quarter_turn_from_rounding_is_one_piece() {
        let exact = curves((0.0, 0.0), 10.0, 0.0, 90.0);
        let above = curves((0.0, 0.0), 10.0, 0.0, 90.0 + 90.0 * f64::EPSILON);
        assert_eq!(above.len(), 1);
        for (a, b) in [
            (above[0].0, exact[0].0),
            (above[0].1, exact[0].1),
            (above[0].2, exact[0].2),
        ] {
            assert!(close(a, b), "{a:?} {b:?}");
        }
        assert_eq!(curves((0.0, 0.0), 10.0, 0.0, -(180.0 + 1e-10)).len(), 2);
        assert_eq!(curves((0.0, 0.0), 10.0, 0.0, 360.0 + 1e-10).len(), 4);
        assert_eq!(curves((0.0, 0.0), 10.0, 0.0, 90.001).len(), 2);
    }

    #[test]
    fn quarter_arc_control_points_use_the_standard_kappa() {
        let pieces = curves((0.0, 0.0), 1.0, 0.0, 90.0);
        let [(c1, c2, p)] = pieces.as_slice() else {
            panic!("one piece");
        };
        let k = 0.552_284_8;
        assert!(close(*c1, (1.0, k)));
        assert!(close(*c2, (k, 1.0)));
        assert!(close(*p, (0.0, 1.0)));
    }

    #[test]
    fn bezier_stays_on_the_circle() {
        let center = (3.0, -2.0);
        let radius = 50.0;
        for (start, sweep) in [(0.0, 90.0), (30.0, -75.0), (180.0, 360.0), (10.0, 200.0)] {
            let mut from = point_at(center, radius, start);
            for (c1, c2, p) in curves(center, radius, start, sweep) {
                for k in 1..8 {
                    let t = k as f64 / 8.0;
                    let u = 1.0 - t;
                    let at = |a: f64, b: f64, c: f64, d: f64| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                    };
                    let x = at(from.0, c1.0, c2.0, p.0) - center.0;
                    let y = at(from.1, c1.1, c2.1, p.1) - center.1;
                    let deviation = ((x * x + y * y).sqrt() - radius).abs();
                    assert!(deviation < 0.02, "{start} {sweep}: off by {deviation}");
                }
                from = p;
            }
            let end = point_at(center, radius, start + sweep);
            assert!(close(from, end), "{start} {sweep}");
        }
    }

    #[test]
    fn tangent_points_of_a_right_angle() {
        let t = tangent((0.0, 0.0), (100.0, 0.0), (100.0, 100.0), 10.0).unwrap();
        assert!(close(t.t1, (90.0, 0.0)));
        assert!(close(t.t2, (100.0, 10.0)));
        assert!((t.center.0 - 90.0).abs() < 1e-9 && (t.center.1 - 10.0).abs() < 1e-9);
        assert!((t.sweep - 90.0).abs() < 1e-3);
        // The mirror image turns right, so the arc runs clockwise.
        let t = tangent((0.0, 0.0), (100.0, 0.0), (100.0, -100.0), 10.0).unwrap();
        assert!((t.sweep + 90.0).abs() < 1e-3);
        assert!(tangent((0.0, 0.0), (1.0, 0.0), (2.0, 0.0), 1.0).is_none());
        assert!(tangent((0.0, 0.0), (0.0, 0.0), (2.0, 0.0), 1.0).is_none());
    }
}

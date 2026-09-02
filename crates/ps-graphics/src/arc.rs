// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Circular arcs as cubic Bézier curves. A sweep is cut into pieces of at
//! most a quarter turn, each approximated by one cubic whose control
//! points sit at `4/3·tan(θ/4)` radii along the tangents; the error of
//! that construction is far below the precision of an `f32` coordinate.

use ps_vm::Point;

/// One cubic piece: the two control points and the end point.
pub type Curve = (Point, Point, Point);

/// Degrees `end − start` swept counter-clockwise for `ccw`, otherwise
/// clockwise, with the end angle brought to the correct side of the start
/// angle the way `arc` and `arcn` do it. The result is signed: positive
/// for a counter-clockwise sweep.
pub fn sweep(start: f32, end: f32, ccw: bool) -> f64 {
    let (start, mut end) = (f64::from(start), f64::from(end));
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

/// The point at `degrees` on the circle.
pub fn point_at(center: Point, radius: f32, degrees: f64) -> Point {
    let (sin, cos) = sin_cos_deg(degrees);
    let r = f64::from(radius);
    Point::new(
        (f64::from(center.x) + r * cos) as f32,
        (f64::from(center.y) + r * sin) as f32,
    )
}

/// The Bézier pieces of the arc from `start` degrees sweeping `sweep`
/// degrees (signed, as [`sweep`] returns it). An empty sweep yields no
/// pieces.
pub fn curves(center: Point, radius: f32, start: f32, sweep: f64) -> Vec<Curve> {
    if sweep == 0.0 || !sweep.is_finite() {
        return Vec::new();
    }
    let pieces = (sweep.abs() / 90.0).ceil().max(1.0) as usize;
    let step = sweep / pieces as f64;
    let kappa = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    let (cx, cy, r) = (f64::from(center.x), f64::from(center.y), f64::from(radius));
    let mut out = Vec::with_capacity(pieces);
    let mut angle = f64::from(start);
    for _ in 0..pieces {
        let next = angle + step;
        let (s0, c0) = sin_cos_deg(angle);
        let (s1, c1) = sin_cos_deg(next);
        let p = |x: f64, y: f64| Point::new((cx + r * x) as f32, (cy + r * y) as f32);
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
    pub t1: Point,
    pub t2: Point,
    pub center: Point,
    pub start: f32,
    pub sweep: f64,
}

pub fn tangent(p0: Point, p1: Point, p2: Point, radius: f32) -> Option<Tangent> {
    let (x0, y0) = (f64::from(p0.x), f64::from(p0.y));
    let (x1, y1) = (f64::from(p1.x), f64::from(p1.y));
    let (x2, y2) = (f64::from(p2.x), f64::from(p2.y));
    let r = f64::from(radius);
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
    let t1 = Point::new((x1 + ux * d) as f32, (y1 + uy * d) as f32);
    let t2 = Point::new((x1 + vx * d) as f32, (y1 + vy * d) as f32);
    let (bx, by) = (ux + vx, uy + vy);
    let lb = (bx * bx + by * by).sqrt();
    let h = r / half.sin();
    let (cx, cy) = (x1 + bx / lb * h, y1 + by / lb * h);
    let center = Point::new(cx as f32, cy as f32);
    let start = (f64::from(t1.y) - cy)
        .atan2(f64::from(t1.x) - cx)
        .to_degrees();
    let end = (f64::from(t2.y) - cy)
        .atan2(f64::from(t2.x) - cx)
        .to_degrees();
    // A left turn is a counter-clockwise arc; `u` points back along the
    // incoming edge, so a left turn has a negative cross product.
    let sweep = sweep(start as f32, end as f32, cross < 0.0);
    Some(Tangent {
        t1,
        t2,
        center,
        start: start as f32,
        sweep,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Point, b: Point) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
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
        let c = curves(Point::new(0.0, 0.0), 10.0, 0.0, 360.0);
        assert_eq!(c.len(), 4);
        // Quarter-turn points are exact, not within rounding.
        assert_eq!(c[3].2, Point::new(10.0, 0.0));
        assert_eq!(c[0].2, Point::new(0.0, 10.0));
        assert_eq!(c[1].2, Point::new(-10.0, 0.0));
        assert_eq!(
            point_at(Point::new(1.0, 1.0), 2.0, -90.0),
            Point::new(1.0, -1.0)
        );
        assert!(curves(Point::new(0.0, 0.0), 10.0, 0.0, 0.0).is_empty());
        assert_eq!(curves(Point::new(0.0, 0.0), 10.0, 0.0, 91.0).len(), 2);
    }

    #[test]
    fn quarter_arc_control_points_use_the_standard_kappa() {
        let pieces = curves(Point::new(0.0, 0.0), 1.0, 0.0, 90.0);
        let [(c1, c2, p)] = pieces.as_slice() else {
            panic!("one piece");
        };
        let k = 0.552_284_8;
        assert!(close(*c1, Point::new(1.0, k)));
        assert!(close(*c2, Point::new(k, 1.0)));
        assert!(close(*p, Point::new(0.0, 1.0)));
    }

    #[test]
    fn bezier_stays_on_the_circle() {
        let center = Point::new(3.0, -2.0);
        let radius = 50.0;
        for (start, sweep) in [(0.0, 90.0), (30.0, -75.0), (180.0, 360.0), (10.0, 200.0)] {
            let mut from = point_at(center, radius, f64::from(start));
            for (c1, c2, p) in curves(center, radius, start, sweep) {
                for k in 1..8 {
                    let t = k as f32 / 8.0;
                    let u = 1.0 - t;
                    let at = |a: f32, b: f32, c: f32, d: f32| {
                        u * u * u * a + 3.0 * u * u * t * b + 3.0 * u * t * t * c + t * t * t * d
                    };
                    let x = at(from.x, c1.x, c2.x, p.x) - center.x;
                    let y = at(from.y, c1.y, c2.y, p.y) - center.y;
                    let deviation = ((x * x + y * y).sqrt() - radius).abs();
                    assert!(deviation < 0.02, "{start} {sweep}: off by {deviation}");
                }
                from = p;
            }
            let end = point_at(center, radius, f64::from(start) + sweep);
            assert!(close(from, end), "{start} {sweep}");
        }
    }

    #[test]
    fn tangent_points_of_a_right_angle() {
        let t = tangent(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, 100.0),
            10.0,
        )
        .unwrap();
        assert!(close(t.t1, Point::new(90.0, 0.0)));
        assert!(close(t.t2, Point::new(100.0, 10.0)));
        assert!(close(t.center, Point::new(90.0, 10.0)));
        assert!((t.sweep - 90.0).abs() < 1e-3);
        // The mirror image turns right, so the arc runs clockwise.
        let t = tangent(
            Point::new(0.0, 0.0),
            Point::new(100.0, 0.0),
            Point::new(100.0, -100.0),
            10.0,
        )
        .unwrap();
        assert!((t.sweep + 90.0).abs() < 1e-3);
        assert!(
            tangent(
                Point::new(0.0, 0.0),
                Point::new(1.0, 0.0),
                Point::new(2.0, 0.0),
                1.0
            )
            .is_none()
        );
        assert!(
            tangent(
                Point::new(0.0, 0.0),
                Point::new(0.0, 0.0),
                Point::new(2.0, 0.0),
                1.0
            )
            .is_none()
        );
    }
}

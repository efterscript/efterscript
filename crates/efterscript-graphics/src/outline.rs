// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The outline of a stroke (PLRM3 §8.2 `strokepath`, §4.5.1): closed
//! polygons that, filled with the nonzero rule, cover what `stroke`
//! would paint with the same line parameters.
//!
//! The path arrives in default user space, which is this backend's
//! device space, so the flatness tolerance — device pixels, per
//! `setflat` — applies to it directly: every curve is cut at its
//! midpoint until both control points lie within the tolerance of the
//! chord. The pen, however, is round in the *user space of the stroke*:
//! a non-uniform CTM makes it an ellipse on the page, and dash lengths
//! are user-space distances too. So each flattened subpath is taken
//! into that space through the inverse CTM, dashed and widened there,
//! and every polygon is brought back through the CTM. A singular CTM
//! has no such space and yields nothing, as its stroke marks nothing.
//!
//! The widening is a union of simple pieces rather than one contour per
//! run: a rectangle per segment, a wedge (mitre, arc, or triangle) on
//! the outer side of each corner, a half-disc or a square beyond each
//! open end, and a disc or square for a degenerate piece. Pieces
//! overlap freely; every polygon is oriented the same way, so the
//! nonzero rule paints their union and no inner-corner loop can cancel
//! a region. A round join at a corner where either segment is shorter
//! than half the line width becomes a whole disc, since the rectangles
//! then leave part of the disc uncovered.
//!
//! Degenerate subpaths follow the `stroke` entry: one that had a
//! segment but covers no distance is a dot with round caps and nothing
//! with the others; a subpath of one `moveto` is nothing. A dash of
//! zero length inside a segment has a direction, so it is a dot with
//! round caps and a square with projecting caps.

use std::f64::consts::PI;

use efterscript_vm::{LineCap, LineJoin, Matrix, Point, Seg, apply64};

use crate::state::{P64, narrow, widen};

/// The line parameters a stroke is measured with and the CTM it is
/// measured in.
#[derive(Clone, Debug, PartialEq)]
pub struct StrokeStyle {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    pub dash: (Vec<f32>, f32),
    /// In device pixels, already clamped to `setflat`'s range.
    pub flatness: f32,
    pub ctm: Matrix,
}

/// A flattened subpath in device space.
#[derive(Clone, Debug, PartialEq)]
struct Polyline {
    points: Vec<P64>,
    closed: bool,
    /// Whether anything followed the `moveto`; a lone `moveto` is not a
    /// subpath `stroke` paints.
    had_segment: bool,
}

/// One run of the pen after dashing, in user space.
#[derive(Clone, Debug, PartialEq)]
struct Piece {
    points: Vec<P64>,
    closed: bool,
    /// The direction of travel at a zero-length dash, which orients a
    /// projecting cap; a degenerate subpath has none.
    heading: Option<P64>,
}

/// Points closer than this, in user space, are one point.
const COINCIDENT: f64 = 1e-9;

/// The most times a curve is halved before the pieces are accepted.
const MAX_SPLITS: u32 = 16;

fn sub(a: P64, b: P64) -> P64 {
    (a.0 - b.0, a.1 - b.1)
}

fn add(a: P64, b: P64) -> P64 {
    (a.0 + b.0, a.1 + b.1)
}

fn scale(a: P64, s: f64) -> P64 {
    (a.0 * s, a.1 * s)
}

fn cross(a: P64, b: P64) -> f64 {
    a.0 * b.1 - a.1 * b.0
}

fn dot(a: P64, b: P64) -> f64 {
    a.0 * b.0 + a.1 * b.1
}

fn length(a: P64) -> f64 {
    dot(a, a).sqrt()
}

fn rotate(a: P64, radians: f64) -> P64 {
    let (s, c) = radians.sin_cos();
    (a.0 * c - a.1 * s, a.0 * s + a.1 * c)
}

/// The normal to the left of a direction of travel.
fn left(d: P64) -> P64 {
    (-d.1, d.0)
}

fn midpoint(a: P64, b: P64) -> P64 {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// Distance from `p` to the line through `a` and `b`, or to `a` when
/// the two coincide.
fn deviation(p: P64, a: P64, b: P64) -> f64 {
    let chord = sub(b, a);
    let len = length(chord);
    if len < COINCIDENT {
        length(sub(p, a))
    } else {
        cross(chord, sub(p, a)).abs() / len
    }
}

/// Appends the polyline of the cubic from `p0` (already in `out`) to
/// `p3`: halved until both control points are within `tolerance` of
/// the chord, whose convex hull then bounds the curve's deviation.
fn flatten_cubic(
    p0: P64,
    p1: P64,
    p2: P64,
    p3: P64,
    tolerance: f64,
    splits: u32,
    out: &mut Vec<P64>,
) {
    if splits >= MAX_SPLITS
        || (deviation(p1, p0, p3) <= tolerance && deviation(p2, p0, p3) <= tolerance)
    {
        out.push(p3);
        return;
    }
    let q0 = midpoint(p0, p1);
    let q1 = midpoint(p1, p2);
    let q2 = midpoint(p2, p3);
    let r0 = midpoint(q0, q1);
    let r1 = midpoint(q1, q2);
    let mid = midpoint(r0, r1);
    flatten_cubic(p0, q0, r0, mid, tolerance, splits + 1, out);
    flatten_cubic(mid, r1, q2, p3, tolerance, splits + 1, out);
}

/// The subpaths of `path` as polylines with their curves flattened.
fn polylines(path: &[Seg], tolerance: f64) -> Vec<Polyline> {
    let mut out: Vec<Polyline> = Vec::new();
    let mut current: Option<Polyline> = None;
    for seg in path {
        match *seg {
            Seg::Move(p) => {
                if let Some(line) = current.take() {
                    out.push(line);
                }
                current = Some(Polyline {
                    points: vec![widen(p)],
                    closed: false,
                    had_segment: false,
                });
            }
            Seg::Line(p) => {
                if let Some(line) = current.as_mut() {
                    line.points.push(widen(p));
                    line.had_segment = true;
                }
            }
            Seg::Curve(a, b, c) => {
                if let Some(line) = current.as_mut() {
                    let from = *line.points.last().expect("a subpath starts with a point");
                    flatten_cubic(
                        from,
                        widen(a),
                        widen(b),
                        widen(c),
                        tolerance,
                        0,
                        &mut line.points,
                    );
                    line.had_segment = true;
                }
            }
            Seg::Close => {
                if let Some(mut line) = current.take() {
                    line.closed = true;
                    line.had_segment = true;
                    let start = line.points[0];
                    out.push(line);
                    // Construction after a close continues from the
                    // subpath's start, as `closepath` leaves the current
                    // point there.
                    current = Some(Polyline {
                        points: vec![start],
                        closed: false,
                        had_segment: false,
                    });
                }
            }
        }
    }
    if let Some(line) = current {
        out.push(line);
    }
    out
}

/// `points` through `m` with runs of coincident points collapsed; a
/// closed run also drops a final point that repeats the first.
fn mapped_and_deduplicated(points: &[P64], closed: bool, m: [f64; 6]) -> Vec<P64> {
    let mut out: Vec<P64> = Vec::with_capacity(points.len());
    for &p in points {
        let q = apply64(m, p.0, p.1);
        if out
            .last()
            .is_none_or(|&last| length(sub(q, last)) >= COINCIDENT)
        {
            out.push(q);
        }
    }
    if closed && out.len() > 1 && length(sub(out[0], *out.last().expect("non-empty"))) < COINCIDENT
    {
        out.pop();
    }
    out
}

/// The dash pattern as a walker over its elements, started at the
/// phase: the state `stroke` reaches by cycling through the array
/// without marking anything until the phase's distance is used up.
struct DashWalker {
    lengths: Vec<f64>,
    index: usize,
    remaining: f64,
    on: bool,
}

impl DashWalker {
    fn new(lengths: &[f32], phase: f32) -> Self {
        let lengths: Vec<f64> = lengths.iter().map(|&l| f64::from(l).max(0.0)).collect();
        let total: f64 = lengths.iter().sum();
        let mut walker = DashWalker {
            remaining: lengths[0],
            lengths,
            index: 0,
            on: true,
        };
        // The pattern is cyclic, so any phase reduces to one within a
        // cycle; a negative one counts back from the cycle's end.
        let mut phase = if total > 0.0 {
            f64::from(phase).rem_euclid(total)
        } else {
            0.0
        };
        while phase > 0.0 {
            if walker.remaining <= 0.0 {
                walker.advance();
                continue;
            }
            let step = walker.remaining.min(phase);
            phase -= step;
            walker.remaining -= step;
            if walker.remaining <= 0.0 {
                walker.advance();
            }
        }
        walker
    }

    /// Moves to the next element, toggling between dash and gap.
    fn advance(&mut self) {
        self.index = (self.index + 1) % self.lengths.len();
        self.remaining = self.lengths[self.index];
        self.on = !self.on;
    }
}

/// The runs the dash pattern cuts `points` into; `dash` is non-empty and
/// not all zero, as `setdash` guarantees.
fn dashed(points: &[P64], closed: bool, dash: &(Vec<f32>, f32)) -> Vec<Piece> {
    let mut walker = DashWalker::new(&dash.0, dash.1);
    let mut pieces: Vec<Piece> = Vec::new();
    let mut run: Vec<P64> = Vec::new();
    // Whether the run started at the very start of the subpath, and
    // whether a gap has been met: a closed subpath whose start lies in
    // a dash has that dash continue round the corner.
    let started_at_origin = walker.on;
    let mut ever_off = !walker.on;
    let segments = if closed {
        points.len()
    } else {
        points.len() - 1
    };
    let finish = |run: &mut Vec<P64>, pieces: &mut Vec<Piece>, heading: P64| {
        if run.is_empty() {
            return;
        }
        let points = std::mem::take(run);
        let heading = (points.len() == 1).then_some(heading);
        pieces.push(Piece {
            points,
            closed: false,
            heading,
        });
    };
    for k in 0..segments {
        let a = points[k];
        let b = points[(k + 1) % points.len()];
        let chord = sub(b, a);
        let len = length(chord);
        let d = scale(chord, 1.0 / len);
        let mut travelled = 0.0;
        // Whether the run's last point lies on this segment and may be
        // slid along it rather than joined by another point.
        let mut sliding = false;
        loop {
            // Zero-length elements: a dash of no length is a dot, a gap
            // of no length nothing; not every element is zero.
            while walker.remaining <= 0.0 {
                if walker.on {
                    finish(&mut run, &mut pieces, d);
                    run.push(add(a, scale(d, travelled)));
                    finish(&mut run, &mut pieces, d);
                } else {
                    ever_off = true;
                }
                walker.advance();
                sliding = false;
            }
            if travelled >= len {
                break;
            }
            let step = walker.remaining.min(len - travelled);
            let to = travelled + step;
            if walker.on {
                if run.is_empty() {
                    run.push(add(a, scale(d, travelled)));
                }
                let end = if to >= len { b } else { add(a, scale(d, to)) };
                if sliding {
                    *run.last_mut().expect("a sliding run has a point") = end;
                } else {
                    run.push(end);
                    sliding = true;
                }
            } else {
                ever_off = true;
            }
            travelled = to;
            walker.remaining -= step;
            if walker.remaining <= 0.0 {
                if walker.on {
                    finish(&mut run, &mut pieces, d);
                }
                walker.advance();
                sliding = false;
            }
        }
    }
    let last_heading = {
        let a = points[points.len() - 2 + usize::from(closed)];
        let b = points[if closed { 0 } else { points.len() - 1 }];
        let chord = sub(b, a);
        scale(chord, 1.0 / length(chord))
    };
    let ended_on = walker.on && !run.is_empty();
    finish(&mut run, &mut pieces, last_heading);
    if closed && started_at_origin && ended_on {
        if !ever_off {
            // One dash longer than the whole subpath: a solid closed run.
            return vec![Piece {
                points: points.to_vec(),
                closed: true,
                heading: None,
            }];
        }
        if pieces.len() >= 2 {
            let last = pieces.pop().expect("two pieces");
            let first = pieces.remove(0);
            let mut points = last.points;
            points.extend_from_slice(&first.points[1..]);
            pieces.insert(
                0,
                Piece {
                    points,
                    closed: false,
                    heading: None,
                },
            );
        }
    }
    pieces
}

/// The pieces of a subpath: a run per dash, or the subpath itself.
fn pieces(points: Vec<P64>, closed: bool, dash: &(Vec<f32>, f32)) -> Vec<Piece> {
    if points.len() < 2 {
        return vec![Piece {
            points,
            closed,
            heading: None,
        }];
    }
    if dash.0.is_empty() {
        return vec![Piece {
            points,
            closed,
            heading: None,
        }];
    }
    dashed(&points, closed, dash)
}

/// How finely a circle of the pen's radius is cut so that its polygon
/// stays within the flatness tolerance on the page: the pen is drawn
/// in user space, so its radius there is stretched by the CTM's largest
/// scale factor before the tolerance applies.
fn circle_steps(half_width: f64, ctm: Matrix, tolerance: f64) -> usize {
    let [a, b, c, d, _, _] = ctm.as_f64();
    let sum = a * a + b * b + c * c + d * d;
    let det = a * d - b * c;
    let largest = ((sum + (sum * sum - 4.0 * det * det).max(0.0).sqrt()) / 2.0).sqrt();
    let radius = half_width * largest;
    if radius <= tolerance {
        return 4;
    }
    let step = (1.0 - tolerance / radius).acos();
    ((PI / step).ceil() as usize).clamp(4, 1024)
}

/// Builds the polygons of one piece in user space.
struct Widener {
    half_width: f64,
    cap: LineCap,
    join: LineJoin,
    miter_limit: f64,
    /// Points on a whole circle of the pen.
    circle_steps: usize,
    out: Vec<Vec<P64>>,
}

impl Widener {
    fn circle(&mut self, centre: P64) {
        let n = self.circle_steps;
        let poly = (0..n)
            .map(|i| {
                let angle = 2.0 * PI * i as f64 / n as f64;
                add(centre, scale((angle.cos(), angle.sin()), self.half_width))
            })
            .collect();
        self.out.push(poly);
    }

    /// The pen's arc around `centre` from the direction `from` (unit)
    /// through `sweep` radians, as the fan `[centre, arc points…]`.
    fn fan(&mut self, centre: P64, from: P64, sweep: f64) {
        let per_step = 2.0 * PI / self.circle_steps as f64;
        let k = ((sweep.abs() / per_step).ceil() as usize).max(1);
        let mut poly = Vec::with_capacity(k + 2);
        poly.push(centre);
        for i in 0..=k {
            let angle = sweep * i as f64 / k as f64;
            poly.push(add(centre, scale(rotate(from, angle), self.half_width)));
        }
        self.out.push(poly);
    }

    /// The rectangle of the segment `a → b` with unit direction `d`.
    fn rectangle(&mut self, a: P64, b: P64, d: P64) {
        let n = scale(left(d), self.half_width);
        self.out
            .push(vec![add(a, n), add(b, n), sub(b, n), sub(a, n)]);
    }

    /// The square beyond `end` in the outward direction `d`.
    fn square_cap(&mut self, end: P64, d: P64) {
        let n = scale(left(d), self.half_width);
        let f = scale(d, self.half_width);
        self.out.push(vec![
            add(end, n),
            add(add(end, n), f),
            add(sub(end, n), f),
            sub(end, n),
        ]);
    }

    fn cap(&mut self, end: P64, outward: P64) {
        match self.cap {
            LineCap::Butt => {}
            // From the left normal through the outward direction to the
            // right normal: a clockwise half turn.
            LineCap::Round => self.fan(end, left(outward), -PI),
            LineCap::Square => self.square_cap(end, outward),
        }
    }

    /// The corner at `p` between a segment arriving along `d_a` (of
    /// length `len_a`) and one leaving along `d_b` (of `len_b`).
    fn join(&mut self, p: P64, d_a: P64, len_a: f64, d_b: P64, len_b: f64) {
        let turn = cross(d_a, d_b);
        let along = dot(d_a, d_b);
        if turn.abs() < 1e-12 && along > 0.0 {
            return;
        }
        // The outer side of a left turn is the right; a reversal takes
        // the right so the arc sweeps ahead of the corner.
        let outer = if turn < 0.0 {
            left(d_a)
        } else {
            scale(left(d_a), -1.0)
        };
        let angle = turn.atan2(along);
        let n_a = scale(outer, self.half_width);
        let n_b = scale(rotate(outer, angle), self.half_width);
        match self.join {
            LineJoin::Round => {
                if len_a < self.half_width || len_b < self.half_width {
                    self.circle(p);
                } else {
                    self.fan(p, outer, angle);
                }
            }
            LineJoin::Miter => {
                // Mitre length over line width is 1/sin(φ/2) for the
                // angle φ between the segments; cos φ is −(d_a · d_b).
                let sin_half = ((1.0 + along) / 2.0).max(0.0).sqrt();
                if sin_half > 0.0 && 1.0 / sin_half <= self.miter_limit {
                    let tip = add(p, scale(add(n_a, n_b), 1.0 / (1.0 + along)));
                    self.out.push(vec![p, add(p, n_a), tip, add(p, n_b)]);
                } else {
                    self.out.push(vec![p, add(p, n_a), add(p, n_b)]);
                }
            }
            LineJoin::Bevel => self.out.push(vec![p, add(p, n_a), add(p, n_b)]),
        }
    }

    fn piece(&mut self, piece: &Piece) {
        let points = &piece.points;
        if points.len() < 2 {
            let Some(&p) = points.first() else {
                return;
            };
            match (self.cap, piece.heading) {
                (LineCap::Round, _) => self.circle(p),
                (LineCap::Square, Some(d)) => {
                    let reach = scale(d, self.half_width);
                    self.rectangle(sub(p, reach), add(p, reach), d);
                }
                _ => {}
            }
            return;
        }
        let count = if piece.closed {
            points.len()
        } else {
            points.len() - 1
        };
        let mut directions = Vec::with_capacity(count);
        let mut lengths = Vec::with_capacity(count);
        for k in 0..count {
            let a = points[k];
            let b = points[(k + 1) % points.len()];
            let chord = sub(b, a);
            let len = length(chord);
            directions.push(scale(chord, 1.0 / len));
            lengths.push(len);
            self.rectangle(a, b, directions[k]);
        }
        let first_corner = if piece.closed { 0 } else { 1 };
        for v in first_corner..count {
            let before = (v + count - 1) % count;
            self.join(
                points[v],
                directions[before],
                lengths[before],
                directions[v],
                lengths[v],
            );
        }
        if !piece.closed {
            self.cap(points[0], scale(directions[0], -1.0));
            self.cap(points[count], directions[count - 1]);
        }
    }
}

/// `polygon` through `m`, oriented counter-clockwise on the page so
/// that every polygon adds to the nonzero winding.
fn to_device(polygon: &[P64], m: [f64; 6]) -> Vec<Point> {
    let mut mapped: Vec<P64> = polygon.iter().map(|p| apply64(m, p.0, p.1)).collect();
    let area: f64 = mapped
        .iter()
        .zip(mapped.iter().cycle().skip(1))
        .map(|(a, b)| cross(*a, *b))
        .sum();
    if area < 0.0 {
        mapped.reverse();
    }
    mapped.into_iter().map(narrow).collect()
}

/// The outline of `path` (default user space) stroked with `style`, as
/// closed subpaths in default user space for a nonzero fill.
pub fn outline(path: &[Seg], style: &StrokeStyle) -> Vec<Seg> {
    let Some(to_user) = style.ctm.inverse64() else {
        return Vec::new();
    };
    let to_page = style.ctm.as_f64();
    let tolerance = f64::from(style.flatness);
    let half_width = f64::from(style.width.abs()) / 2.0;
    let mut widener = Widener {
        half_width,
        cap: style.cap,
        join: style.join,
        miter_limit: f64::from(style.miter_limit),
        circle_steps: circle_steps(half_width, style.ctm, tolerance),
        out: Vec::new(),
    };
    for line in polylines(path, tolerance) {
        if !line.had_segment {
            continue;
        }
        let points = mapped_and_deduplicated(&line.points, line.closed, to_user);
        for piece in pieces(points, line.closed, &style.dash) {
            widener.piece(&piece);
        }
    }
    let mut segs = Vec::new();
    for polygon in &widener.out {
        let mut corners = to_device(polygon, to_page).into_iter();
        let Some(first) = corners.next() else {
            continue;
        };
        segs.push(Seg::Move(first));
        segs.extend(corners.map(Seg::Line));
        segs.push(Seg::Close);
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flat_curve_is_one_chord_and_a_bent_one_is_split() {
        let mut out = vec![(0.0, 0.0)];
        flatten_cubic(
            (0.0, 0.0),
            (1.0, 0.1),
            (2.0, 0.1),
            (3.0, 0.0),
            1.0,
            0,
            &mut out,
        );
        assert_eq!(out.len(), 2);
        let mut out = vec![(0.0, 0.0)];
        flatten_cubic(
            (0.0, 0.0),
            (0.0, 50.0),
            (50.0, 50.0),
            (50.0, 0.0),
            1.0,
            0,
            &mut out,
        );
        assert!(out.len() > 4, "{}", out.len());
        // Every accepted chord stays within the tolerance of its curve's
        // hull, so the polyline never strays far from the arch.
        assert!(out.iter().all(|p| p.1 <= 37.5 + 1.0));
    }

    #[test]
    fn dash_phase_starts_inside_the_pattern() {
        let walker = DashWalker::new(&[3.0, 1.0], 0.0);
        assert!(walker.on && walker.remaining == 3.0);
        let walker = DashWalker::new(&[3.0, 1.0], 3.5);
        assert!(!walker.on && (walker.remaining - 0.5).abs() < 1e-12);
        let walker = DashWalker::new(&[2.0], 5.0);
        assert!(walker.on && (walker.remaining - 1.0).abs() < 1e-12);
        let walker = DashWalker::new(&[2.0, 3.0], -1.0);
        assert!(!walker.on && (walker.remaining - 1.0).abs() < 1e-12);
    }

    #[test]
    fn circle_steps_follow_the_page_radius() {
        assert_eq!(circle_steps(0.1, Matrix::IDENTITY, 1.0), 4);
        let coarse = circle_steps(10.0, Matrix::IDENTITY, 1.0);
        let fine = circle_steps(10.0, Matrix::IDENTITY, 0.2);
        assert!(coarse < fine);
        let stretched = circle_steps(10.0, Matrix::scaling(1.0, 4.0), 1.0);
        assert_eq!(stretched, circle_steps(40.0, Matrix::IDENTITY, 1.0));
    }

    #[test]
    fn polygons_come_out_counter_clockwise() {
        let clockwise = [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0), (1.0, 0.0)];
        let mapped = to_device(&clockwise, Matrix::IDENTITY.as_f64());
        assert_eq!(mapped[0], Point::new(1.0, 0.0));
        let flipped = to_device(&clockwise, Matrix::scaling(1.0, -1.0).as_f64());
        assert_eq!(flipped[0], Point::new(0.0, 0.0));
    }
}

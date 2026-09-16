// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The graphics state (PLRM3 §4.2) and the current path it contains.
//!
//! Path points are transformed through the CTM when they are added, so a
//! stored path is already in default user space and a later CTM change
//! leaves it where it is. The state stack is a plain `Vec<GState>`; the
//! path's segment vector and each clip entry's path are reference-counted
//! and copied on first write after a `gsave`, so saving a state costs a
//! few reference bumps whatever the path size.

use std::rc::Rc;

use ps_vm::{
    Bounds, CieColor, FontRef, LineCap, LineJoin, Matrix, PatternInfo, Point, ProcRef, Rect,
    Screen, Seg, SpaceSpec, VmError,
};

/// The inside rule of a fill or clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

/// One intersection applied to the clip since the last `initclip`.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipEntry {
    /// Distinguishes entries with identical geometry, so the emitter can
    /// tell whether a clip already open in the IR is still in effect.
    pub id: u64,
    pub rule: FillRule,
    /// In default user space.
    pub path: Rc<Vec<Seg>>,
}

/// The current path: segments in default user space plus the points the
/// construction operators need.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path {
    pub segs: Rc<Vec<Seg>>,
    /// Device-space current point, `None` when the path is empty.
    pub current: Option<Point>,
    /// Where the current subpath began, for `closepath`.
    pub start: Option<Point>,
}

impl Path {
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    fn push(&mut self, seg: Seg) {
        Rc::make_mut(&mut self.segs).push(seg);
    }

    /// Requires a current point, the way every operator but `moveto` does.
    pub fn current(&self) -> Result<Point, VmError> {
        self.current.ok_or(VmError::NoCurrentPoint)
    }

    /// Starts a subpath; a `Move` with no segment after it is replaced
    /// rather than left as a one-point subpath (PLRM3 §4.4).
    pub fn move_to(&mut self, p: Point) {
        match Rc::make_mut(&mut self.segs).last_mut() {
            Some(last @ Seg::Move(_)) => *last = Seg::Move(p),
            _ => self.push(Seg::Move(p)),
        }
        self.current = Some(p);
        self.start = Some(p);
    }

    pub fn line_to(&mut self, p: Point) -> Result<(), VmError> {
        self.current()?;
        self.push(Seg::Line(p));
        self.current = Some(p);
        Ok(())
    }

    pub fn curve_to(&mut self, c1: Point, c2: Point, p: Point) -> Result<(), VmError> {
        self.current()?;
        self.push(Seg::Curve(c1, c2, p));
        self.current = Some(p);
        Ok(())
    }

    /// Closes the current subpath; nothing happens on an empty path or
    /// after a close.
    pub fn close(&mut self) {
        if self.current.is_none() || matches!(self.segs.last(), Some(Seg::Close)) {
            return;
        }
        self.push(Seg::Close);
        self.current = self.start;
    }

    /// A path over ready-made segments, with the current point and
    /// subpath start recovered from them.
    pub fn from_segments(segs: Vec<Seg>) -> Path {
        let mut path = Path::default();
        for seg in &segs {
            match *seg {
                Seg::Move(p) => {
                    path.current = Some(p);
                    path.start = Some(p);
                }
                Seg::Line(p) | Seg::Curve(_, _, p) => path.current = Some(p),
                Seg::Close => path.current = path.start,
            }
        }
        path.segs = segs.into();
        path
    }

    /// Every point of the path, control points included.
    pub fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.segs.iter().flat_map(|seg| match *seg {
            Seg::Move(p) | Seg::Line(p) => vec![p],
            Seg::Curve(a, b, c) => vec![a, b, c],
            Seg::Close => Vec::new(),
        })
    }
}

/// Segments of a rectangle, already transformed.
pub fn rect_segments(ctm: Matrix, rect: &Rect) -> Vec<Seg> {
    let corner = |x, y| ctm.apply(Point::new(x, y));
    vec![
        Seg::Move(corner(rect.x, rect.y)),
        Seg::Line(corner(rect.x + rect.width, rect.y)),
        Seg::Line(corner(rect.x + rect.width, rect.y + rect.height)),
        Seg::Line(corner(rect.x, rect.y + rect.height)),
        Seg::Close,
    ]
}

/// Segments of a box given by its corners, untransformed.
pub fn bounds_segments(b: Bounds) -> Vec<Seg> {
    vec![
        Seg::Move(Point::new(b.llx, b.lly)),
        Seg::Line(Point::new(b.urx, b.lly)),
        Seg::Line(Point::new(b.urx, b.ury)),
        Seg::Line(Point::new(b.llx, b.ury)),
        Seg::Close,
    ]
}

/// The lowest flatness `setflat` accepts; larger and smaller requests are
/// clamped, per PLRM3 §8.2 `setflat`.
pub const MIN_FLATNESS: f32 = 0.2;
pub const MAX_FLATNESS: f32 = 100.0;

pub const DEFAULT_MEDIA_BOX: Bounds = Bounds::new(0.0, 0.0, 612.0, 792.0);

#[derive(Clone, Debug, PartialEq)]
pub struct GState {
    pub ctm: Matrix,
    pub space: SpaceSpec,
    pub color: Vec<f32>,
    /// The pattern instance the colour is, when `space` is a pattern
    /// space and one has been set; `color` then holds the components of
    /// the underlying space (none for a coloured pattern). A pattern
    /// space without an instance is the initial null colour of PLRM3
    /// §4.9.1, which paints nothing.
    pub pattern: Option<PatternInfo>,
    /// What the VM attached to the colour while a CIE-based space is
    /// current; cleared when the space changes, kept otherwise.
    pub cie: Option<CieColor>,
    pub line_width: f32,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f32,
    pub dash: (Vec<f32>, f32),
    pub flatness: f32,
    /// Intersections since `initclip`, oldest first; empty means the
    /// page is the clip.
    pub clip: Vec<ClipEntry>,
    pub media_box: Bounds,
    pub path: Path,
    /// Whether the null device is installed: marks are discarded and page
    /// operators do nothing until a state without it is restored.
    pub null_device: bool,
    /// The current font, `None` until `setfont`.
    pub font: Option<FontRef>,
    /// The red, green, blue, and gray halftone screens, recorded for the
    /// getters and never applied.
    pub screens: [Screen; 4],
    /// The transfer functions, likewise.
    pub transfers: [ProcRef; 4],
    /// The colour rendering dictionary, likewise; `None` is the default.
    pub color_rendering: Option<ProcRef>,
}

impl Default for GState {
    fn default() -> Self {
        GState {
            ctm: Matrix::IDENTITY,
            space: SpaceSpec::DeviceGray,
            color: vec![0.0],
            pattern: None,
            cie: None,
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: (Vec::new(), 0.0),
            flatness: 1.0,
            clip: Vec::new(),
            media_box: DEFAULT_MEDIA_BOX,
            path: Path::default(),
            null_device: false,
            font: None,
            screens: [Screen::DEFAULT; 4],
            transfers: [ProcRef::IDENTITY; 4],
            color_rendering: None,
        }
    }
}

impl GState {
    /// What `initgraphics` leaves: the defaults with the device untouched
    /// (media box and null device kept) and the font, screens, transfer
    /// functions, and colour rendering kept, since `initgraphics` and
    /// `showpage` do not reset them (PLRM3 §8.2).
    pub fn reinitialized(&self) -> GState {
        GState {
            media_box: self.media_box,
            null_device: self.null_device,
            font: self.font,
            screens: self.screens,
            transfers: self.transfers,
            color_rendering: self.color_rendering,
            ..GState::default()
        }
    }

    pub fn inverse_ctm(&self) -> Result<Matrix, VmError> {
        self.ctm.inverse().ok_or(VmError::UndefinedResult)
    }

    /// The current point in the current user space.
    pub fn current_point(&self) -> Result<Point, VmError> {
        let device = self.path.current()?;
        Ok(self.inverse_ctm()?.apply(device))
    }

    /// The user-space bounding box of every point of the current path.
    pub fn path_bbox(&self) -> Result<Bounds, VmError> {
        self.path.current()?;
        let inverse = self.inverse_ctm()?;
        let mut bounds: Option<Bounds> = None;
        for p in self.path.points().map(|p| inverse.apply(p)) {
            bounds = Some(match bounds {
                None => Bounds::new(p.x, p.y, p.x, p.y),
                Some(b) => Bounds::new(
                    b.llx.min(p.x),
                    b.lly.min(p.y),
                    b.urx.max(p.x),
                    b.ury.max(p.y),
                ),
            });
        }
        bounds.ok_or(VmError::NoCurrentPoint)
    }

    pub fn set_color(&mut self, components: &[f32]) -> Result<(), VmError> {
        if components.len() != self.space.components() {
            return Err(VmError::RangeCheck);
        }
        self.color = self.clamped(components);
        self.pattern = None;
        Ok(())
    }

    /// Makes `pattern` the colour: an uncoloured pattern takes exactly
    /// the underlying space's components, a coloured one none, whatever
    /// the space's base.
    pub fn set_pattern(
        &mut self,
        pattern: &PatternInfo,
        components: &[f32],
    ) -> Result<(), VmError> {
        let expected = if pattern.paint_type == 2 {
            self.space.components()
        } else {
            0
        };
        if components.len() != expected {
            return Err(VmError::RangeCheck);
        }
        self.color = self.clamped(components);
        self.pattern = Some(*pattern);
        Ok(())
    }

    /// Whether the colour is a pattern space's initial null: painting
    /// with it makes no marks.
    pub fn paints_nothing(&self) -> bool {
        matches!(self.space, SpaceSpec::Pattern { .. }) && self.pattern.is_none()
    }

    fn clamped(&self, components: &[f32]) -> Vec<f32> {
        components
            .iter()
            .enumerate()
            .map(|(k, &c)| {
                let (lo, hi) = self.space.component_limits(k);
                if c.is_nan() {
                    lo
                } else {
                    c.clamp(lo, hi.max(lo))
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_tracks_current_point_and_start() {
        let mut path = Path::default();
        assert_eq!(path.current(), Err(VmError::NoCurrentPoint));
        assert_eq!(
            path.line_to(Point::new(1.0, 1.0)),
            Err(VmError::NoCurrentPoint)
        );
        path.close();
        assert!(path.is_empty());
        path.move_to(Point::new(1.0, 2.0));
        path.line_to(Point::new(3.0, 4.0)).unwrap();
        path.close();
        path.close();
        assert_eq!(path.current(), Ok(Point::new(1.0, 2.0)));
        assert_eq!(path.segs.len(), 3);
        assert_eq!(path.points().count(), 2);
    }

    #[test]
    fn gsave_shares_segments_until_written() {
        let mut state = GState::default();
        state.path.move_to(Point::new(0.0, 0.0));
        let saved = state.clone();
        assert!(Rc::ptr_eq(&saved.path.segs, &state.path.segs));
        state.path.line_to(Point::new(5.0, 5.0)).unwrap();
        assert!(!Rc::ptr_eq(&saved.path.segs, &state.path.segs));
        assert_eq!(saved.path.segs.len(), 1);
        assert_eq!(state.path.segs.len(), 2);
    }

    #[test]
    fn colour_is_clamped_and_checked() {
        let mut state = GState::default();
        assert_eq!(state.set_color(&[1.5]), Ok(()));
        assert_eq!(state.color, vec![1.0]);
        assert_eq!(state.set_color(&[0.5, 0.5]), Err(VmError::RangeCheck));
        state.space = SpaceSpec::Indexed {
            base: Box::new(SpaceSpec::DeviceRGB),
            hival: 3,
            lookup: vec![0; 12],
        };
        state.set_color(&[7.0]).unwrap();
        assert_eq!(state.color, vec![3.0]);
        state.space = SpaceSpec::Lab {
            white: [0.95, 1.0, 1.07],
            black: [0.0; 3],
            range: [-50.0, 50.0, -20.0, 20.0],
        };
        state.set_color(&[150.0, -80.0, f32::NAN]).unwrap();
        assert_eq!(state.color, vec![100.0, -50.0, -20.0]);
    }

    #[test]
    fn the_vm_colour_survives_a_colour_change_but_not_a_space_change() {
        let mut state = GState::default();
        let attached = CieColor {
            space: 3,
            components: [0.5, 0.0, 0.0, 0.0],
        };
        state.cie = Some(attached);
        state.color_rendering = Some(ProcRef(2));
        state.set_color(&[0.25]).unwrap();
        assert_eq!(state.cie, Some(attached));
        let again = state.reinitialized();
        assert_eq!(again.cie, None);
        assert_eq!(again.color_rendering, Some(ProcRef(2)));
    }

    #[test]
    fn a_pattern_takes_the_base_components_and_a_numeric_colour_clears_it() {
        let coloured = PatternInfo {
            id: 1,
            matrix: Matrix::IDENTITY,
            bbox: Bounds::new(0.0, 0.0, 1.0, 1.0),
            xstep: 1.0,
            ystep: 1.0,
            paint_type: 1,
            tiling_type: 1,
        };
        let uncoloured = PatternInfo {
            paint_type: 2,
            ..coloured
        };
        let mut state = GState {
            space: SpaceSpec::Pattern {
                base: Some(Box::new(SpaceSpec::DeviceRGB)),
            },
            ..GState::default()
        };
        assert!(state.paints_nothing());
        assert_eq!(
            state.set_pattern(&coloured, &[1.0]),
            Err(VmError::RangeCheck)
        );
        assert_eq!(state.set_pattern(&coloured, &[]), Ok(()));
        assert_eq!(state.color, Vec::<f32>::new());
        assert!(!state.paints_nothing());
        assert_eq!(
            state.set_pattern(&uncoloured, &[]),
            Err(VmError::RangeCheck)
        );
        assert_eq!(state.set_pattern(&uncoloured, &[2.0, 0.5, -1.0]), Ok(()));
        assert_eq!(state.color, vec![1.0, 0.5, 0.0]);
        assert_eq!(state.pattern, Some(uncoloured));
        state.set_color(&[0.0, 0.0, 0.0]).unwrap();
        assert_eq!(state.pattern, None);
        state.space = SpaceSpec::Pattern { base: None };
        assert_eq!(state.set_pattern(&uncoloured, &[]), Ok(()));
        assert!(!GState::default().paints_nothing());
    }

    #[test]
    fn bbox_is_in_user_space_with_control_points() {
        let mut state = GState {
            ctm: Matrix::translation(10.0, 10.0),
            ..GState::default()
        };
        state.path.move_to(Point::new(10.0, 10.0));
        state
            .path
            .curve_to(
                Point::new(50.0, 90.0),
                Point::new(20.0, 20.0),
                Point::new(30.0, 30.0),
            )
            .unwrap();
        assert_eq!(state.path_bbox(), Ok(Bounds::new(0.0, 0.0, 40.0, 80.0)));
        assert_eq!(state.current_point(), Ok(Point::new(20.0, 20.0)));
        state.ctm = Matrix::scaling(0.0, 1.0);
        assert_eq!(state.current_point(), Err(VmError::UndefinedResult));
    }
}

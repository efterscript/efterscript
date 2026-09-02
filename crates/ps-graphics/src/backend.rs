// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The [`GraphicsBackend`] implementation: a graphics-state stack, the
//! page being built, and the emitter that turns paints into IR.
//!
//! Emission is lazy. Nothing is recorded while state changes or paths are
//! built; at a paint the emitter compares the settings the paint depends
//! on with what the IR last set and records only the differences, then
//! the paint with its path. Clips are handled the same way: the clip in
//! effect is a list of entries, and at a paint the emitter opens the
//! entries not yet open in the IR (`Save` then `Clip`) and closes those
//! no longer in effect (`Restore`), keeping a stack of "last set" states
//! so a `Restore` also restores what the IR knows about line width and
//! colour, as PDF's `Q` does. A `gsave`/`grestore` pair that paints
//! nothing therefore leaves no trace.
//!
//! A Type 3 glyph is captured by redirecting emission: between
//! `begin_glyph` and `end_glyph` the page's operation list is swapped for
//! the glyph's, geometry is taken back through the CTM in effect at
//! `begin_glyph` so the procedure is in glyph space, the emitter starts
//! from the state the glyph inherits (so only the glyph's own settings are
//! recorded), and only clips established inside the glyph appear in it.

use std::collections::{BTreeMap, HashMap};

use ps_vm::{
    Bounds, FontInfo, FontRef, FontSource, Glyph, GraphicsBackend, ImageSpec, LineCap, LineJoin,
    Matrix, Point, Rect, Seg, SpaceSpec, VmError,
};

use crate::arc;
use crate::ir::{FillRule, FontIndex, FontSpec, GlyphProc, IrOp, Op, Page, PageSink, glyph_names};
use crate::state::{ClipEntry, GState, MAX_FLATNESS, MIN_FLATNESS, Path, rect_segments};

/// What the IR last set, tracked per open `Save`.
#[derive(Clone, Debug, PartialEq)]
struct Emitted {
    line_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    miter_limit: f32,
    dash: (Vec<f32>, f32),
    flatness: f32,
    space: SpaceSpec,
    color: Vec<f32>,
    /// Whether the IR has named the colour space itself. A captured glyph
    /// inherits its space unnamed; a colour set inside it then needs the
    /// space named first, so the procedure stands on its own.
    space_known: bool,
}

impl Emitted {
    /// The settings of `state`, as a glyph procedure inherits them.
    fn of(state: &GState) -> Self {
        Emitted {
            line_width: state.line_width,
            line_cap: state.line_cap,
            line_join: state.line_join,
            miter_limit: state.miter_limit,
            dash: state.dash.clone(),
            flatness: state.flatness,
            space: state.space.clone(),
            color: state.color.clone(),
            space_known: true,
        }
    }
}

impl Default for Emitted {
    /// PDF's initial graphics state, which the IR assumes at page start.
    fn default() -> Self {
        Emitted::of(&GState::default())
    }
}

/// Which settings a paint depends on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Needs {
    /// A fill: colour and flatness.
    Fill,
    /// A stroke: colour, flatness, and the line parameters.
    Stroke,
    /// A mask: colour only.
    Color,
    /// An image: nothing beyond the clip.
    Nothing,
}

#[derive(Debug)]
struct Emitter {
    /// One entry per open `Save` plus the base; the last is current.
    states: Vec<Emitted>,
    /// Ids of the clip entries open in the IR, outermost first.
    open: Vec<u64>,
    /// Clip entries below this index belong to the enclosing page or
    /// glyph and are not this emitter's to open.
    clip_floor: usize,
}

impl Emitter {
    fn new() -> Self {
        Emitter {
            states: vec![Emitted::default()],
            open: Vec::new(),
            clip_floor: 0,
        }
    }

    /// An emitter for a glyph procedure begun in `state`.
    fn for_glyph(state: &GState) -> Self {
        Emitter {
            states: vec![Emitted {
                space_known: false,
                ..Emitted::of(state)
            }],
            open: Vec::new(),
            clip_floor: state.clip.len(),
        }
    }

    fn current(&mut self) -> &mut Emitted {
        self.states
            .last_mut()
            .expect("the base state is never popped")
    }
}

/// A glyph procedure being captured: where emission went before it began
/// and how to bring geometry into glyph space.
struct Capture {
    font: FontRef,
    name: Vec<u8>,
    measure: bool,
    /// Default user space to glyph space, kept in double precision so the
    /// round trip through the CTM leaves no residue; `None` when the
    /// glyph's CTM was singular, in which case nothing can be kept.
    to_glyph: Option<[f64; 6]>,
    outer_ops: Vec<Op>,
    outer_emitter: Emitter,
}

pub struct Graphics<S> {
    sink: S,
    stack: Vec<GState>,
    gstate: GState,
    page: Page,
    emitter: Emitter,
    next_clip_id: u64,
    /// What the VM said each font instance is.
    fonts: HashMap<u32, FontInfo>,
    /// The page resource each instance shown on this page resolved to.
    page_fonts: HashMap<u32, FontIndex>,
    /// The Type 3 resources of this page by font family (`FID`).
    type3: Vec<(u32, FontIndex)>,
    captures: Vec<Capture>,
}

impl<S: PageSink> Graphics<S> {
    pub fn new(sink: S) -> Self {
        let gstate = GState::default();
        Graphics {
            sink,
            stack: Vec::new(),
            page: Page::new(gstate.media_box),
            gstate,
            emitter: Emitter::new(),
            next_clip_id: 0,
            fonts: HashMap::new(),
            page_fonts: HashMap::new(),
            type3: Vec::new(),
            captures: Vec::new(),
        }
    }

    pub fn sink(&self) -> &S {
        &self.sink
    }

    pub fn sink_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    /// The current graphics state, for inspection.
    pub fn state(&self) -> &GState {
        &self.gstate
    }

    /// The operations recorded for the page under construction.
    pub fn ops(&self) -> &[Op] {
        &self.page.ops
    }

    /// Records an operation where emission currently goes: the page, or
    /// the glyph procedure being captured, whose geometry is taken into
    /// glyph space on the way. Nothing is kept for a glyph whose CTM had
    /// no inverse.
    fn record(&mut self, op: IrOp) {
        let op = match self.captures.last() {
            None => op,
            Some(capture) => match capture.to_glyph {
                Some(to_glyph) => transformed(op, to_glyph),
                None => return,
            },
        };
        self.page.ops.push(op.into());
    }

    fn device(&self, p: Point) -> Point {
        self.gstate.ctm.apply(p)
    }

    // --- emission -----------------------------------------------------------------

    /// Opens and closes IR clips so the open set matches the clip in
    /// effect above the emitter's floor.
    fn sync_clip(&mut self) {
        let floor = self.emitter.clip_floor;
        let in_effect: &[ClipEntry] = self.gstate.clip.get(floor..).unwrap_or(&[]);
        let common = self
            .emitter
            .open
            .iter()
            .zip(in_effect)
            .take_while(|(open, entry)| **open == entry.id)
            .count();
        let opening: Vec<ClipEntry> = in_effect[common..].to_vec();
        while self.emitter.open.len() > common {
            self.emitter.open.pop();
            self.emitter.states.pop();
            self.record(IrOp::Restore);
        }
        for entry in opening {
            let saved = self.emitter.current().clone();
            self.emitter.states.push(saved);
            self.emitter.open.push(entry.id);
            self.record(IrOp::Save);
            self.record(IrOp::Clip {
                path: (*entry.path).clone(),
                rule: entry.rule,
            });
        }
    }

    /// Closes every open IR clip, as a page or a glyph ends.
    fn close_all(&mut self) {
        while self.emitter.open.pop().is_some() {
            self.emitter.states.pop();
            self.record(IrOp::Restore);
        }
    }

    /// Records the settings the paint needs that differ from what the IR
    /// last set.
    fn flush(&mut self, needs: Needs) {
        if needs == Needs::Nothing {
            return;
        }
        let state = &self.gstate;
        let mut ops = Vec::new();
        {
            let emitted = self.emitter.current();
            if state.space != emitted.space {
                let index = self.page.resources.intern_space(&state.space);
                ops.push(IrOp::SetColorSpace(index));
                emitted.space = state.space.clone();
                emitted.color = state.space.initial_color();
                emitted.space_known = true;
            }
            if state.color != emitted.color {
                if !emitted.space_known {
                    let index = self.page.resources.intern_space(&state.space);
                    ops.push(IrOp::SetColorSpace(index));
                    emitted.space_known = true;
                }
                ops.push(IrOp::SetColor(state.color.clone()));
                emitted.color = state.color.clone();
            }
            if matches!(needs, Needs::Fill | Needs::Stroke) && state.flatness != emitted.flatness {
                ops.push(IrOp::Flatness(state.flatness));
                emitted.flatness = state.flatness;
            }
            if needs == Needs::Stroke {
                if state.line_width != emitted.line_width {
                    ops.push(IrOp::LineWidth(state.line_width));
                    emitted.line_width = state.line_width;
                }
                if state.line_cap != emitted.line_cap {
                    ops.push(IrOp::LineCap(state.line_cap));
                    emitted.line_cap = state.line_cap;
                }
                if state.line_join != emitted.line_join {
                    ops.push(IrOp::LineJoin(state.line_join));
                    emitted.line_join = state.line_join;
                }
                if state.miter_limit != emitted.miter_limit {
                    ops.push(IrOp::MiterLimit(state.miter_limit));
                    emitted.miter_limit = state.miter_limit;
                }
                if state.dash != emitted.dash {
                    ops.push(IrOp::Dash(state.dash.0.clone(), state.dash.1));
                    emitted.dash = state.dash.clone();
                }
            }
        }
        for op in ops {
            self.record(op);
        }
    }

    /// Paints `path` (already in default user space) with the current
    /// settings. The current path is not touched.
    fn paint(&mut self, path: Vec<Seg>, make: impl FnOnce(Vec<Seg>, Matrix) -> IrOp, needs: Needs) {
        if self.gstate.null_device || path.is_empty() {
            return;
        }
        self.sync_clip();
        self.flush(needs);
        let op = make(path, self.gstate.ctm);
        self.record(op);
    }

    // --- fonts ---------------------------------------------------------------------

    /// The page resource for the current font instance, interned on first
    /// use: a resident font by base and encoding, a Type 3 font by family
    /// and encoding.
    fn font_resource(&mut self, font: FontRef) -> Result<FontIndex, VmError> {
        if let Some(&index) = self.page_fonts.get(&font.instance) {
            return Ok(index);
        }
        let info = self.fonts.get(&font.instance).ok_or(VmError::InvalidFont)?;
        let encoding = glyph_names(&info.encoding);
        let index = match info.source {
            FontSource::Resident(base) => self
                .page
                .resources
                .intern_font(FontSpec::Resident { base, encoding }),
            FontSource::Type3 {
                family,
                font_matrix,
                font_bbox,
            } => {
                let known = self.type3.iter().find(|(f, index)| {
                    *f == family && *self.page.resources.fonts[index.0].encoding() == encoding
                });
                match known {
                    Some(&(_, index)) => index,
                    None => {
                        let index = self.page.resources.add_font(FontSpec::Type3 {
                            font_matrix,
                            font_bbox,
                            encoding,
                            glyphs: BTreeMap::new(),
                        });
                        self.type3.push((family, index));
                        index
                    }
                }
            }
        };
        self.page_fonts.insert(font.instance, index);
        Ok(index)
    }

    /// Whether the current font is a Type 3 font, whose text depends on
    /// every setting its glyph procedures inherit.
    fn is_type3(&self, font: FontRef) -> bool {
        matches!(
            self.fonts.get(&font.instance),
            Some(FontInfo {
                source: FontSource::Type3 { .. },
                ..
            })
        )
    }

    /// Starts a page: the operations and the per-page font tables.
    fn reset_page(&mut self, media_box: Bounds) {
        self.page = Page::new(media_box);
        self.emitter = Emitter::new();
        self.page_fonts.clear();
        self.type3.clear();
    }

    /// Page operations are refused while a glyph is being captured.
    fn page_operation(&self) -> Result<(), VmError> {
        if self.captures.is_empty() {
            Ok(())
        } else {
            Err(VmError::InvalidAccess)
        }
    }

    /// Paints the current path and empties it.
    fn paint_current(&mut self, make: impl FnOnce(Vec<Seg>, Matrix) -> IrOp, needs: Needs) {
        let path = std::mem::take(&mut self.gstate.path);
        self.paint((*path.segs).clone(), make, needs);
    }

    fn intersect_clip(&mut self, path: Vec<Seg>, rule: FillRule) {
        let id = self.next_clip_id;
        self.next_clip_id += 1;
        self.gstate.clip.push(ClipEntry {
            id,
            rule,
            path: path.into(),
        });
    }

    fn rect_path(&self, rects: &[Rect]) -> Vec<Seg> {
        rects
            .iter()
            .flat_map(|r| rect_segments(self.gstate.ctm, r))
            .collect()
    }

    /// Appends the pieces of an arc in user space; the caller has placed
    /// the current point at the arc's start.
    fn append_arc(&mut self, center: Point, radius: f32, start: f32, sweep: f64) {
        for (c1, c2, p) in arc::curves(center, radius, start, sweep) {
            let (c1, c2, p) = (self.device(c1), self.device(c2), self.device(p));
            self.gstate
                .path
                .curve_to(c1, c2, p)
                .expect("the arc start is the current point");
        }
    }

    /// `arc`/`arcn`: a line (or move, on an empty path) to the start
    /// point, then the curves.
    fn arc_family(
        &mut self,
        center: Point,
        radius: f32,
        start: f32,
        end: f32,
        ccw: bool,
    ) -> Result<(), VmError> {
        if !radius.is_finite() {
            return Err(VmError::RangeCheck);
        }
        let first = self.device(arc::point_at(center, radius, f64::from(start)));
        if self.gstate.path.current.is_some() {
            self.gstate.path.line_to(first)?;
        } else {
            self.gstate.path.move_to(first);
        }
        self.append_arc(center, radius, start, arc::sweep(start, end, ccw));
        Ok(())
    }

    fn deliver(&mut self, page: Page) {
        self.sink.page(page);
    }
}

impl<S: PageSink> GraphicsBackend for Graphics<S> {
    fn gsave(&mut self) -> Result<(), VmError> {
        self.stack.push(self.gstate.clone());
        Ok(())
    }

    fn grestore(&mut self) -> Result<(), VmError> {
        if let Some(state) = self.stack.pop() {
            self.gstate = state;
        }
        Ok(())
    }

    fn gstate_depth(&self) -> usize {
        self.stack.len()
    }

    fn grestore_to(&mut self, depth: usize) -> Result<(), VmError> {
        while self.stack.len() > depth {
            self.grestore()?;
        }
        Ok(())
    }

    fn initgraphics(&mut self) -> Result<(), VmError> {
        self.gstate = self.gstate.reinitialized();
        Ok(())
    }

    fn set_line_width(&mut self, width: f32) -> Result<(), VmError> {
        if !width.is_finite() {
            return Err(VmError::RangeCheck);
        }
        self.gstate.line_width = width.abs();
        Ok(())
    }

    fn line_width(&self) -> f32 {
        self.gstate.line_width
    }

    fn set_line_cap(&mut self, cap: LineCap) -> Result<(), VmError> {
        self.gstate.line_cap = cap;
        Ok(())
    }

    fn line_cap(&self) -> LineCap {
        self.gstate.line_cap
    }

    fn set_line_join(&mut self, join: LineJoin) -> Result<(), VmError> {
        self.gstate.line_join = join;
        Ok(())
    }

    fn line_join(&self) -> LineJoin {
        self.gstate.line_join
    }

    fn set_miter_limit(&mut self, limit: f32) -> Result<(), VmError> {
        if !limit.is_finite() || limit < 1.0 {
            return Err(VmError::RangeCheck);
        }
        self.gstate.miter_limit = limit;
        Ok(())
    }

    fn miter_limit(&self) -> f32 {
        self.gstate.miter_limit
    }

    fn set_dash(&mut self, array: &[f32], phase: f32) -> Result<(), VmError> {
        if array.iter().any(|l| !l.is_finite() || *l < 0.0)
            || (!array.is_empty() && array.iter().all(|&l| l == 0.0))
            || !phase.is_finite()
        {
            return Err(VmError::RangeCheck);
        }
        self.gstate.dash = (array.to_vec(), phase);
        Ok(())
    }

    fn dash(&self) -> (Vec<f32>, f32) {
        self.gstate.dash.clone()
    }

    fn set_flatness(&mut self, flatness: f32) -> Result<(), VmError> {
        if flatness.is_nan() {
            return Err(VmError::RangeCheck);
        }
        self.gstate.flatness = flatness.clamp(MIN_FLATNESS, MAX_FLATNESS);
        Ok(())
    }

    fn flatness(&self) -> f32 {
        self.gstate.flatness
    }

    fn concat(&mut self, matrix: Matrix) -> Result<(), VmError> {
        self.gstate.ctm = matrix.then(self.gstate.ctm);
        Ok(())
    }

    fn set_matrix(&mut self, matrix: Matrix) -> Result<(), VmError> {
        self.gstate.ctm = matrix;
        Ok(())
    }

    fn current_matrix(&self) -> Matrix {
        self.gstate.ctm
    }

    fn default_matrix(&self) -> Matrix {
        Matrix::IDENTITY
    }

    fn set_color_space(&mut self, space: &SpaceSpec) -> Result<(), VmError> {
        self.gstate.color = space.initial_color();
        self.gstate.space = space.clone();
        Ok(())
    }

    fn set_color(&mut self, components: &[f32]) -> Result<(), VmError> {
        self.gstate.set_color(components)
    }

    fn current_color_space(&self) -> SpaceSpec {
        self.gstate.space.clone()
    }

    fn current_color(&self) -> Vec<f32> {
        self.gstate.color.clone()
    }

    fn newpath(&mut self) -> Result<(), VmError> {
        self.gstate.path = Path::default();
        Ok(())
    }

    fn moveto(&mut self, p: Point) -> Result<(), VmError> {
        let p = self.device(p);
        self.gstate.path.move_to(p);
        Ok(())
    }

    fn lineto(&mut self, p: Point) -> Result<(), VmError> {
        let p = self.device(p);
        self.gstate.path.line_to(p)
    }

    fn curveto(&mut self, c1: Point, c2: Point, p: Point) -> Result<(), VmError> {
        let (c1, c2, p) = (self.device(c1), self.device(c2), self.device(p));
        self.gstate.path.curve_to(c1, c2, p)
    }

    fn closepath(&mut self) -> Result<(), VmError> {
        self.gstate.path.close();
        Ok(())
    }

    fn arc(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError> {
        self.arc_family(center, radius, start, end, true)
    }

    fn arcn(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError> {
        self.arc_family(center, radius, start, end, false)
    }

    /// A negative radius is `undefinedresult`; when no arc fits (the
    /// points coincide or are collinear) a straight line to `p1` is
    /// appended and both tangent points are `p1`.
    fn arcto(&mut self, p1: Point, p2: Point, radius: f32) -> Result<(Point, Point), VmError> {
        let p0 = self.gstate.current_point()?;
        if !radius.is_finite() || radius < 0.0 {
            return Err(VmError::UndefinedResult);
        }
        let Some(tangent) = arc::tangent(p0, p1, p2, radius) else {
            let p = self.device(p1);
            self.gstate.path.line_to(p)?;
            return Ok((p1, p1));
        };
        let t1 = self.device(tangent.t1);
        self.gstate.path.line_to(t1)?;
        self.append_arc(tangent.center, radius, tangent.start, tangent.sweep);
        Ok((tangent.t1, tangent.t2))
    }

    fn current_point(&self) -> Result<Point, VmError> {
        self.gstate.current_point()
    }

    fn path_bbox(&self) -> Result<Bounds, VmError> {
        self.gstate.path_bbox()
    }

    fn fill(&mut self) -> Result<(), VmError> {
        self.paint_current(
            |path, _| IrOp::Fill {
                path,
                rule: FillRule::NonZero,
            },
            Needs::Fill,
        );
        Ok(())
    }

    fn eofill(&mut self) -> Result<(), VmError> {
        self.paint_current(
            |path, _| IrOp::Fill {
                path,
                rule: FillRule::EvenOdd,
            },
            Needs::Fill,
        );
        Ok(())
    }

    fn stroke(&mut self) -> Result<(), VmError> {
        self.paint_current(|path, ctm| IrOp::Stroke { path, ctm }, Needs::Stroke);
        Ok(())
    }

    fn rectfill(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        let path = self.rect_path(rects);
        self.paint(
            path,
            |path, _| IrOp::Fill {
                path,
                rule: FillRule::NonZero,
            },
            Needs::Fill,
        );
        Ok(())
    }

    fn rectstroke(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        let path = self.rect_path(rects);
        self.paint(path, |path, ctm| IrOp::Stroke { path, ctm }, Needs::Stroke);
        Ok(())
    }

    fn rectclip(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        let path = self.rect_path(rects);
        self.intersect_clip(path, FillRule::NonZero);
        self.newpath()
    }

    /// The current path stays, as PLRM3 §8.2 `clip` leaves it.
    fn clip(&mut self) -> Result<(), VmError> {
        let path = (*self.gstate.path.segs).clone();
        self.intersect_clip(path, FillRule::NonZero);
        Ok(())
    }

    fn eoclip(&mut self) -> Result<(), VmError> {
        let path = (*self.gstate.path.segs).clone();
        self.intersect_clip(path, FillRule::EvenOdd);
        Ok(())
    }

    fn initclip(&mut self) -> Result<(), VmError> {
        self.gstate.clip.clear();
        Ok(())
    }

    /// The most recent clip entry stands for the whole clip; with none
    /// the media box is the clip.
    fn clippath(&mut self) -> Result<Vec<Seg>, VmError> {
        let segs: Vec<Seg> = match self.gstate.clip.last() {
            Some(entry) => (*entry.path).clone(),
            None => crate::state::bounds_segments(self.gstate.media_box),
        };
        self.gstate.path = Path::from_segments(segs.clone());
        let to_user = self.gstate.ctm.inverse().unwrap_or(Matrix::IDENTITY);
        Ok(segs
            .into_iter()
            .map(|seg| match seg {
                Seg::Move(p) => Seg::Move(to_user.apply(p)),
                Seg::Line(p) => Seg::Line(to_user.apply(p)),
                Seg::Curve(a, b, c) => {
                    Seg::Curve(to_user.apply(a), to_user.apply(b), to_user.apply(c))
                }
                Seg::Close => Seg::Close,
            })
            .collect())
    }

    fn image(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError> {
        self.place_image(spec, data, Needs::Nothing)
    }

    fn imagemask(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError> {
        self.place_image(spec, data, Needs::Color)
    }

    fn set_font(&mut self, font: Option<FontRef>) -> Result<(), VmError> {
        self.gstate.font = font;
        Ok(())
    }

    fn font(&self) -> Option<FontRef> {
        self.gstate.font
    }

    fn define_font(&mut self, instance: u32, info: &FontInfo) -> Result<(), VmError> {
        self.fonts.insert(instance, info.clone());
        self.page_fonts.remove(&instance);
        Ok(())
    }

    /// Records the run as one text operation at the current point, with
    /// the settings a text paint depends on emitted first, and advances
    /// the current point by the run's displacement.
    fn show(&mut self, glyphs: &[Glyph]) -> Result<(), VmError> {
        let from = self.gstate.path.current()?;
        let font = self.gstate.font.ok_or(VmError::InvalidFont)?;
        let ctm = self.gstate.ctm;
        if !self.gstate.null_device && !glyphs.is_empty() {
            let index = self.font_resource(font)?;
            // The font matrix followed by a translation to the current
            // point and the CTM; the current point is taken in device
            // space, where the path keeps it, rather than back through
            // the inverse CTM and forward again.
            let mut matrix = font.matrix.then(ctm);
            matrix.0[4] += from.x - ctm.0[4];
            matrix.0[5] += from.y - ctm.0[5];
            let needs = if self.is_type3(font) {
                Needs::Stroke
            } else {
                Needs::Fill
            };
            self.sync_clip();
            self.flush(needs);
            self.record(IrOp::Text {
                font: index,
                matrix,
                glyphs: glyphs.to_vec(),
            });
        }
        let delta = ctm.apply_delta(font.matrix.apply_delta(Glyph::total(glyphs)));
        self.gstate
            .path
            .move_to(Point::new(from.x + delta.x, from.y + delta.y));
        Ok(())
    }

    fn begin_glyph(
        &mut self,
        font: FontRef,
        _code: u8,
        name: &[u8],
        measure: bool,
    ) -> Result<(), VmError> {
        let outer_ops = std::mem::take(&mut self.page.ops);
        let outer_emitter = std::mem::replace(&mut self.emitter, Emitter::for_glyph(&self.gstate));
        self.captures.push(Capture {
            font,
            name: name.to_vec(),
            measure,
            to_glyph: inverse64(self.gstate.ctm),
            outer_ops,
            outer_emitter,
        });
        Ok(())
    }

    /// Stores the captured procedure under the glyph's name in its font's
    /// resource, unless the glyph was only measured or declared nothing
    /// (`(0, 0)` and no box: an abandoned procedure). A name already
    /// captured keeps its first procedure.
    fn end_glyph(&mut self, width: (f32, f32), bbox: Option<Bounds>) -> Result<(), VmError> {
        let capture = self.captures.pop().ok_or(VmError::InvalidAccess)?;
        self.close_all();
        let ops = std::mem::replace(&mut self.page.ops, capture.outer_ops);
        self.emitter = capture.outer_emitter;
        let abandoned = width == (0.0, 0.0) && bbox.is_none();
        if capture.measure || abandoned || capture.to_glyph.is_none() || self.gstate.null_device {
            return Ok(());
        }
        let index = self.font_resource(capture.font)?;
        let FontSpec::Type3 { glyphs, .. } = &mut self.page.resources.fonts[index.0] else {
            return Err(VmError::InvalidFont);
        };
        glyphs
            .entry(capture.name)
            .or_insert(GlyphProc { ops, width, bbox });
        Ok(())
    }

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError> {
        self.page_operation()?;
        self.gstate.media_box = media_box;
        Ok(())
    }

    /// Delivers the page and reinitializes the graphics state for the
    /// next one, keeping the media box. Under the null device there is no
    /// page to deliver, but the state is reinitialized all the same.
    fn showpage(&mut self) -> Result<(), VmError> {
        self.page_operation()?;
        if !self.gstate.null_device {
            self.close_all();
            let media_box = self.gstate.media_box;
            let mut page = std::mem::replace(&mut self.page, Page::new(media_box));
            page.media_box = media_box;
            self.reset_page(media_box);
            self.deliver(page);
        }
        self.gstate = self.gstate.reinitialized();
        Ok(())
    }

    /// Delivers a copy of the page so far without ending it.
    fn copypage(&mut self) -> Result<(), VmError> {
        self.page_operation()?;
        if self.gstate.null_device {
            return Ok(());
        }
        let mut page = self.page.clone();
        page.media_box = self.gstate.media_box;
        for _ in &self.emitter.open {
            page.ops.push(IrOp::Restore.into());
        }
        self.deliver(page);
        Ok(())
    }

    fn erasepage(&mut self) -> Result<(), VmError> {
        self.page_operation()?;
        if self.gstate.null_device {
            return Ok(());
        }
        self.reset_page(self.gstate.media_box);
        Ok(())
    }

    /// The null device takes the device's default CTM and no clip; it
    /// stays until a state without it is restored.
    fn nulldevice(&mut self) -> Result<(), VmError> {
        self.gstate.null_device = true;
        self.gstate.ctm = Matrix::IDENTITY;
        self.gstate.clip.clear();
        Ok(())
    }
}

impl<S: PageSink> Graphics<S> {
    /// PDF paints an image over the unit square with its first row at the
    /// top; the PostScript image matrix maps user space to a sample grid
    /// whose first row is at the bottom. `matrix` composes the flip, the
    /// grid, the inverse image matrix, and the CTM.
    fn place_image(&mut self, spec: &ImageSpec, data: &[u8], needs: Needs) -> Result<(), VmError> {
        let grid = spec.matrix.inverse().ok_or(VmError::UndefinedResult)?;
        if self.gstate.null_device {
            return Ok(());
        }
        let flip = Matrix([1.0, 0.0, 0.0, -1.0, 0.0, 1.0]);
        let matrix = flip
            .then(Matrix::scaling(spec.width as f32, spec.height as f32))
            .then(grid)
            .then(self.gstate.ctm);
        self.sync_clip();
        self.flush(needs);
        let image = self.page.resources.add_image(spec, data);
        self.record(IrOp::Image { image, matrix });
        Ok(())
    }
}

/// The inverse of `m` in double precision, or `None` for a singular
/// matrix.
fn inverse64(m: Matrix) -> Option<[f64; 6]> {
    let [a, b, c, d, tx, ty] = m.0.map(f64::from);
    let det = a * d - b * c;
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
    Some([ia, ib, ic, id, -(tx * ia + ty * ic), -(tx * ib + ty * id)])
}

fn apply64(m: [f64; 6], p: Point) -> Point {
    let [a, b, c, d, tx, ty] = m;
    let (x, y) = (f64::from(p.x), f64::from(p.y));
    Point::new((a * x + c * y + tx) as f32, (b * x + d * y + ty) as f32)
}

/// `first` followed by `then`, computed in double precision.
fn then64(first: Matrix, then: [f64; 6]) -> Matrix {
    let [a, b, c, d, tx, ty] = first.0.map(f64::from);
    let [a2, b2, c2, d2, tx2, ty2] = then;
    Matrix(
        [
            a * a2 + b * c2,
            a * b2 + b * d2,
            c * a2 + d * c2,
            c * b2 + d * d2,
            tx * a2 + ty * c2 + tx2,
            tx * b2 + ty * d2 + ty2,
        ]
        .map(|v| v as f32 + 0.0),
    )
}

fn map_segments(path: Vec<Seg>, m: [f64; 6]) -> Vec<Seg> {
    path.into_iter()
        .map(|seg| match seg {
            Seg::Move(p) => Seg::Move(apply64(m, p)),
            Seg::Line(p) => Seg::Line(apply64(m, p)),
            Seg::Curve(a, b, c) => Seg::Curve(apply64(m, a), apply64(m, b), apply64(m, c)),
            Seg::Close => Seg::Close,
        })
        .collect()
}

/// The operation with its geometry taken from default user space through
/// `m`: paths point by point, and the matrices a stroke, an image, or a
/// nested run carry composed with it.
fn transformed(op: IrOp, m: [f64; 6]) -> IrOp {
    match op {
        IrOp::Fill { path, rule } => IrOp::Fill {
            path: map_segments(path, m),
            rule,
        },
        IrOp::Stroke { path, ctm } => IrOp::Stroke {
            path: map_segments(path, m),
            ctm: then64(ctm, m),
        },
        IrOp::Clip { path, rule } => IrOp::Clip {
            path: map_segments(path, m),
            rule,
        },
        IrOp::Image { image, matrix } => IrOp::Image {
            image,
            matrix: then64(matrix, m),
        },
        IrOp::Text {
            font,
            matrix,
            glyphs,
        } => IrOp::Text {
            font,
            matrix: then64(matrix, m),
            glyphs,
        },
        other => other,
    }
}

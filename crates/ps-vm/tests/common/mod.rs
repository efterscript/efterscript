// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A recording graphics backend shared by the boundary tests: every call
//! is logged as a value, queries are answered from a small internal
//! state, and nothing draws.

#![allow(dead_code)]

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use ps_vm::{
    Bounds, CieColor, DEFAULT_SMOOTHNESS, FontInfo, FontRef, FormInfo, Glyph, GraphicsBackend,
    ImageSpec, LineCap, LineJoin, MarkValue, Matrix, PatternInfo, Point, ProcRef, Rect, Seg,
    ShadingSpec, SpaceSpec, VmError,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    GSave,
    GRestore,
    GRestoreTo(usize),
    InitGraphics,
    LineWidth(f32),
    LineCap(LineCap),
    LineJoin(LineJoin),
    MiterLimit(f32),
    Dash(Vec<f32>, f32),
    Flatness(f32),
    Concat(Matrix),
    SetMatrix(Matrix),
    ColorSpace(SpaceSpec),
    Color(Vec<f32>),
    NewPath,
    MoveTo(Point),
    LineTo(Point),
    CurveTo(Point, Point, Point),
    ClosePath,
    Arc(Point, f32, f32, f32),
    ArcN(Point, f32, f32, f32),
    ArcTo(Point, Point, f32),
    Fill,
    EoFill,
    Stroke,
    RectFill(Vec<Rect>),
    RectStroke(Vec<Rect>),
    RectClip(Vec<Rect>),
    Clip,
    EoClip,
    InitClip,
    ClipPath,
    Image(ImageSpec, Vec<u8>),
    ImageMask(ImageSpec, Vec<u8>),
    SetFont(Option<FontRef>),
    Show(Vec<Glyph>),
    BeginGlyph(FontRef, u8, Vec<u8>, bool),
    EndGlyph((f32, f32), Option<Bounds>),
    SetPattern(PatternInfo, Vec<f32>),
    BeginPatternCell(PatternInfo),
    EndPatternCell,
    BeginForm(FormInfo),
    EndForm,
    PlaceForm(FormInfo),
    Shade(ShadingSpec),
    Smoothness(f32),
    Overprint(bool),
    StrokeOutline,
    MediaBox(Bounds),
    ShowPage,
    CopyPage,
    ErasePage,
    NullDevice,
    PdfMark(Vec<u8>, Vec<MarkValue>),
    DistillerParams(Vec<(Vec<u8>, MarkValue)>),
}

#[derive(Clone)]
pub struct State {
    ctm: Matrix,
    line_width: f32,
    cap: LineCap,
    join: LineJoin,
    miter: f32,
    dash: (Vec<f32>, f32),
    flat: f32,
    smoothness: f32,
    space: SpaceSpec,
    color: Vec<f32>,
    pattern: Option<PatternInfo>,
    cie: Option<CieColor>,
    color_rendering: Option<ProcRef>,
    font: Option<FontRef>,
    // The current point in device space, as a real backend would keep it;
    // part of the state, since the path is.
    current: Option<Point>,
}

impl Default for State {
    fn default() -> Self {
        State {
            ctm: Matrix::IDENTITY,
            line_width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter: 10.0,
            dash: (Vec::new(), 0.0),
            flat: 1.0,
            smoothness: DEFAULT_SMOOTHNESS,
            space: SpaceSpec::DeviceGray,
            color: vec![0.0],
            pattern: None,
            cie: None,
            color_rendering: None,
            font: None,
            current: None,
        }
    }
}

pub type Log = Rc<RefCell<Vec<Call>>>;
/// The font descriptions received, by instance.
pub type Fonts = Rc<RefCell<Vec<(u32, FontInfo)>>>;

pub struct Recording {
    pub log: Log,
    pub fonts: Fonts,
    pub state: State,
    pub stack: Vec<State>,
    /// The pattern cells and form bodies captured on the current page,
    /// which `begin_*` answers `false` for; cleared by `showpage`.
    pub cells: HashSet<u64>,
    pub bodies: HashSet<u64>,
    /// The CTM at each open `begin_glyph`, innermost last.
    pub glyph_ctms: Vec<Matrix>,
}

impl Recording {
    pub fn new(log: Log) -> Self {
        Self::with_fonts(log, Rc::new(RefCell::new(Vec::new())))
    }

    pub fn with_fonts(log: Log, fonts: Fonts) -> Self {
        Recording {
            log,
            fonts,
            state: State::default(),
            stack: Vec::new(),
            cells: HashSet::new(),
            bodies: HashSet::new(),
            glyph_ctms: Vec::new(),
        }
    }

    fn record(&self, call: Call) {
        self.log.borrow_mut().push(call);
    }

    fn set_current(&mut self, p: Point) {
        self.state.current = Some(self.state.ctm.apply(p));
    }
}

impl GraphicsBackend for Recording {
    fn gsave(&mut self) -> Result<(), VmError> {
        self.record(Call::GSave);
        self.stack.push(self.state.clone());
        Ok(())
    }

    fn grestore(&mut self) -> Result<(), VmError> {
        self.record(Call::GRestore);
        if let Some(state) = self.stack.pop() {
            self.state = state;
        }
        Ok(())
    }

    fn gstate_depth(&self) -> usize {
        self.stack.len()
    }

    fn grestore_to(&mut self, depth: usize) -> Result<(), VmError> {
        self.record(Call::GRestoreTo(depth));
        while self.stack.len() > depth {
            let state = self.stack.pop().expect("checked");
            self.state = state;
        }
        Ok(())
    }

    fn initgraphics(&mut self) -> Result<(), VmError> {
        self.record(Call::InitGraphics);
        // Kept across `initgraphics`, as the real backend keeps it.
        let color_rendering = self.state.color_rendering;
        self.state = State {
            color_rendering,
            ..State::default()
        };
        Ok(())
    }

    fn set_line_width(&mut self, width: f32) -> Result<(), VmError> {
        self.record(Call::LineWidth(width));
        self.state.line_width = width;
        Ok(())
    }

    fn line_width(&self) -> f32 {
        self.state.line_width
    }

    fn set_line_cap(&mut self, cap: LineCap) -> Result<(), VmError> {
        self.record(Call::LineCap(cap));
        self.state.cap = cap;
        Ok(())
    }

    fn line_cap(&self) -> LineCap {
        self.state.cap
    }

    fn set_line_join(&mut self, join: LineJoin) -> Result<(), VmError> {
        self.record(Call::LineJoin(join));
        self.state.join = join;
        Ok(())
    }

    fn line_join(&self) -> LineJoin {
        self.state.join
    }

    fn set_miter_limit(&mut self, limit: f32) -> Result<(), VmError> {
        self.record(Call::MiterLimit(limit));
        self.state.miter = limit;
        Ok(())
    }

    fn miter_limit(&self) -> f32 {
        self.state.miter
    }

    fn set_dash(&mut self, array: &[f32], phase: f32) -> Result<(), VmError> {
        self.record(Call::Dash(array.to_vec(), phase));
        self.state.dash = (array.to_vec(), phase);
        Ok(())
    }

    fn dash(&self) -> (Vec<f32>, f32) {
        self.state.dash.clone()
    }

    fn set_flatness(&mut self, flatness: f32) -> Result<(), VmError> {
        self.record(Call::Flatness(flatness));
        self.state.flat = flatness;
        Ok(())
    }

    fn flatness(&self) -> f32 {
        self.state.flat
    }

    fn concat(&mut self, matrix: Matrix) -> Result<(), VmError> {
        self.record(Call::Concat(matrix));
        self.state.ctm = matrix.then(self.state.ctm);
        Ok(())
    }

    fn set_matrix(&mut self, matrix: Matrix) -> Result<(), VmError> {
        self.record(Call::SetMatrix(matrix));
        self.state.ctm = matrix;
        Ok(())
    }

    fn current_matrix(&self) -> Matrix {
        self.state.ctm
    }

    fn default_matrix(&self) -> Matrix {
        Matrix::IDENTITY
    }

    fn set_color_space(&mut self, space: &SpaceSpec) -> Result<(), VmError> {
        self.record(Call::ColorSpace(space.clone()));
        self.state.color = space.initial_color();
        self.state.space = space.clone();
        self.state.pattern = None;
        self.state.cie = None;
        Ok(())
    }

    fn set_color(&mut self, components: &[f32]) -> Result<(), VmError> {
        self.record(Call::Color(components.to_vec()));
        self.state.color = components.to_vec();
        self.state.pattern = None;
        Ok(())
    }

    fn current_color_space(&self) -> SpaceSpec {
        self.state.space.clone()
    }

    fn current_color(&self) -> Vec<f32> {
        self.state.color.clone()
    }

    fn newpath(&mut self) -> Result<(), VmError> {
        self.record(Call::NewPath);
        self.state.current = None;
        Ok(())
    }

    fn moveto(&mut self, p: Point) -> Result<(), VmError> {
        self.record(Call::MoveTo(p));
        self.set_current(p);
        Ok(())
    }

    fn lineto(&mut self, p: Point) -> Result<(), VmError> {
        self.record(Call::LineTo(p));
        self.current_point()?;
        self.set_current(p);
        Ok(())
    }

    fn curveto(&mut self, c1: Point, c2: Point, p: Point) -> Result<(), VmError> {
        self.record(Call::CurveTo(c1, c2, p));
        self.current_point()?;
        self.set_current(p);
        Ok(())
    }

    fn closepath(&mut self) -> Result<(), VmError> {
        self.record(Call::ClosePath);
        Ok(())
    }

    fn arc(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError> {
        self.record(Call::Arc(center, radius, start, end));
        let (sin, cos) = end.to_radians().sin_cos();
        self.set_current(Point::new(center.x + radius * cos, center.y + radius * sin));
        Ok(())
    }

    fn arcn(&mut self, center: Point, radius: f32, start: f32, end: f32) -> Result<(), VmError> {
        self.record(Call::ArcN(center, radius, start, end));
        let (sin, cos) = end.to_radians().sin_cos();
        self.set_current(Point::new(center.x + radius * cos, center.y + radius * sin));
        Ok(())
    }

    fn arcto(&mut self, p1: Point, p2: Point, radius: f32) -> Result<(Point, Point), VmError> {
        self.record(Call::ArcTo(p1, p2, radius));
        self.current_point()?;
        self.set_current(p1);
        Ok((p1, p2))
    }

    fn current_point(&self) -> Result<Point, VmError> {
        let device = self.state.current.ok_or(VmError::NoCurrentPoint)?;
        let inverse = self.state.ctm.inverse().ok_or(VmError::UndefinedResult)?;
        Ok(inverse.apply(device))
    }

    fn path_bbox(&self) -> Result<Bounds, VmError> {
        let p = self.current_point()?;
        Ok(Bounds::new(p.x, p.y, p.x, p.y))
    }

    fn fill(&mut self) -> Result<(), VmError> {
        self.record(Call::Fill);
        self.state.current = None;
        Ok(())
    }

    fn eofill(&mut self) -> Result<(), VmError> {
        self.record(Call::EoFill);
        self.state.current = None;
        Ok(())
    }

    fn stroke(&mut self) -> Result<(), VmError> {
        self.record(Call::Stroke);
        self.state.current = None;
        Ok(())
    }

    fn rectfill(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        self.record(Call::RectFill(rects.to_vec()));
        Ok(())
    }

    fn rectstroke(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        self.record(Call::RectStroke(rects.to_vec()));
        Ok(())
    }

    fn rectclip(&mut self, rects: &[Rect]) -> Result<(), VmError> {
        self.record(Call::RectClip(rects.to_vec()));
        self.state.current = None;
        Ok(())
    }

    fn clip(&mut self) -> Result<(), VmError> {
        self.record(Call::Clip);
        Ok(())
    }

    fn eoclip(&mut self) -> Result<(), VmError> {
        self.record(Call::EoClip);
        Ok(())
    }

    fn initclip(&mut self) -> Result<(), VmError> {
        self.record(Call::InitClip);
        Ok(())
    }

    fn clippath(&mut self) -> Result<Vec<Seg>, VmError> {
        self.record(Call::ClipPath);
        self.set_current(Point::new(0.0, 0.0));
        Ok(vec![Seg::Move(Point::new(0.0, 0.0)), Seg::Close])
    }

    fn image(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError> {
        self.record(Call::Image(spec.clone(), data.to_vec()));
        Ok(())
    }

    fn imagemask(&mut self, spec: &ImageSpec, data: &[u8]) -> Result<(), VmError> {
        self.record(Call::ImageMask(spec.clone(), data.to_vec()));
        Ok(())
    }

    fn define_font(&mut self, instance: u32, info: &FontInfo) -> Result<(), VmError> {
        self.fonts.borrow_mut().push((instance, info.clone()));
        Ok(())
    }

    fn set_font(&mut self, font: Option<FontRef>) -> Result<(), VmError> {
        self.record(Call::SetFont(font));
        self.state.font = font;
        Ok(())
    }

    fn font(&self) -> Option<FontRef> {
        self.state.font
    }

    fn show(&mut self, glyphs: &[Glyph]) -> Result<(), VmError> {
        self.record(Call::Show(glyphs.to_vec()));
        let from = self.current_point()?;
        let font = self.state.font.ok_or(VmError::InvalidFont)?;
        let delta = font.matrix.apply_delta(Glyph::total(glyphs));
        self.set_current(Point::new(from.x + delta.x, from.y + delta.y));
        Ok(())
    }

    fn begin_glyph(
        &mut self,
        font: FontRef,
        code: u8,
        name: &[u8],
        measure: bool,
    ) -> Result<(), VmError> {
        self.record(Call::BeginGlyph(font, code, name.to_vec(), measure));
        self.glyph_ctms.push(self.state.ctm);
        Ok(())
    }

    fn end_glyph(&mut self, width: (f32, f32), bbox: Option<Bounds>) -> Result<(), VmError> {
        self.record(Call::EndGlyph(width, bbox));
        self.glyph_ctms.pop();
        Ok(())
    }

    fn glyph_matrix(&self) -> Option<Matrix> {
        self.glyph_ctms.last().copied()
    }

    fn set_pattern(&mut self, pattern: &PatternInfo, components: &[f32]) -> Result<(), VmError> {
        self.record(Call::SetPattern(pattern.clone(), components.to_vec()));
        self.state.color = components.to_vec();
        self.state.pattern = Some(pattern.clone());
        Ok(())
    }

    fn current_pattern(&self) -> Option<PatternInfo> {
        self.state.pattern.clone()
    }

    fn set_cie_color(&mut self, color: CieColor) -> Result<(), VmError> {
        self.state.cie = Some(color);
        Ok(())
    }

    fn current_cie_color(&self) -> Option<CieColor> {
        self.state.cie
    }

    fn set_color_rendering(&mut self, dict: Option<ProcRef>) -> Result<(), VmError> {
        self.state.color_rendering = dict;
        Ok(())
    }

    fn color_rendering(&self) -> Option<ProcRef> {
        self.state.color_rendering
    }

    /// Captures each instance once per page, inside a saved state that
    /// starts from the initial colour, as the real backend does.
    fn begin_pattern_cell(&mut self, pattern: &PatternInfo) -> Result<bool, VmError> {
        self.record(Call::BeginPatternCell(pattern.clone()));
        if !self.cells.insert(pattern.id) {
            return Ok(false);
        }
        self.gsave()?;
        self.state.ctm = pattern.matrix;
        self.state.current = None;
        self.state.space = SpaceSpec::DeviceGray;
        self.state.color = vec![0.0];
        self.state.pattern = None;
        self.state.cie = None;
        Ok(true)
    }

    fn end_pattern_cell(&mut self) -> Result<(), VmError> {
        self.record(Call::EndPatternCell);
        Ok(())
    }

    fn begin_form(&mut self, form: &FormInfo) -> Result<bool, VmError> {
        self.record(Call::BeginForm(*form));
        if !self.bodies.insert(form.id) {
            return Ok(false);
        }
        self.gsave()?;
        self.state.ctm = form.matrix;
        self.state.current = None;
        Ok(true)
    }

    fn end_form(&mut self) -> Result<(), VmError> {
        self.record(Call::EndForm);
        Ok(())
    }

    fn place_form(&mut self, form: &FormInfo) -> Result<(), VmError> {
        self.record(Call::PlaceForm(*form));
        Ok(())
    }

    fn shade(&mut self, shading: &ShadingSpec) -> Result<(), VmError> {
        self.record(Call::Shade(shading.clone()));
        Ok(())
    }

    fn set_smoothness(&mut self, smoothness: f32) -> Result<(), VmError> {
        self.record(Call::Smoothness(smoothness));
        self.state.smoothness = smoothness;
        Ok(())
    }

    fn smoothness(&self) -> f32 {
        self.state.smoothness
    }

    fn set_overprint(&mut self, on: bool) -> Result<(), VmError> {
        self.record(Call::Overprint(on));
        Ok(())
    }

    fn stroke_outline(&mut self) -> Result<(), VmError> {
        self.record(Call::StrokeOutline);
        Ok(())
    }

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError> {
        self.record(Call::MediaBox(media_box));
        Ok(())
    }

    fn showpage(&mut self) -> Result<(), VmError> {
        self.record(Call::ShowPage);
        self.state.current = None;
        self.cells.clear();
        self.bodies.clear();
        Ok(())
    }

    fn copypage(&mut self) -> Result<(), VmError> {
        self.record(Call::CopyPage);
        Ok(())
    }

    fn erasepage(&mut self) -> Result<(), VmError> {
        self.record(Call::ErasePage);
        Ok(())
    }

    fn nulldevice(&mut self) -> Result<(), VmError> {
        self.record(Call::NullDevice);
        Ok(())
    }

    fn pdfmark(&mut self, kind: &[u8], entries: &[MarkValue]) -> Result<(), VmError> {
        self.record(Call::PdfMark(kind.to_vec(), entries.to_vec()));
        Ok(())
    }

    fn set_distiller_params(&mut self, entries: &[(Vec<u8>, MarkValue)]) -> Result<(), VmError> {
        self.record(Call::DistillerParams(entries.to_vec()));
        Ok(())
    }
}

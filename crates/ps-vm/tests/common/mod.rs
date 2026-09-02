// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A recording graphics backend shared by the boundary tests: every call
//! is logged as a value, queries are answered from a small internal
//! state, and nothing draws.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use ps_vm::{
    Bounds, FontRef, Glyph, GraphicsBackend, ImageSpec, LineCap, LineJoin, Matrix, Point, Rect,
    Seg, SpaceSpec, VmError,
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
    MediaBox(Bounds),
    ShowPage,
    CopyPage,
    ErasePage,
    NullDevice,
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
    space: SpaceSpec,
    color: Vec<f32>,
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
            space: SpaceSpec::DeviceGray,
            color: vec![0.0],
            font: None,
            current: None,
        }
    }
}

pub type Log = Rc<RefCell<Vec<Call>>>;

pub struct Recording {
    pub log: Log,
    pub state: State,
    pub stack: Vec<State>,
}

impl Recording {
    pub fn new(log: Log) -> Self {
        Recording {
            log,
            state: State::default(),
            stack: Vec::new(),
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
        self.state = State::default();
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
        Ok(())
    }

    fn set_color(&mut self, components: &[f32]) -> Result<(), VmError> {
        self.record(Call::Color(components.to_vec()));
        self.state.color = components.to_vec();
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
        Ok(())
    }

    fn end_glyph(&mut self, width: (f32, f32), bbox: Option<Bounds>) -> Result<(), VmError> {
        self.record(Call::EndGlyph(width, bbox));
        Ok(())
    }

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError> {
        self.record(Call::MediaBox(media_box));
        Ok(())
    }

    fn showpage(&mut self) -> Result<(), VmError> {
        self.record(Call::ShowPage);
        self.state.current = None;
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
}

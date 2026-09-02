// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The graphics boundary, checked with a recording backend: every call is
//! logged as a value, queries are answered from a small internal state,
//! and nothing draws. Scenarios that depend on the IR belong to the
//! graphics crate.

use std::cell::RefCell;
use std::rc::Rc;

use ps_vm::{
    Bounds, Config, GraphicsBackend, ImageSpec, Interp, Io, LineCap, LineJoin, Matrix, Object,
    Outcome, Point, Rect, Seg, SliceSource, SpaceSpec, Stream, VmError,
};

#[derive(Clone, Debug, PartialEq)]
enum Call {
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
    MediaBox(Bounds),
    ShowPage,
    CopyPage,
    ErasePage,
    NullDevice,
}

#[derive(Clone)]
struct State {
    ctm: Matrix,
    line_width: f32,
    cap: LineCap,
    join: LineJoin,
    miter: f32,
    dash: (Vec<f32>, f32),
    flat: f32,
    space: SpaceSpec,
    color: Vec<f32>,
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
        }
    }
}

type Log = Rc<RefCell<Vec<Call>>>;

struct Recording {
    log: Log,
    state: State,
    stack: Vec<State>,
    // The current point in device space, as a real backend would keep it.
    current: Option<Point>,
}

impl Recording {
    fn new(log: Log) -> Self {
        Recording {
            log,
            state: State::default(),
            stack: Vec::new(),
            current: None,
        }
    }

    fn record(&self, call: Call) {
        self.log.borrow_mut().push(call);
    }

    fn set_current(&mut self, p: Point) {
        self.current = Some(self.state.ctm.apply(p));
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
        self.current = None;
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
        self.current = None;
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
        let device = self.current.ok_or(VmError::NoCurrentPoint)?;
        let inverse = self.state.ctm.inverse().ok_or(VmError::UndefinedResult)?;
        Ok(inverse.apply(device))
    }

    fn path_bbox(&self) -> Result<Bounds, VmError> {
        let p = self.current_point()?;
        Ok(Bounds::new(p.x, p.y, p.x, p.y))
    }

    fn fill(&mut self) -> Result<(), VmError> {
        self.record(Call::Fill);
        self.current = None;
        Ok(())
    }

    fn eofill(&mut self) -> Result<(), VmError> {
        self.record(Call::EoFill);
        self.current = None;
        Ok(())
    }

    fn stroke(&mut self) -> Result<(), VmError> {
        self.record(Call::Stroke);
        self.current = None;
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
        self.current = None;
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

    fn set_media_box(&mut self, media_box: Bounds) -> Result<(), VmError> {
        self.record(Call::MediaBox(media_box));
        Ok(())
    }

    fn showpage(&mut self) -> Result<(), VmError> {
        self.record(Call::ShowPage);
        self.current = None;
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

// --- helpers -----------------------------------------------------------------

struct Run {
    interp: Interp,
    outcome: Outcome,
    output: String,
    log: Log,
}

impl Run {
    fn calls(&self) -> Vec<Call> {
        self.log.borrow().clone()
    }

    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }

    fn command(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.command),
            _ => None,
        }
    }

    fn top_numbers(&self, n: usize) -> Vec<f32> {
        let stack = self.interp.ostack();
        stack[stack.len() - n..]
            .iter()
            .map(|o| o.as_number().expect("number"))
            .collect()
    }
}

fn run_with(program: &str, mut io: Io) -> Run {
    let out = ps_vm::Capture::new();
    io.stdout = Some(Box::new(out.clone()));
    io.stderr = Some(Box::new(ps_vm::Capture::new()));
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        interp,
        outcome,
        output: out.text(),
        log,
    }
}

fn exec(program: &str) -> Run {
    run_with(program, Io::default())
}

fn run_without_backend(program: &str) -> (Interp, Outcome, String) {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    (interp, outcome, out.text())
}

fn approx(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn matrix_approx(a: Matrix, b: Matrix) -> bool {
    a.0.iter().zip(b.0).all(|(x, y)| approx(*x, y))
}

fn concats(run: &Run) -> Vec<Matrix> {
    run.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Concat(m) => Some(m),
            _ => None,
        })
        .collect()
}

// --- layer boundary ------------------------------------------------------------

// no-backend-moveto-undefined.ps
#[test]
fn scripting_embedder_pays_nothing() {
    let (interp, outcome, _) = run_without_backend("0 0 moveto");
    assert!(
        matches!(&outcome, Outcome::Error(e) if e.name == "undefined" && e.command == "moveto")
    );
    assert!(!interp.has_graphics_backend());
    assert!(interp.operator("moveto").is_none());
    assert!(interp.operator("add").is_some());

    let (_, outcome, output) = run_without_backend(
        "systemdict /fill known = systemdict /setpagedevice known = \
         { 1 2 lineto } stopped = $error /errorname get ==",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "false\ntrue\ntrue\n/undefined\n");
}

#[test]
fn installing_a_backend_defines_the_group_once() {
    let run = exec("systemdict /moveto known = 0 0 moveto 0 0 moveto");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\n");
    assert!(run.interp.operator("moveto").is_some());
    assert_eq!(
        run.calls(),
        [
            Call::MediaBox(Bounds::new(0.0, 0.0, 612.0, 792.0)),
            Call::MoveTo(Point::new(0.0, 0.0)),
            Call::MoveTo(Point::new(0.0, 0.0)),
        ]
    );
    let mut interp = run.interp;
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(b"1 1 moveto"));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(log.borrow()[1], Call::MoveTo(Point::new(1.0, 1.0)));
}

#[test]
fn operands_are_checked_and_kept_on_failure() {
    let run = exec("5 moveto");
    assert_eq!(run.error(), Some("stackunderflow"));
    assert_eq!(run.command(), Some("moveto"));

    let run = exec("(a) 0 moveto");
    assert_eq!(run.error(), Some("typecheck"));

    let run = exec("3 setlinecap");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("-1 setlinejoin");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("0.5 setmiterlimit");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("[0 0] 0 setdash");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("[3 -1] 0 setdash");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("[1 (x)] 0 setdash");
    assert_eq!(run.error(), Some("typecheck"));

    // A failing operator leaves its operands where they were.
    let run = exec("1 2 { (x) 4 lineto } stopped pop count");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.top_numbers(1), [4.0]);

    let run = exec("[1 2 3] setmatrix");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("[1 2 3 4 5 (x)] concat");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("0 0 moveto 1 2 3 4 5 arcto");
    assert_eq!(run.outcome, Outcome::Ok);
    let run = exec("0 0 moveto 1 2 3 4 arcto");
    assert_eq!(run.error(), Some("stackunderflow"));
    let run = exec("(x) 1 2 3 4 arc");
    assert_eq!(run.error(), Some("typecheck"));
}

// --- graphics state ------------------------------------------------------------

#[test]
fn line_parameters_round_trip() {
    let run = exec(
        "2.5 setlinewidth currentlinewidth = 1 setlinecap currentlinecap = \
         2 setlinejoin currentlinejoin = 4 setmiterlimit currentmiterlimit = \
         [3 1] 0.5 setdash currentdash = == 0.3 setflat currentflat =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "2.5\n1\n2\n4.0\n0.5\n[3.0 1.0]\n0.3\n");
    assert!(run.calls().contains(&Call::Dash(vec![3.0, 1.0], 0.5)));
    assert!(run.calls().contains(&Call::LineCap(LineCap::Round)));
    assert!(run.calls().contains(&Call::LineJoin(LineJoin::Bevel)));
    assert!(run.calls().contains(&Call::MiterLimit(4.0)));
    assert!(run.calls().contains(&Call::Flatness(0.3)));

    let run = exec("initgraphics gsave grestoreall");
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(run.calls().contains(&Call::InitGraphics));
}

// --- coordinate system ------------------------------------------------------------

#[test]
fn translate_scale_rotate_concatenate() {
    let run = exec("72 72 translate 2 3 scale 90 rotate");
    assert_eq!(run.outcome, Outcome::Ok);
    let concats = concats(&run);
    assert_eq!(concats.len(), 3);
    assert_eq!(concats[0], Matrix::translation(72.0, 72.0));
    assert_eq!(concats[1], Matrix::scaling(2.0, 3.0));
    assert!(matrix_approx(
        concats[2],
        Matrix([0.0, 1.0, -1.0, 0.0, 0.0, 0.0])
    ));
    // The recording backend applies them like a real one would.
    let run = exec("72 72 translate 0 0 moveto currentpoint 10 20 translate currentpoint");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.top_numbers(4), [0.0, 0.0, -10.0, -20.0]);
}

#[test]
fn matrix_forms_leave_the_ctm_alone() {
    let run = exec(
        "10 20 matrix translate == 2 3 matrix scale == \
         matrix currentmatrix == matrix identmatrix == matrix defaultmatrix ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "[1.0 0.0 0.0 1.0 10.0 20.0]\n[2.0 0.0 0.0 3.0 0.0 0.0]\n\
         [1.0 0.0 0.0 1.0 0.0 0.0]\n[1.0 0.0 0.0 1.0 0.0 0.0]\n[1.0 0.0 0.0 1.0 0.0 0.0]\n"
    );
    assert!(concats(&run).is_empty());

    let run = exec("[2 0 0 2 5 5] setmatrix [1 0 0 1 1 1] concat matrix currentmatrix ==");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "[2.0 0.0 0.0 2.0 7.0 7.0]\n");
    assert!(
        run.calls()
            .contains(&Call::SetMatrix(Matrix([2.0, 0.0, 0.0, 2.0, 5.0, 5.0])))
    );
    let run = exec("2 2 scale initmatrix matrix currentmatrix ==");
    assert_eq!(run.output, "[1.0 0.0 0.0 1.0 0.0 0.0]\n");
}

#[test]
fn matrix_arithmetic_operators() {
    let run = exec(
        "[2 0 0 2 0 0] [1 0 0 1 10 10] matrix concatmatrix == \
         [1 0 0 1 10 10] [2 0 0 2 0 0] matrix concatmatrix == \
         [2 0 0 4 10 20] matrix invertmatrix ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "[2.0 0.0 0.0 2.0 10.0 10.0]\n[2.0 0.0 0.0 2.0 20.0 20.0]\n[0.5 0.0 0.0 0.25 -5.0 -5.0]\n"
    );
    let run = exec("[0 0 0 0 1 1] matrix invertmatrix");
    assert_eq!(run.error(), Some("undefinedresult"));
    let run = exec("matrix readonly identmatrix");
    assert_eq!(run.error(), Some("invalidaccess"));
    let run = exec("5 array currentmatrix");
    assert_eq!(run.error(), Some("rangecheck"));
}

#[test]
fn transforms_go_through_the_ctm_or_a_matrix() {
    let run = exec(
        "10 10 translate 2 2 scale 1 1 transform 2 3 dtransform \
         12 12 itransform 4 4 idtransform 1 1 [1 0 0 1 5 5] transform \
         1 1 [3 0 0 3 5 5] dtransform 6 6 [3 0 0 3 5 5] itransform",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let got = run.top_numbers(14);
    let want = [
        12.0,
        12.0,
        4.0,
        6.0,
        1.0,
        1.0,
        2.0,
        2.0,
        6.0,
        6.0,
        3.0,
        3.0,
        1.0 / 3.0,
        1.0 / 3.0,
    ];
    for (g, w) in got.iter().zip(want) {
        assert!(approx(*g, w), "{got:?}");
    }
    let run = exec("0 0 scale 1 1 itransform");
    assert_eq!(run.error(), Some("undefinedresult"));
}

// --- paths -------------------------------------------------------------------------

#[test]
fn path_operators_pass_their_arguments_through() {
    let run = exec(
        "newpath 10 20 moveto 5 5 rmoveto 30 40 lineto 1 2 rlineto \
         1 2 3 4 5 6 curveto 1 1 2 2 3 3 rcurveto closepath \
         10 20 5 0 90 arc 10 20 5 90 0 arcn 100 0 100 100 10 arcto \
         fill eofill stroke clip eoclip initclip clippath",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let calls = run.calls();
    assert_eq!(
        &calls[1..],
        [
            Call::NewPath,
            Call::MoveTo(Point::new(10.0, 20.0)),
            Call::MoveTo(Point::new(15.0, 25.0)),
            Call::LineTo(Point::new(30.0, 40.0)),
            Call::LineTo(Point::new(31.0, 42.0)),
            Call::CurveTo(
                Point::new(1.0, 2.0),
                Point::new(3.0, 4.0),
                Point::new(5.0, 6.0)
            ),
            Call::CurveTo(
                Point::new(6.0, 7.0),
                Point::new(7.0, 8.0),
                Point::new(8.0, 9.0)
            ),
            Call::ClosePath,
            Call::Arc(Point::new(10.0, 20.0), 5.0, 0.0, 90.0),
            Call::ArcN(Point::new(10.0, 20.0), 5.0, 90.0, 0.0),
            Call::ArcTo(Point::new(100.0, 0.0), Point::new(100.0, 100.0), 10.0),
            Call::Fill,
            Call::EoFill,
            Call::Stroke,
            Call::Clip,
            Call::EoClip,
            Call::InitClip,
            Call::ClipPath,
        ]
    );
    // arcto leaves the two tangent points.
    let run = exec("0 0 moveto 100 0 100 100 10 arcto");
    assert_eq!(run.top_numbers(4), [100.0, 0.0, 100.0, 100.0]);
}

#[test]
fn current_point_queries() {
    let run = exec("currentpoint");
    assert_eq!(run.error(), Some("nocurrentpoint"));
    let run = exec("1 2 rmoveto");
    assert_eq!(run.error(), Some("nocurrentpoint"));
    assert_eq!(run.command(), Some("rmoveto"));
    let run = exec("3 4 moveto currentpoint pathbbox");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.top_numbers(6), [3.0, 4.0, 3.0, 4.0, 3.0, 4.0]);
    let run = exec("pathbbox");
    assert_eq!(run.error(), Some("nocurrentpoint"));
}

#[test]
fn rect_operators_take_numbers_or_arrays() {
    let run = exec("1 2 3 4 rectfill [5 6 7 8 9 10 11 12] rectstroke 0 0 1 1 rectclip");
    assert_eq!(run.outcome, Outcome::Ok);
    let rect = |x, y, width, height| Rect {
        x,
        y,
        width,
        height,
    };
    assert_eq!(
        &run.calls()[1..],
        [
            Call::RectFill(vec![rect(1.0, 2.0, 3.0, 4.0)]),
            Call::RectStroke(vec![rect(5.0, 6.0, 7.0, 8.0), rect(9.0, 10.0, 11.0, 12.0)]),
            Call::RectClip(vec![rect(0.0, 0.0, 1.0, 1.0)]),
        ]
    );
    let run = exec("[1 2 3] rectfill");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("(abc) rectfill");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("1 2 3 rectfill");
    assert_eq!(run.error(), Some("stackunderflow"));
}

// --- colour -------------------------------------------------------------------------

#[test]
fn device_colour_operators_set_space_and_components() {
    let run = exec(
        "0.5 setgray 1 0 0.25 setrgbcolor 0 0.5 1 sethsbcolor 0 1 0 0.5 setcmykcolor 2 setgray",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        &run.calls()[1..],
        [
            Call::ColorSpace(SpaceSpec::DeviceGray),
            Call::Color(vec![0.5]),
            Call::ColorSpace(SpaceSpec::DeviceRGB),
            Call::Color(vec![1.0, 0.0, 0.25]),
            Call::ColorSpace(SpaceSpec::DeviceRGB),
            Call::Color(vec![1.0, 0.5, 0.5]),
            Call::ColorSpace(SpaceSpec::DeviceCMYK),
            Call::Color(vec![0.0, 1.0, 0.0, 0.5]),
            Call::ColorSpace(SpaceSpec::DeviceGray),
            Call::Color(vec![1.0]),
        ]
    );
}

#[test]
fn colour_queries_convert_between_device_spaces() {
    let run = exec(
        "0.5 setgray currentrgbcolor currentcmykcolor \
         1 0 0 setrgbcolor currentgray currenthsbcolor \
         0 0 0 1 setcmykcolor currentrgbcolor currentgray",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let got = run.top_numbers(15);
    let want = [
        0.5, 0.5, 0.5, 0.0, 0.0, 0.0, 0.5, 0.3, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0,
    ];
    for (g, w) in got.iter().zip(want) {
        assert!(approx(*g, w), "{got:?}");
    }
}

#[test]
fn colour_spaces_are_parsed_and_rebuilt() {
    let run = exec(
        "/DeviceRGB setcolorspace currentcolorspace == \
         [/DeviceCMYK] setcolorspace currentcolor == == == == \
         [/Separation /Spot /DeviceCMYK {dup 0 0 0}] setcolorspace 0.6 setcolor \
         currentcolorspace == currentcolor == \
         [/DeviceN [/A /B] /DeviceGray {add 2 div}] setcolorspace 0.25 0.75 setcolor currentcolor == == \
         [/Indexed /DeviceRGB 1 <000000ffffff>] setcolorspace 1 setcolor currentcolor == \
         currentcolorspace ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "[/DeviceRGB]\n1.0\n0.0\n0.0\n0.0\n\
         [/Separation /Spot /DeviceCMYK {dup 0 0 0}]\n0.6\n0.75\n0.25\n1\n\
         [/Indexed /DeviceRGB 1 (\\000\\000\\000\\377\\377\\377)]\n"
    );
    let spot = SpaceSpec::Separation {
        name: b"Spot".to_vec(),
        alternate: Box::new(SpaceSpec::DeviceCMYK),
        tint_source: b"{dup 0 0 0}".to_vec(),
    };
    assert!(run.calls().contains(&Call::ColorSpace(spot)));
    assert!(run.calls().contains(&Call::Color(vec![0.6])));
    assert!(run.calls().contains(&Call::Color(vec![0.25, 0.75])));
    assert!(run.calls().contains(&Call::ColorSpace(SpaceSpec::Indexed {
        base: Box::new(SpaceSpec::DeviceRGB),
        hival: 1,
        lookup: vec![0, 0, 0, 255, 255, 255],
    })));
}

#[test]
fn bound_tint_transforms_capture_operator_names() {
    let run = exec(
        "[/Separation (Ink) /DeviceGray {1 exch sub} bind] setcolorspace currentcolorspace ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "[/Separation /Ink /DeviceGray {1 exch sub}]\n");
}

#[test]
fn colour_space_errors() {
    for (program, error) in [
        ("/Pattern setcolorspace", "undefined"),
        ("[/CIEBasedABC << >>] setcolorspace", "undefined"),
        ("[/Separation /S /DeviceGray 3] setcolorspace", "typecheck"),
        ("[/Separation /S] setcolorspace", "rangecheck"),
        ("[/Indexed /DeviceRGB 1 (abc)] setcolorspace", "rangecheck"),
        (
            "[/Indexed /DeviceRGB 5000 (abc)] setcolorspace",
            "rangecheck",
        ),
        (
            "[/Indexed /DeviceGray 1 {pop 0}] setcolorspace",
            "typecheck",
        ),
        ("[/DeviceN [] /DeviceGray {}] setcolorspace", "rangecheck"),
        ("[/DeviceRGB 1] setcolorspace", "rangecheck"),
        ("1 setcolorspace", "typecheck"),
        ("/DeviceRGB setcolorspace 1 2 setcolor", "stackunderflow"),
        ("/DeviceRGB setcolorspace 1 (x) 3 setcolor", "typecheck"),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
}

// --- images -------------------------------------------------------------------------

fn image_calls(run: &Run) -> Vec<(ImageSpec, Vec<u8>)> {
    run.calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Image(spec, data) | Call::ImageMask(spec, data) => Some((spec, data)),
            _ => None,
        })
        .collect()
}

#[test]
fn image_data_from_a_procedure_stops_at_the_byte_count() {
    let run = exec(
        "/calls 0 def 16 4 8 [16 0 0 -4 0 4] { /calls calls 1 add def (0123456789abcdef) } image calls =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "4\n");
    let images = image_calls(&run);
    assert_eq!(images.len(), 1);
    let (spec, data) = &images[0];
    assert_eq!(data.len(), 64);
    assert_eq!(&data[..16], b"0123456789abcdef");
    assert_eq!(
        (spec.width, spec.height, spec.bits_per_component),
        (16, 4, 8)
    );
    assert_eq!(spec.color_space, Some(SpaceSpec::DeviceGray));
    assert_eq!(spec.decode, vec![0.0, 1.0]);
    assert_eq!(spec.matrix, Matrix([16.0, 0.0, 0.0, -4.0, 0.0, 4.0]));
    assert!(!spec.is_mask);

    // Rows pad to a byte boundary; chunks larger than needed are trimmed.
    let run = exec("10 3 true [10 0 0 3 0 0] { (\\000\\377\\000\\377) } imagemask");
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(data.len(), 6);
    assert_eq!(spec.decode, vec![1.0, 0.0]);
    assert!(spec.is_mask);
    assert_eq!(spec.color_space, None);

    // An empty chunk ends the image early; whole rows only.
    let run = exec(
        "/n 0 def 4 4 8 [4 0 0 4 0 0] { /n n 1 add def n 3 lt { (abcdef) } { () } ifelse } image",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.height, 3);
    assert_eq!(data, b"abcdefabcdef");

    // Errors inside the data procedure name the image operator.
    let run = exec("2 2 8 [2 0 0 2 0 0] { 42 } image");
    assert_eq!(run.error(), Some("typecheck"));
    assert_eq!(run.command(), Some("image"));
    let run = exec("2 2 true [2 0 0 2 0 0] { } imagemask");
    assert_eq!(run.error(), Some("stackunderflow"));
    assert_eq!(run.command(), Some("imagemask"));
}

#[test]
fn image_data_from_strings_and_files() {
    let run = exec("/DeviceRGB setcolorspace 2 2 8 [2 0 0 2 0 0] (0123456789ab) image");
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.components(), 3);
    assert_eq!(data, b"0123456789ab");

    let run = exec("2 2 8 [2 0 0 2 0 0] (0123456789ab) image");
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(data, b"0123");
    assert_eq!(spec.height, 2);

    struct Bytes(Vec<u8>);
    impl Stream for Bytes {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
            let n = buf.len().min(self.0.len()).min(3);
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0.drain(..n);
            Ok(n)
        }
        fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
            Err(VmError::InvalidAccess)
        }
    }
    let io = Io {
        stdin: Some(Box::new(Bytes(b"abcdefghijklmnop".to_vec()))),
        ..Default::default()
    };
    let run = run_with(
        "3 3 8 [3 0 0 3 0 0] (%stdin) (r) file image (%stdin) (r) file 5 string readstring pop ==",
        io,
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(image_calls(&run)[0].1, b"abcdefghi");
    assert_eq!(run.output, "(jklmn)\n");
}

#[test]
fn image_dictionary_form() {
    let run = exec(
        "<< /ImageType 1 /Width 4 /Height 2 /BitsPerComponent 8 /Decode [1 0] \
         /ImageMatrix [4 0 0 -2 0 2] /DataSource (12345678) /Interpolate true \
         /MultipleDataSources false >> image",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(
        *spec,
        ImageSpec {
            width: 4,
            height: 2,
            bits_per_component: 8,
            color_space: Some(SpaceSpec::DeviceGray),
            decode: vec![1.0, 0.0],
            matrix: Matrix([4.0, 0.0, 0.0, -2.0, 0.0, 2.0]),
            interpolate: true,
            is_mask: false,
        }
    );
    assert_eq!(data, b"12345678");

    let run = exec(
        "[/Indexed /DeviceRGB 3 <000000ff0000 00ff00 0000ff>] setcolorspace \
         << /ImageType 1 /Width 4 /Height 1 /BitsPerComponent 2 \
         /ImageMatrix [4 0 0 1 0 0] /DataSource <1b> >> image",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.decode, vec![0.0, 3.0]);
    assert_eq!(data, &[0x1b]);

    let run = exec(
        "<< /ImageType 1 /Width 2 /Height 1 /BitsPerComponent 1 /ImageMatrix [2 0 0 1 0 0] \
         /DataSource <80> >> imagemask",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(image_calls(&run)[0].0.is_mask);

    for (program, error) in [
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 8 /ImageMatrix [2 0 0 1 0 0] /DataSource <80> /ImageType 3 >> image",
            "rangecheck",
        ),
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 8 /ImageMatrix [2 0 0 1 0 0] /DataSource [<80>] /MultipleDataSources true >> image",
            "typecheck",
        ),
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 3 /ImageMatrix [2 0 0 1 0 0] /DataSource <80> >> image",
            "rangecheck",
        ),
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 8 /ImageMatrix [2 0 0 1 0 0] /DataSource <80> >> imagemask",
            "rangecheck",
        ),
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 8 /DataSource <80> >> image",
            "typecheck",
        ),
        (
            "<< /Width 2 /Height 1 /BitsPerComponent 8 /ImageMatrix [2 0 0 1 0 0] /DataSource <80> /Decode [0 1 0 1] >> image",
            "rangecheck",
        ),
        ("2 -1 8 [2 0 0 1 0 0] <80> image", "rangecheck"),
        ("2 1 8 [2 0 0 1 0 0] 7 image", "typecheck"),
        ("2 1 8 [2 0 0 1 0 0] [<80>] image", "typecheck"),
        ("2 1 5 [2 0 0 1 0 0] <80> imagemask", "typecheck"),
        ("colorimage", "undefined"),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
}

// --- page device -------------------------------------------------------------------

// setpagedevice-unknown-keys.ps
#[test]
fn setpagedevice_records_and_sets_the_media_box() {
    let run = exec(
        "<< /PageSize [612 792] /TraySwitch true >> setpagedevice \
         currentpagedevice /TraySwitch get == \
         << /PageSize [200 100] >> setpagedevice currentpagedevice /PageSize get == \
         currentpagedevice /TraySwitch get ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\n[200 100]\ntrue\n");
    let boxes: Vec<Bounds> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::MediaBox(b) => Some(b),
            _ => None,
        })
        .collect();
    assert_eq!(
        boxes,
        [
            Bounds::new(0.0, 0.0, 612.0, 792.0),
            Bounds::new(0.0, 0.0, 612.0, 792.0),
            Bounds::new(0.0, 0.0, 200.0, 100.0),
        ]
    );
    let run = exec("<< /PageSize 5 >> setpagedevice");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("<< /PageSize [1 2 3] >> setpagedevice");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("currentpagedevice /Foo 1 put");
    assert_eq!(run.error(), Some("invalidaccess"));
    let run = exec(
        "<< /Nested << /Deep [(s) {x}] >> >> setpagedevice \
         currentpagedevice /Nested get /Deep get dup gcheck = 0 get ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\n(s)\n");
}

// --- save/restore -------------------------------------------------------------------

#[test]
fn restore_restores_the_graphics_state() {
    let run = exec("1 setlinewidth save 5 setlinewidth restore currentlinewidth =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "1.0\n");
    let calls = run.calls();
    assert_eq!(calls[1], Call::LineWidth(1.0));
    assert_eq!(calls[2], Call::GSave);
    assert_eq!(calls[3], Call::LineWidth(5.0));
    assert_eq!(calls[4], Call::GRestoreTo(0));

    // Nested saves and gsaves round-trip to the depth of the restored save.
    let run = exec(
        "save gsave gsave save gsave 7 setlinewidth restore currentlinewidth = restore \
         currentlinewidth =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "1.0\n1.0\n");
    assert!(run.calls().contains(&Call::GRestoreTo(3)));
    assert!(run.calls().contains(&Call::GRestoreTo(0)));

    let run = exec(
        "save 2 setlinewidth save 3 setlinewidth restore currentlinewidth = \
         restore currentlinewidth =",
    );
    assert_eq!(run.output, "2.0\n1.0\n");

    // Restoring the outer save discards the inner one.
    let run = exec("save save exch restore restore");
    assert_eq!(run.error(), Some("invalidrestore"));
}

#[test]
fn grestore_clamps_at_the_save_floor() {
    let run = exec(
        "1 setlinewidth save 5 setlinewidth grestore currentlinewidth = \
         6 setlinewidth grestore grestore currentlinewidth = restore currentlinewidth =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "1.0\n1.0\n1.0\n");
    let calls = run.calls();
    // At the floor a grestore is a pop followed by a push of the same state.
    assert_eq!(
        &calls[2..6],
        [
            Call::GSave,
            Call::LineWidth(5.0),
            Call::GRestore,
            Call::GSave
        ]
    );
    assert!(calls.contains(&Call::GRestoreTo(0)));

    // grestore on an empty stack is a no-op; grestoreall pops to the floor.
    let run = exec("grestore gsave gsave 3 setlinewidth grestoreall currentlinewidth =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "1.0\n");
    assert_eq!(run.calls()[1], Call::GSave);
    assert!(run.calls().contains(&Call::GRestoreTo(0)));

    let run = exec(
        "2 setlinewidth save gsave gsave 3 setlinewidth grestoreall currentlinewidth = \
         gsave 4 setlinewidth grestoreall currentlinewidth = restore",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "2.0\n2.0\n");
}

#[test]
fn saves_before_installation_are_harmless() {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    assert_eq!(interp.run(&mut SliceSource::new(b"save")), Outcome::Ok);
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    let outcome = interp.run(&mut SliceSource::new(
        b"gsave 3 setlinewidth grestore currentlinewidth = restore currentlinewidth =",
    ));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "1.0\n1.0\n");
    assert!(log.borrow().contains(&Call::GRestoreTo(0)));
}

// --- pages ----------------------------------------------------------------------------

#[test]
fn page_operators_dispatch() {
    let run = exec("showpage copypage erasepage nulldevice");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        &run.calls()[1..],
        [
            Call::ShowPage,
            Call::CopyPage,
            Call::ErasePage,
            Call::NullDevice
        ]
    );
}

#[test]
fn backend_errors_name_the_operator() {
    struct Failing(Recording);
    impl GraphicsBackend for Failing {
        fn gsave(&mut self) -> Result<(), VmError> {
            Err(VmError::LimitCheck)
        }
        fn grestore(&mut self) -> Result<(), VmError> {
            self.0.grestore()
        }
        fn gstate_depth(&self) -> usize {
            self.0.gstate_depth()
        }
        fn grestore_to(&mut self, depth: usize) -> Result<(), VmError> {
            self.0.grestore_to(depth)
        }
        fn initgraphics(&mut self) -> Result<(), VmError> {
            self.0.initgraphics()
        }
        fn set_line_width(&mut self, w: f32) -> Result<(), VmError> {
            self.0.set_line_width(w)
        }
        fn line_width(&self) -> f32 {
            self.0.line_width()
        }
        fn set_line_cap(&mut self, c: LineCap) -> Result<(), VmError> {
            self.0.set_line_cap(c)
        }
        fn line_cap(&self) -> LineCap {
            self.0.line_cap()
        }
        fn set_line_join(&mut self, j: LineJoin) -> Result<(), VmError> {
            self.0.set_line_join(j)
        }
        fn line_join(&self) -> LineJoin {
            self.0.line_join()
        }
        fn set_miter_limit(&mut self, l: f32) -> Result<(), VmError> {
            self.0.set_miter_limit(l)
        }
        fn miter_limit(&self) -> f32 {
            self.0.miter_limit()
        }
        fn set_dash(&mut self, a: &[f32], p: f32) -> Result<(), VmError> {
            self.0.set_dash(a, p)
        }
        fn dash(&self) -> (Vec<f32>, f32) {
            self.0.dash()
        }
        fn set_flatness(&mut self, f: f32) -> Result<(), VmError> {
            self.0.set_flatness(f)
        }
        fn flatness(&self) -> f32 {
            self.0.flatness()
        }
        fn concat(&mut self, m: Matrix) -> Result<(), VmError> {
            self.0.concat(m)
        }
        fn set_matrix(&mut self, m: Matrix) -> Result<(), VmError> {
            self.0.set_matrix(m)
        }
        fn current_matrix(&self) -> Matrix {
            self.0.current_matrix()
        }
        fn default_matrix(&self) -> Matrix {
            self.0.default_matrix()
        }
        fn set_color_space(&mut self, s: &SpaceSpec) -> Result<(), VmError> {
            self.0.set_color_space(s)
        }
        fn set_color(&mut self, c: &[f32]) -> Result<(), VmError> {
            self.0.set_color(c)
        }
        fn current_color_space(&self) -> SpaceSpec {
            self.0.current_color_space()
        }
        fn current_color(&self) -> Vec<f32> {
            self.0.current_color()
        }
        fn newpath(&mut self) -> Result<(), VmError> {
            self.0.newpath()
        }
        fn moveto(&mut self, p: Point) -> Result<(), VmError> {
            self.0.moveto(p)
        }
        fn lineto(&mut self, p: Point) -> Result<(), VmError> {
            self.0.lineto(p)
        }
        fn curveto(&mut self, a: Point, b: Point, c: Point) -> Result<(), VmError> {
            self.0.curveto(a, b, c)
        }
        fn closepath(&mut self) -> Result<(), VmError> {
            self.0.closepath()
        }
        fn arc(&mut self, c: Point, r: f32, s: f32, e: f32) -> Result<(), VmError> {
            self.0.arc(c, r, s, e)
        }
        fn arcn(&mut self, c: Point, r: f32, s: f32, e: f32) -> Result<(), VmError> {
            self.0.arcn(c, r, s, e)
        }
        fn arcto(&mut self, a: Point, b: Point, r: f32) -> Result<(Point, Point), VmError> {
            self.0.arcto(a, b, r)
        }
        fn current_point(&self) -> Result<Point, VmError> {
            self.0.current_point()
        }
        fn path_bbox(&self) -> Result<Bounds, VmError> {
            self.0.path_bbox()
        }
        fn fill(&mut self) -> Result<(), VmError> {
            Err(VmError::IoError)
        }
        fn eofill(&mut self) -> Result<(), VmError> {
            self.0.eofill()
        }
        fn stroke(&mut self) -> Result<(), VmError> {
            self.0.stroke()
        }
        fn rectfill(&mut self, r: &[Rect]) -> Result<(), VmError> {
            self.0.rectfill(r)
        }
        fn rectstroke(&mut self, r: &[Rect]) -> Result<(), VmError> {
            self.0.rectstroke(r)
        }
        fn rectclip(&mut self, r: &[Rect]) -> Result<(), VmError> {
            self.0.rectclip(r)
        }
        fn clip(&mut self) -> Result<(), VmError> {
            self.0.clip()
        }
        fn eoclip(&mut self) -> Result<(), VmError> {
            self.0.eoclip()
        }
        fn initclip(&mut self) -> Result<(), VmError> {
            self.0.initclip()
        }
        fn clippath(&mut self) -> Result<Vec<Seg>, VmError> {
            self.0.clippath()
        }
        fn image(&mut self, s: &ImageSpec, d: &[u8]) -> Result<(), VmError> {
            self.0.image(s, d)
        }
        fn imagemask(&mut self, s: &ImageSpec, d: &[u8]) -> Result<(), VmError> {
            self.0.imagemask(s, d)
        }
        fn set_media_box(&mut self, b: Bounds) -> Result<(), VmError> {
            self.0.set_media_box(b)
        }
        fn showpage(&mut self) -> Result<(), VmError> {
            self.0.showpage()
        }
        fn copypage(&mut self) -> Result<(), VmError> {
            self.0.copypage()
        }
        fn erasepage(&mut self) -> Result<(), VmError> {
            self.0.erasepage()
        }
        fn nulldevice(&mut self) -> Result<(), VmError> {
            self.0.nulldevice()
        }
    }

    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Failing(Recording::new(log))));
    let outcome = interp.run(&mut SliceSource::new(b"0 0 moveto fill"));
    assert!(matches!(&outcome, Outcome::Error(e) if e.name == "ioerror" && e.command == "fill"));
    // A save whose graphics save fails leaves no save record behind.
    let outcome = interp.run(&mut SliceSource::new(b"save"));
    assert!(matches!(&outcome, Outcome::Error(e) if e.name == "limitcheck" && e.command == "save"));
    assert_eq!(interp.memory().save_depth(), 0);
    assert!(interp.ostack().iter().all(|o| o.ty() != ps_vm::Type::Save));
    let _ = Object::null();
}

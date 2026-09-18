// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The graphics boundary, checked with a recording backend: every call is
//! logged as a value, queries are answered from a small internal state,
//! and nothing draws. Scenarios that depend on the IR belong to the
//! graphics crate.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_vm::{
    Bounds, Config, Encoded, FontRef, Glyph, GraphicsBackend, ImageSpec, Interp, Io, LineCap,
    LineJoin, Matrix, Object, Outcome, Point, Rect, Seg, SliceSource, SpaceSpec, Stream, VmError,
};

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
    let out = efterscript_vm::Capture::new();
    io.stdout = Some(Box::new(out.clone()));
    io.stderr = Some(Box::new(efterscript_vm::Capture::new()));
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
        ("[/CIEBasedABC << >>] setcolorspace", "undefined"),
        ("[/Pattern [/Pattern]] setcolorspace", "typecheck"),
        (
            "[/Pattern [/Pattern /DeviceRGB]] setcolorspace",
            "typecheck",
        ),
        (
            "[/Pattern /DeviceRGB /DeviceGray] setcolorspace",
            "rangecheck",
        ),
        ("[/Pattern 5] setcolorspace", "typecheck"),
        ("/Pattern setcolorspace 5 setcolor", "typecheck"),
        (
            "[/Pattern /DeviceRGB] setcolorspace 1 0 0 setcolor",
            "typecheck",
        ),
        ("[/Pattern] setcolorspace << >> setcolor", "undefined"),
        (
            "[/Pattern] setcolorspace << /PatternType 1 >> setcolor",
            "undefined",
        ),
        (
            "[/Pattern] setcolorspace << /Implementation 0 >> setcolor",
            "typecheck",
        ),
        ("[/Pattern] setcolorspace setcolor", "stackunderflow"),
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

/// The pattern space is accepted by name and in both array forms; its
/// initial colour is the base's, and `currentcolor` reports the null
/// instance of PLRM3 §4.9.1 until an instance exists.
#[test]
fn pattern_space_is_selected_and_reported() {
    let run = exec(
        "[/Pattern] setcolorspace currentcolorspace == currentcolor == count == \
         /Pattern setcolorspace currentcolorspace == \
         [/Pattern /DeviceRGB] setcolorspace currentcolorspace == currentcolor == count == \
         currentrgbcolor == == == \
         [/Pattern [/Indexed /DeviceRGB 1 <000000ffffff>]] setcolorspace currentcolor == \
         /Pattern /ColorSpaceFamily resourcestatus pop pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "[/Pattern]\nnull\n0\n[/Pattern]\n[/Pattern /DeviceRGB]\nnull\n0\n0.0\n0.0\n0.0\nnull\n0\n"
    );
    let spaces: Vec<SpaceSpec> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::ColorSpace(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(
        spaces,
        [
            SpaceSpec::Pattern { base: None },
            SpaceSpec::Pattern { base: None },
            SpaceSpec::Pattern {
                base: Some(Box::new(SpaceSpec::DeviceRGB)),
            },
            SpaceSpec::Pattern {
                base: Some(Box::new(SpaceSpec::Indexed {
                    base: Box::new(SpaceSpec::DeviceRGB),
                    hival: 1,
                    lookup: vec![0, 0, 0, 255, 255, 255],
                })),
            },
        ]
    );
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
    // The operand form's samples are gray whatever the current space;
    // the dictionary form takes the current space.
    let run = exec("/DeviceRGB setcolorspace 2 2 8 [2 0 0 2 0 0] (0123456789ab) image");
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.components(), 1);
    assert_eq!(data, b"0123");
    let run = exec(
        "/DeviceRGB setcolorspace << /ImageType 1 /Width 2 /Height 2 /BitsPerComponent 8 /ImageMatrix [2 0 0 2 0 0] /DataSource (0123456789ab) >> image",
    );
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

/// A marker stream the walker accepts: SOI, a segment, a scan with a
/// stuffed byte, EOI.
const DCT_HEX: &str = "FFD8FFDB0004AABBFFDA0003011234FF00FFD9";

fn dct_bytes() -> Vec<u8> {
    (0..DCT_HEX.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&DCT_HEX[i..i + 2], 16).unwrap())
        .collect()
}

#[test]
fn a_dct_source_is_passed_through_encoded() {
    // The stream comes through a hexadecimal layer over the job; the
    // image reads it to its end-of-image marker, the hexadecimal layer
    // is read through its `>`, and the program continues.
    let program = format!(
        "<< /ImageType 1 /Width 16 /Height 16 /BitsPerComponent 8 \
         /ImageMatrix [16 0 0 -16 0 16] \
         /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
         {DCT_HEX}>\n(after) ="
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "after\n");
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.encoded, Some(Encoded::Dct));
    assert_eq!(
        (spec.width, spec.height, spec.bits_per_component),
        (16, 16, 8)
    );
    assert_eq!(spec.color_space, Some(SpaceSpec::DeviceGray));
    assert_eq!(*data, dct_bytes());
}

#[test]
fn a_truncated_dct_source_ends_at_the_source_end_and_a_broken_one_is_ioerror() {
    let run = exec(
        "<< /ImageType 1 /Width 2 /Height 2 /BitsPerComponent 8 /ImageMatrix [2 0 0 2 0 0] \
         /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
         FFD8FFDB0004AA",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let (spec, data) = &image_calls(&run)[0];
    assert_eq!(spec.encoded, Some(Encoded::Dct));
    assert_eq!(*data, [0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0xAA]);
    let run = exec(
        "<< /ImageType 1 /Width 2 /Height 2 /BitsPerComponent 8 /ImageMatrix [2 0 0 2 0 0] \
         /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
         FFD8FFDB0004AABB12",
    );
    assert_eq!(run.error(), Some("ioerror"));
    assert_eq!(run.command(), Some("image"));
    // A mask cannot be DCT-encoded: its samples are read, which the
    // placeholder decoder refuses.
    let run = exec(
        "<< /ImageType 1 /Width 8 /Height 1 /BitsPerComponent 1 /ImageMatrix [8 0 0 1 0 0] \
         /Decode [0 1] /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> imagemask\n\
         FFD8FFD9>",
    );
    assert_eq!(run.error(), Some("undefined"));
    // A plain image's spec is not flagged.
    let run = exec("2 2 8 [2 0 0 2 0 0] <00112233> image");
    assert_eq!(image_calls(&run)[0].0.encoded, None);
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
            encoded: None,
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
        ("colorimage", "stackunderflow"),
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
        fn set_font(&mut self, f: Option<FontRef>) -> Result<(), VmError> {
            self.0.set_font(f)
        }
        fn font(&self) -> Option<FontRef> {
            self.0.font()
        }
        fn show(&mut self, g: &[Glyph]) -> Result<(), VmError> {
            self.0.show(g)
        }
        fn begin_glyph(&mut self, f: FontRef, c: u8, n: &[u8], m: bool) -> Result<(), VmError> {
            self.0.begin_glyph(f, c, n, m)
        }
        fn end_glyph(&mut self, w: (f32, f32), b: Option<Bounds>) -> Result<(), VmError> {
            self.0.end_glyph(w, b)
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
    assert!(
        interp
            .ostack()
            .iter()
            .all(|o| o.ty() != efterscript_vm::Type::Save)
    );
    let _ = Object::null();
}

#[test]
fn setpagedevice_type_checks_the_keys_it_recognises() {
    let run = exec(
        "<< /InputAttributes << /Priority [0] >> /Duplex false /NumCopies 2 /ImagingBBox null \
         /HWResolution [300 300] /Orientation 1 /Policies << /PageSize 3 >> /Tumble true \
         /Collate false /OutputAttributes 1 dict /PageOffset [0 0] /TraySwitch (any) >> \
         setpagedevice currentpagedevice /ImagingBBox get null eq = \
         currentpagedevice /NumCopies get =",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "true\n2\n");
    for request in [
        "/InputAttributes (tray)",
        "/OutputAttributes [1]",
        "/Policies 1",
        "/Duplex 1",
        "/Collate (yes)",
        "/Tumble null",
        "/NumCopies 1.5",
        "/Orientation (portrait)",
        "/ImagingBBox 1",
        "/HWResolution (300)",
        "/PageOffset << >>",
    ] {
        let run = exec(&format!(
            "<< /PageSize [100 100] {request} >> setpagedevice"
        ));
        assert_eq!(run.error(), Some("typecheck"), "{request}");
        assert_eq!(run.command(), Some("setpagedevice"), "{request}");
        assert_eq!(
            run.interp.ostack().len(),
            1,
            "{request}: the request stays on the stack"
        );
    }
    // Nothing of a refused request is recorded.
    let run = exec(
        "{ << /Duplex true /Collate (no) >> setpagedevice } stopped pop \
         currentpagedevice /Duplex known =",
    );
    assert_eq!(run.output, "false\n");
}

// colorimage-rgb.ps, colorimage-planes.ps
#[test]
fn colorimage_paints_device_samples_from_one_source_or_planes() {
    let single = exec(
        "0 0 0 1 setcmykcolor 2 2 8 [2 0 0 -2 0 2] <FF000000FF000000FFFFFFFF> false 3 colorimage",
    );
    assert_eq!(single.error(), None);
    let planes = exec(
        "/r <FF0000FF> def /g <00FF00FF> def /b <0000FFFF> def \
         2 2 8 [2 0 0 -2 0 2] { r } { g } { b } true 3 colorimage",
    );
    assert_eq!(planes.error(), None);
    let images = image_calls(&single);
    assert_eq!(images, image_calls(&planes));
    let (spec, data) = &images[0];
    assert_eq!(spec.color_space, Some(SpaceSpec::DeviceRGB));
    assert_eq!(spec.bits_per_component, 8);
    assert_eq!(spec.decode, vec![0.0, 1.0, 0.0, 1.0, 0.0, 1.0]);
    assert_eq!(data, &[255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255]);
    let strings = exec("2 2 8 [2 0 0 -2 0 2] <FF0000FF> <00FF00FF> <0000FFFF> true 3 colorimage");
    assert_eq!(image_calls(&strings), images);
    let gray = exec("1 1 8 [1 0 0 1 0 0] <80> false 1 colorimage");
    assert_eq!(
        image_calls(&gray)[0].0.color_space,
        Some(SpaceSpec::DeviceGray)
    );
    let cmyk = exec("1 1 8 [1 0 0 1 0 0] <00000080> false 4 colorimage");
    assert_eq!(
        image_calls(&cmyk)[0].0.color_space,
        Some(SpaceSpec::DeviceCMYK)
    );
    for (program, error) in [
        ("1 1 8 [1 0 0 1 0 0] <00> false 2 colorimage", "rangecheck"),
        (
            "1 1 1 [1 0 0 1 0 0] <00> <00> <00> true 3 colorimage",
            "limitcheck",
        ),
        (
            "1 1 8 [1 0 0 1 0 0] <00> { } <00> true 3 colorimage",
            "typecheck",
        ),
        ("1 1 8 [1 0 0 1 0 0] 5 false 1 colorimage", "typecheck"),
    ] {
        assert_eq!(exec(program).error(), Some(error), "{program}");
    }
}

// --- stroke adjustment, overprint, and the page device in the state ------------

// overprint-round-trip.ps
#[test]
fn stroke_adjust_and_overprint_live_in_the_graphics_state() {
    let run = exec(
        "currentstrokeadjust = currentoverprint = \
         true setstrokeadjust true setoverprint \
         gsave false setstrokeadjust false setoverprint \
         currentstrokeadjust = currentoverprint = grestore \
         currentstrokeadjust = currentoverprint = \
         initgraphics currentstrokeadjust = currentoverprint = \
         save false setoverprint false setstrokeadjust restore \
         currentstrokeadjust = currentoverprint =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "false\nfalse\nfalse\nfalse\ntrue\ntrue\ntrue\ntrue\ntrue\ntrue\n"
    );
    // The backend hears every setoverprint and the restorations that
    // change the value; stroke adjustment never crosses the boundary.
    let overprints: Vec<bool> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Overprint(on) => Some(on),
            _ => None,
        })
        .collect();
    assert_eq!(overprints, [true, false, true, false, true]);
    assert!(run.interp.stroke_adjust());
    assert!(run.interp.overprint());

    let run = exec("1 setstrokeadjust");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("(x) setoverprint");
    assert_eq!(run.error(), Some("typecheck"));
    assert_eq!(run.top_numbers(0), Vec::<f32>::new());
    // A failing setter leaves its operand in place.
    assert_eq!(run.interp.ostack().len(), 1);
}

#[test]
fn a_glyph_procedure_starts_without_stroke_adjustment() {
    let run = exec(
        "/T << /FontType 3 /FontMatrix [0.001 0 0 0.001 0 0] /Encoding 256 array \
         /BuildGlyph { pop pop 500 0 setcharwidth /seen currentstrokeadjust def \
         true setstrokeadjust } >> definefont 10 scalefont setfont \
         true setstrokeadjust 0 0 moveto (a) show seen = currentstrokeadjust = \
         false setstrokeadjust 0 0 moveto (a) show seen = currentstrokeadjust =",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(run.output, "false\ntrue\nfalse\nfalse\n");
}

// pagedevice-in-gstate.ps, pagedevice-merges.ps
#[test]
fn the_page_device_follows_the_graphics_state() {
    let run = exec(
        "currentpagedevice /PageSize get == \
         gsave << /PageSize [200 200] /Duplex true >> setpagedevice \
         currentpagedevice /PageSize get == \
         gsave << /PageSize [100 50] >> setpagedevice grestore \
         currentpagedevice /PageSize get == currentpagedevice /Duplex get == \
         grestore currentpagedevice /PageSize get == currentpagedevice /Duplex known == \
         save << /PageSize [300 300] >> setpagedevice \
         gsave << /PageSize [400 400] >> setpagedevice \
         restore currentpagedevice /PageSize get == \
         gsave gsave << /PageSize [500 500] >> setpagedevice grestoreall \
         currentpagedevice /PageSize get ==",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(
        run.output,
        "[612 792]\n[200 200]\n[200 200]\ntrue\n[612 792]\nfalse\n[612 792]\n[612 792]\n"
    );
    // The media box the backend is told: at installation, on each
    // setpagedevice, and on each restoration that changes the device.
    let boxes: Vec<Bounds> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::MediaBox(b) => Some(b),
            _ => None,
        })
        .collect();
    let b = |w, h| Bounds::new(0.0, 0.0, w, h);
    assert_eq!(
        boxes,
        [
            b(612.0, 792.0),
            b(200.0, 200.0),
            b(100.0, 50.0),
            b(200.0, 200.0),
            b(612.0, 792.0),
            b(300.0, 300.0),
            b(400.0, 400.0),
            b(612.0, 792.0),
            b(500.0, 500.0),
            b(612.0, 792.0),
        ]
    );
}

#[test]
fn each_setpagedevice_installs_a_fresh_read_only_dictionary() {
    let run = exec(
        "currentpagedevice << /PageSize [200 200] >> setpagedevice currentpagedevice \
         2 copy eq = exch /PageSize get == /PageSize get == \
         currentpagedevice wcheck = \
         { << /PageSize [1 2 3] >> setpagedevice } stopped pop \
         currentpagedevice /PageSize get ==",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(
        run.output,
        "false\n[612 792]\n[200 200]\nfalse\n[200 200]\n"
    );
}

// --- pathbbox and the declared box ----------------------------------------------

// pathbbox-rules.ps
#[test]
fn a_declared_setbbox_answers_pathbbox() {
    // The recording backend answers pathbbox with the current point's
    // box, so anything else comes from the declared box.
    let run = exec(
        "10 10 20 20 setbbox 12 12 moveto pathbbox \
         newpath 12 12 moveto pathbbox",
    );
    assert_eq!(run.outcome, Outcome::Ok, "{:?}", run.outcome);
    assert_eq!(
        run.top_numbers(8),
        [10.0, 10.0, 20.0, 20.0, 12.0, 12.0, 12.0, 12.0]
    );
    // The declared box is derived from its device-space envelope under
    // the CTM at the declaration, through the inverse of the current one.
    let run = exec("2 2 scale 10 10 20 20 setbbox 12 12 moveto 1 1 translate pathbbox");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.top_numbers(4), [9.0, 9.0, 19.0, 19.0]);
    let run = exec("45 rotate 0 0 10 10 setbbox 5 5 moveto pathbbox");
    assert_eq!(run.outcome, Outcome::Ok);
    let got = run.top_numbers(4);
    let want = [-5.0, -5.0, 15.0, 15.0];
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() < 1e-3, "{got:?}");
    }
    // Without a current point the declared box does not answer.
    let run = exec("10 10 20 20 setbbox pathbbox");
    assert_eq!(run.error(), Some("nocurrentpoint"));
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Tiling patterns at the boundary (PLRM3 §4.9): `makepattern`'s
//! instance, `setpattern` and `setcolor` with an instance, the shape of
//! `currentcolor`, the capture of a cell at the first paint, the
//! uncoloured colour-operator rule, and what an error inside the paint
//! procedure leaves behind.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use ps_vm::{Config, Interp, Io, Matrix, Outcome, Rect, SliceSource, SpaceSpec};

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
}

fn exec(program: &str) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
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

fn exec_without_backend(program: &str) -> (Outcome, String) {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    (outcome, out.text())
}

const PROTO: &str = "<< /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
     /XStep 10 /YStep 10 /PaintProc { pop 0 0 5 5 rectfill } >>";
const UNCOLOURED: &str = "<< /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 10 10] \
     /XStep 10 /YStep 10 /PaintProc { pop 0 0 5 5 rectfill } >>";

fn defs() -> String {
    format!("/D {PROTO} def /P D matrix makepattern def /U {UNCOLOURED} matrix makepattern def ")
}

fn with_defs(program: &str) -> Run {
    exec(&format!("{}{program}", defs()))
}

fn cell(x: f32, y: f32, w: f32, h: f32) -> Call {
    Call::RectFill(vec![Rect {
        x,
        y,
        width: w,
        height: h,
    }])
}

fn matrix_approx(a: Matrix, b: Matrix) -> bool {
    a.0.iter().zip(b.0).all(|(x, y)| (x - y).abs() < 1e-4)
}

// --- makepattern ---------------------------------------------------------------

#[test]
fn makepattern_makes_a_read_only_local_copy_with_an_implementation_entry() {
    let run = with_defs(
        "P /PatternType get = D length = P length = D /Implementation known = \
         P /Implementation known = P wcheck = P gcheck = \
         P /PaintProc get D /PaintProc get eq = P type == \
         true setglobal D matrix makepattern gcheck = false setglobal",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "1\n7\n8\nfalse\ntrue\nfalse\nfalse\ntrue\ndicttype\nfalse\n"
    );
}

#[test]
fn makepattern_locks_the_instance_to_user_space() {
    let run = with_defs(
        "2 2 scale 10 20 translate D [1 0 0 1 5 5] makepattern setpattern 0 0 1 1 rectfill",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let info = run
        .calls()
        .into_iter()
        .find_map(|c| match c {
            Call::BeginPatternCell(info) => Some(info),
            _ => None,
        })
        .expect("the fill asks for the cell");
    assert!(matrix_approx(
        info.matrix,
        Matrix([2.0, 0.0, 0.0, 2.0, 30.0, 50.0])
    ));
    assert_eq!(info.xstep, 10.0);
    assert_eq!(info.paint_type, 1);
    // The instance id is the Implementation entry, and ids are never
    // reused: the third instance made is number 2.
    let run = with_defs("D matrix makepattern /Implementation get =");
    assert_eq!(run.output, "2\n");
}

#[test]
fn makepattern_works_without_a_backend_with_the_identity_ctm() {
    let (outcome, output) = exec_without_backend(&format!(
        "{PROTO} [2 0 0 2 0 0] makepattern dup /Implementation get = /PatternType get ="
    ));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(output, "0\n1\n");
    let (outcome, _) =
        exec_without_backend(&format!("{PROTO} [1 0 0 1 0 0] makepattern setpattern"));
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "undefined"));
}

#[test]
fn makepattern_errors() {
    let entries = [
        "/PatternType 1",
        "/PaintType 1",
        "/TilingType 1",
        "/BBox [0 0 10 10]",
        "/XStep 10",
        "/YStep 10",
        "/PaintProc { pop }",
    ];
    for missing in 0..entries.len() {
        let dict: Vec<&str> = entries
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != missing)
            .map(|(_, e)| *e)
            .collect();
        let run = exec(&format!("<< {} >> matrix makepattern", dict.join(" ")));
        assert_eq!(
            run.error(),
            Some("undefined"),
            "without {}",
            entries[missing]
        );
    }
    let base = |change: &str| {
        let dict: Vec<String> = entries
            .iter()
            .map(|e| {
                let key = e.split(' ').next().unwrap();
                if change.starts_with(key) {
                    change.to_string()
                } else {
                    (*e).to_string()
                }
            })
            .collect();
        format!("<< {} >> matrix makepattern", dict.join(" "))
    };
    for (program, error) in [
        (base("/PatternType 2"), "rangecheck"),
        (base("/PatternType (1)"), "typecheck"),
        (base("/PaintType 3"), "rangecheck"),
        (base("/TilingType 4"), "rangecheck"),
        (base("/XStep 0"), "rangecheck"),
        (base("/YStep (10)"), "typecheck"),
        (base("/BBox [0 0 0 10]"), "rangecheck"),
        (base("/BBox 5"), "typecheck"),
        (base("/BBox [0 0 10]"), "typecheck"),
        (base("/PaintProc [1 2]"), "typecheck"),
        (format!("{PROTO} 5 makepattern"), "typecheck"),
        (format!("{PROTO} [1 0 0 1 0] makepattern"), "rangecheck"),
        (format!("{PROTO} [1 0 0 1 0 (a)] makepattern"), "typecheck"),
        ("5 matrix makepattern".to_string(), "typecheck"),
        ("matrix makepattern".to_string(), "stackunderflow"),
    ] {
        let run = exec(&program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
    // A failing makepattern leaves its operands in place (the default
    // handler has popped the command).
    let run = exec(&format!("{PROTO} 5 makepattern"));
    assert_eq!(run.interp.ostack().len(), 2);
}

// --- setpattern and setcolor -----------------------------------------------------

fn pattern_calls(run: &Run) -> Vec<Call> {
    run.calls()
        .into_iter()
        .filter(|c| {
            matches!(
                c,
                Call::ColorSpace(_) | Call::SetPattern(..) | Call::Color(_)
            )
        })
        .collect()
}

#[test]
fn setpattern_selects_a_pattern_space_over_the_current_one_and_the_colour() {
    let run = with_defs(
        "1 0 0 setrgbcolor P setpattern currentcolorspace == \
         0 1 0.5 U setpattern currentcolorspace == \
         P setpattern currentcolorspace == \
         0.5 setgray 0.25 U setpattern currentcolorspace ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "[/Pattern /DeviceRGB]\n[/Pattern /DeviceRGB]\n[/Pattern /DeviceRGB]\n[/Pattern /DeviceGray]\n"
    );
    let calls = pattern_calls(&run);
    let rgb = SpaceSpec::Pattern {
        base: Some(Box::new(SpaceSpec::DeviceRGB)),
    };
    let gray = SpaceSpec::Pattern {
        base: Some(Box::new(SpaceSpec::DeviceGray)),
    };
    assert_eq!(calls.len(), 10);
    assert_eq!(calls[0], Call::ColorSpace(SpaceSpec::DeviceRGB));
    assert_eq!(calls[1], Call::Color(vec![1.0, 0.0, 0.0]));
    assert_eq!(calls[2], Call::ColorSpace(rgb));
    assert!(matches!(&calls[3], Call::SetPattern(info, c) if info.paint_type == 1 && c.is_empty()));
    // Already a pattern space: the base stays, no space is set.
    assert!(
        matches!(&calls[4], Call::SetPattern(info, c) if info.paint_type == 2 && *c == vec![0.0, 1.0, 0.5])
    );
    assert!(matches!(&calls[5], Call::SetPattern(info, c) if info.paint_type == 1 && c.is_empty()));
    assert_eq!(calls[6], Call::ColorSpace(SpaceSpec::DeviceGray));
    assert_eq!(calls[7], Call::Color(vec![0.5]));
    assert_eq!(calls[8], Call::ColorSpace(gray));
    assert!(matches!(&calls[9], Call::SetPattern(_, c) if *c == vec![0.25]));
}

#[test]
fn setcolor_takes_the_instance_in_a_pattern_space() {
    let run = with_defs(
        "[/Pattern /DeviceRGB] setcolorspace 0.2 0.4 0.6 U setcolor \
         [/Pattern] setcolorspace P setcolor \
         [/Pattern /DeviceCMYK] setcolorspace P setcolor count =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    let calls: Vec<Call> = run
        .calls()
        .into_iter()
        .filter(|c| matches!(c, Call::SetPattern(..)))
        .collect();
    assert!(matches!(&calls[0], Call::SetPattern(_, c) if *c == vec![0.2, 0.4, 0.6]));
    assert!(matches!(&calls[1], Call::SetPattern(_, c) if c.is_empty()));
    assert!(matches!(&calls[2], Call::SetPattern(_, c) if c.is_empty()));
}

#[test]
fn setpattern_and_setcolor_errors() {
    for (program, error) in [
        ("[/Pattern] setcolorspace U setpattern", "rangecheck"),
        ("[/Pattern] setcolorspace U setcolor", "rangecheck"),
        ("/DeviceRGB setcolorspace U setpattern", "stackunderflow"),
        (
            "/DeviceRGB setcolorspace 1 0 U setpattern",
            "stackunderflow",
        ),
        ("/DeviceRGB setcolorspace 1 (a) 0 U setpattern", "typecheck"),
        (
            "[/Pattern /DeviceGray] setcolorspace (a) U setcolor",
            "typecheck",
        ),
        ("5 setpattern", "typecheck"),
        ("D setpattern", "undefined"),
        ("<< /Implementation 99 >> setpattern", "typecheck"),
        ("<< /Implementation (x) >> setpattern", "typecheck"),
        ("[/Pattern] setcolorspace D setcolor", "undefined"),
        ("[/Pattern] setcolorspace 5 setcolor", "typecheck"),
        ("setpattern", "stackunderflow"),
    ] {
        let run = with_defs(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
    // A dictionary that copied an instance's entries is not the instance.
    let run = with_defs("P length dict P { 2 index 3 1 roll put } forall setpattern");
    assert_eq!(run.error(), Some("typecheck"));
}

#[test]
fn currentcolor_reports_the_components_then_the_instance() {
    let run = with_defs(
        "P setpattern currentcolor count = P eq = clear \
         0.5 setgray 0.25 U setpattern currentcolor count = U eq = = clear \
         1 0 0 setrgbcolor 0 1 0.5 U setpattern currentcolor count = U eq = = = = clear \
         [/Pattern /DeviceRGB] setcolorspace P setcolor currentcolor count = P eq = clear \
         [/Pattern] setcolorspace currentcolor count = null eq = clear \
         [/Pattern [/Indexed /DeviceRGB 1 <000000ffffff>]] setcolorspace 1 U setcolor \
         currentcolor pop == \
         gsave P setpattern grestore currentcolor pop ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output,
        "1\ntrue\n2\ntrue\n0.25\n4\ntrue\n0.5\n1.0\n0.0\n1\ntrue\n1\ntrue\n1\n1\n"
    );
}

// --- the cell -------------------------------------------------------------------

#[test]
fn the_first_paint_captures_the_cell_and_the_operator_runs_again() {
    let run = with_defs("P setpattern 0 0 100 100 rectfill 0 0 100 100 rectfill count =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    let calls = run.calls();
    let at = calls
        .iter()
        .position(|c| matches!(c, Call::SetPattern(..)))
        .unwrap();
    let Call::SetPattern(info, _) = calls[at] else {
        unreachable!()
    };
    assert_eq!(
        &calls[at + 1..],
        [
            Call::BeginPatternCell(info),
            Call::GSave,
            cell(0.0, 0.0, 5.0, 5.0),
            Call::EndPatternCell,
            Call::GRestoreTo(0),
            // The operator runs again: it asks once more (the page holds
            // the cell now) and paints.
            Call::BeginPatternCell(info),
            cell(0.0, 0.0, 100.0, 100.0),
            Call::BeginPatternCell(info),
            cell(0.0, 0.0, 100.0, 100.0),
        ]
    );
    assert!(run.interp.estack().is_empty());
    assert_eq!(run.interp.dstack().len(), 3);
}

#[test]
fn every_painting_operator_captures_and_the_measuring_ones_do_not() {
    let font = "/Helvetica 12 selectfont 10 10 moveto ";
    for program in [
        "0 0 moveto 10 10 lineto fill",
        "0 0 moveto 10 10 lineto eofill",
        "0 0 moveto 10 10 lineto stroke",
        "0 0 10 10 rectfill",
        "[0 0 10 10] rectstroke",
        "(a) show",
        "1 1 (a) ashow",
        "1 1 32 (a) widthshow",
        "1 1 32 1 1 (a) awidthshow",
        "{ pop pop } (ab) kshow",
        "(a) [5] xshow",
        "(a) [5] yshow",
        "(a) [5 5] xyshow",
        "/a glyphshow",
        "1 1 true [1 0 0 1 0 0] <00> imagemask",
        "{0 0 10 10 setbbox 0 0 moveto 5 5 lineto closepath} cvlit ufill",
        "{0 0 10 10 setbbox 0 0 moveto 5 5 lineto closepath} cvlit ueofill",
        "{0 0 10 10 setbbox 0 0 moveto 5 5 lineto} cvlit ustroke",
        "{0 0 10 10 setbbox 0 0 moveto 5 5 lineto} cvlit matrix ustroke",
    ] {
        let run = with_defs(&format!("P setpattern {font}{program} count ="));
        assert_eq!(run.outcome, Outcome::Ok, "{program}");
        assert_eq!(run.output, "0\n", "{program}");
        let calls = run.calls();
        let begins = calls
            .iter()
            .filter(|c| matches!(c, Call::BeginPatternCell(_)))
            .count();
        let ends = calls
            .iter()
            .filter(|c| matches!(c, Call::EndPatternCell))
            .count();
        assert_eq!((begins, ends), (2, 1), "{program}: {calls:?}");
        assert!(
            calls.contains(&cell(0.0, 0.0, 5.0, 5.0)),
            "{program}: the cell was painted"
        );
        let end = calls
            .iter()
            .position(|c| matches!(c, Call::EndPatternCell))
            .unwrap();
        let painted = calls[end..].iter().any(|c| {
            matches!(
                c,
                Call::Fill
                    | Call::EoFill
                    | Call::Stroke
                    | Call::RectFill(_)
                    | Call::RectStroke(_)
                    | Call::Show(_)
                    | Call::ImageMask(..)
            )
        });
        assert!(painted, "{program}: the operator ran again");
    }
    for program in [
        "(a) stringwidth pop pop",
        "(a) false charpath",
        "1 1 8 [1 0 0 1 0 0] <ff> image",
        "0 0 moveto 10 10 lineto clip",
    ] {
        let run = with_defs(&format!("P setpattern {font}{program}"));
        assert_eq!(run.outcome, Outcome::Ok, "{program}");
        assert!(
            !run.calls()
                .iter()
                .any(|c| matches!(c, Call::BeginPatternCell(_))),
            "{program}"
        );
    }
}

#[test]
fn an_uncoloured_cell_refuses_the_colour_operators() {
    for inside in [
        "0.5 setgray",
        "0 0 1 setrgbcolor",
        "0 0 1 sethsbcolor",
        "0 0 1 0 setcmykcolor",
        "/DeviceGray setcolorspace",
        "0 setcolor",
        "P setpattern",
        "1 1 8 [1 0 0 1 0 0] <ff> image",
        "1 1 8 [1 0 0 1 0 0] <ff> false 1 colorimage",
        "gsave 0.5 setgray grestore",
    ] {
        let program = format!(
            "{}3 setlinewidth /V << /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 10 10] \
             /XStep 10 /YStep 10 /PaintProc {{ pop {inside} 0 0 5 5 rectfill }} >> \
             matrix makepattern def 0.5 setgray 0.25 V setpattern 0 0 100 100 rectfill",
            defs()
        );
        let run = exec(&program);
        assert_eq!(run.error(), Some("undefined"), "{inside}");
        let calls = run.calls();
        let n = calls.len();
        assert_eq!(calls[n - 2], Call::EndPatternCell, "{inside}");
        assert_eq!(calls[n - 1], Call::GRestoreTo(0), "{inside}");
        assert!(!calls.contains(&cell(0.0, 0.0, 100.0, 100.0)), "{inside}");
    }
    // A coloured cell sets what it likes, and an image mask is fine in
    // either.
    let run = with_defs(
        "/C << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /PaintProc { pop 0.5 setgray 1 1 true [1 0 0 1 0 0] <00> imagemask } >> \
         matrix makepattern def C setpattern 0 0 100 100 rectfill \
         /M << /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 10 10] /XStep 10 /YStep 10 \
         /PaintProc { pop 1 1 true [1 0 0 1 0 0] <00> imagemask } >> \
         matrix makepattern def 0 M setpattern 0 0 100 100 rectfill",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    // The rule ends with the cell.
    let run = with_defs("0.25 U setpattern 0 0 100 100 rectfill 0.5 setgray currentgray =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0.5\n");
}

#[test]
fn a_raising_paint_procedure_restores_the_state_and_leaves_the_operands() {
    let run = with_defs(
        "3 setlinewidth /B << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
         /XStep 10 /YStep 10 /PaintProc { pop 9 setlinewidth 1 0 div } >> matrix makepattern def \
         B setpattern { 0 0 100 100 rectfill } stopped pop \
         $error /errorname get == currentlinewidth = count =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "/undefinedresult\n3.0\n6\n");
    let calls = run.calls();
    let end = calls
        .iter()
        .position(|c| matches!(c, Call::EndPatternCell))
        .unwrap();
    assert_eq!(calls[end + 1], Call::GRestoreTo(0));
    assert!(!calls.contains(&cell(0.0, 0.0, 100.0, 100.0)));
    assert!(run.interp.estack().is_empty());
    // `stop` and `exit` inside the procedure close the capture too.
    for inside in ["stop", "exit"] {
        let run = with_defs(&format!(
            "/B << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
             /XStep 10 /YStep 10 /PaintProc {{ pop {inside} }} >> matrix makepattern def \
             B setpattern {{ 0 0 100 100 rectfill }} stopped pop 0.5 setgray"
        ));
        assert_eq!(run.outcome, Outcome::Ok, "{inside}");
        let calls = run.calls();
        assert!(calls.contains(&Call::EndPatternCell), "{inside}");
        assert!(calls.contains(&Call::GRestoreTo(0)), "{inside}");
    }
}

#[test]
fn a_pattern_is_captured_on_each_page() {
    let run =
        with_defs("P setpattern 0 0 10 10 rectfill showpage 0 0 10 10 rectfill 0 0 10 10 rectfill");
    assert_eq!(run.outcome, Outcome::Ok);
    let ends = run
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::EndPatternCell))
        .count();
    assert_eq!(ends, 2);
}

#[test]
fn page_operators_are_undefined_inside_a_cell() {
    for inside in [
        "showpage",
        "copypage",
        "erasepage",
        "<< /PageSize [100 100] >> setpagedevice",
    ] {
        let run = with_defs(&format!(
            "/B << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
             /XStep 10 /YStep 10 /PaintProc {{ pop {inside} }} >> matrix makepattern def \
             B setpattern 0 0 100 100 rectfill"
        ));
        assert_eq!(run.error(), Some("undefined"), "{inside}");
        assert!(!run.interp.in_paint_procedure());
    }
}

#[test]
fn the_pattern_colour_follows_the_graphics_state() {
    let run = with_defs(
        "P setpattern gsave 0.5 setgray grestore currentcolor P eq = \
         save 0.25 U setpattern restore currentcolor P eq = \
         P setpattern save 0 0 10 10 rectfill restore 0 0 10 10 rectfill",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\ntrue\n");
}

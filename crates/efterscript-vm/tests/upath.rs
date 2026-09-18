// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! User paths at the graphics boundary: which backend calls the
//! user-path operators make, the structure and box checks of PLRM3
//! §4.6.1 with their errors, the encoded form of §4.6.2, and the state
//! left behind by a failed paint. The dumps and printed results are
//! corpus files under `corpus/unit/upath`; the recording backend keeps
//! no path, so `upath` is checked there.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_vm::{Bounds, Config, Interp, Io, Matrix, Outcome, Point, SliceSource};

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

/// The calls after the media box every run starts with.
fn calls(run: &Run) -> Vec<Call> {
    run.calls().into_iter().skip(1).collect()
}

fn p(x: f32, y: f32) -> Point {
    Point::new(x, y)
}

// --- the painting operators --------------------------------------------------------

#[test]
fn ufill_is_gsave_newpath_uappend_fill_grestore() {
    let run = exec("{0 0 100 100 setbbox 10 10 moveto 90 90 lineto closepath} ufill count =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    assert_eq!(
        calls(&run),
        [
            Call::GSave,
            Call::NewPath,
            Call::MoveTo(p(10.0, 10.0)),
            Call::LineTo(p(90.0, 90.0)),
            Call::ClosePath,
            Call::Fill,
            Call::GRestoreTo(0),
        ]
    );
}

#[test]
fn ueofill_and_ustroke_paint_their_way() {
    let run = exec(
        "/u {0 0 100 100 setbbox 10 10 moveto 90 90 lineto} cvlit def \
         u ueofill u ustroke u [2 0 0 2 0 0] ustroke count =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    let paints: Vec<Call> = calls(&run)
        .into_iter()
        .filter(|c| {
            !matches!(
                c,
                Call::GSave | Call::NewPath | Call::MoveTo(_) | Call::LineTo(_)
            )
        })
        .collect();
    assert_eq!(
        paints,
        [
            Call::EoFill,
            Call::GRestoreTo(0),
            Call::Stroke,
            Call::GRestoreTo(0),
            Call::Concat(Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0])),
            Call::Stroke,
            Call::GRestoreTo(0),
        ]
    );
}

/// A matrix is an array of exactly six numbers; anything else on top is
/// the user path.
#[test]
fn ustroke_tells_a_matrix_from_a_path_by_shape() {
    for (program, error) in [
        (
            "{0 0 9 9 setbbox 1 1 moveto} [2 0 0 2 0] ustroke",
            "typecheck",
        ),
        (
            "{0 0 9 9 setbbox 1 1 moveto} [2 0 0 2 0 (a)] ustroke",
            "typecheck",
        ),
        ("{0 0 9 9 setbbox 1 1 moveto} 2 ustroke", "typecheck"),
        ("[1 0 0 1 0 0] ustroke", "stackunderflow"),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
}

/// A paint that fails restores the graphics state it saved and leaves
/// the operands where they were.
#[test]
fn a_failed_paint_restores_the_state_and_keeps_its_operands() {
    let mut run = exec(
        "2 setlinewidth gsave 5 setlinewidth \
         {0 0 10 10 setbbox 5 5 moveto 20 20 lineto} [1 0 0 1 0 0] ustroke",
    );
    assert_eq!(run.error(), Some("rangecheck"));
    assert_eq!(run.interp.ostack().len(), 2);
    assert!(calls(&run).ends_with(&[Call::MoveTo(p(5.0, 5.0)), Call::GRestoreTo(1)]));
    assert_eq!(run.interp.graphics_backend().unwrap().line_width(), 5.0);
    assert_eq!(run.interp.graphics_backend().unwrap().gstate_depth(), 1);
}

#[test]
fn uappend_adds_to_the_current_path_without_saving() {
    let run = exec(
        "10 10 moveto {0 0 100 100 setbbox 50 50 moveto 60 60 lineto} uappend 70 70 lineto fill",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        calls(&run),
        [
            Call::MoveTo(p(10.0, 10.0)),
            Call::MoveTo(p(50.0, 50.0)),
            Call::LineTo(p(60.0, 60.0)),
            Call::LineTo(p(70.0, 70.0)),
            Call::Fill,
        ]
    );
}

// --- structure and box checks -------------------------------------------------------------

#[test]
fn every_allowed_operator_walks_through_the_backend() {
    let run = exec(
        "{ucache 0 0 200 200 setbbox 10 10 moveto 5 5 rmoveto 20 20 lineto 5 5 rlineto \
          30 30 40 40 50 50 curveto 1 1 2 2 3 3 rcurveto 100 100 20 0 90 arc \
          100 100 20 90 0 arcn 150 150 160 160 5 arct closepath} uappend",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        calls(&run),
        [
            Call::MoveTo(p(10.0, 10.0)),
            Call::MoveTo(p(15.0, 15.0)),
            Call::LineTo(p(20.0, 20.0)),
            Call::LineTo(p(25.0, 25.0)),
            Call::CurveTo(p(30.0, 30.0), p(40.0, 40.0), p(50.0, 50.0)),
            Call::CurveTo(p(51.0, 51.0), p(52.0, 52.0), p(53.0, 53.0)),
            Call::Arc(p(100.0, 100.0), 20.0, 0.0, 90.0),
            Call::ArcN(p(100.0, 100.0), 20.0, 90.0, 0.0),
            Call::ArcTo(p(150.0, 150.0), p(160.0, 160.0), 5.0),
            Call::ClosePath,
        ]
    );
}

/// Operator objects, as `bind` leaves them, name the operators as well
/// as executable names do.
#[test]
fn bound_user_paths_are_accepted() {
    let run = exec("{0 0 100 100 setbbox 10 10 moveto 90 90 lineto closepath} bind ufill");
    assert_eq!(run.outcome, Outcome::Ok);
    assert!(calls(&run).contains(&Call::LineTo(p(90.0, 90.0))));
}

#[test]
fn malformed_user_paths_are_typecheck() {
    for program in [
        "5 ufill",
        "(abc) ufill",
        "[1 2 3] ufill",
        "[] ufill",
        "{ucache} ufill",
        "{0 0 10 10 setbbox 5 moveto} ufill",
        "{0 0 10 10 setbbox 5 5 5 moveto} ufill",
        "{0 0 10 10 setbbox 5 5 moveto 6 6} ufill",
        "{5 5 moveto 0 0 10 10 setbbox} ufill",
        "{0 0 10 10 setbbox 5 5 moveto /lineto} ufill",
        "{0 0 10 10 setbbox 5 5 moveto 6 6 7 7 2 arcto} ufill",
        "{0 0 10 10 setbbox 5 5 moveto (x) 6 lineto} ufill",
        "{0 0 10 10 setbbox 5 5 moveto 6 6 lineto ucache} ufill",
        "{ucache ucache 0 0 10 10 setbbox 5 5 moveto} ufill",
        "{0 0 10 10 setbbox 5 5 moveto 0 0 10 10 setbbox} ufill",
        "{0 0 10 10 setbbox 5 5 lineto} ufill",
        "{0 0 10 10 setbbox 1 1 rmoveto} ufill",
        "{0 0 10 10 setbbox 5 5 6 6 7 7 2 arct} ufill",
        "{0 0 10 10 setbbox closepath} ufill",
        "{0 0 10 10 setbbox 5 5 moveto 6 6 lineto} 5 uappend",
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some("typecheck"), "{program}");
    }
}

#[test]
fn coordinates_are_checked_against_the_box() {
    for (program, error) in [
        (
            "{0 0 10 10 setbbox 5 5 moveto 20 20 lineto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto 6 6 rlineto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto -6 5 rmoveto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto 6 6 7 7 8 11 curveto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto 6 6 12 7 8 8 curveto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto 1 1 6 6 2 2 rcurveto} ufill",
            Some("rangecheck"),
        ),
        (
            "{0 0 10 10 setbbox 5 5 moveto 6 6 12 6 1 arct} ufill",
            Some("rangecheck"),
        ),
        ("{10 10 0 0 setbbox 5 5 moveto} ufill", Some("rangecheck")),
        // The figure of an arc: this one reaches x = 11.
        (
            "{0 0 10 10 setbbox 5 5 6 0 90 arc} ufill",
            Some("rangecheck"),
        ),
        // This one sweeps clockwise the long way round through x = 0.
        (
            "{1 0 10 10 setbbox 5 5 5 45 135 arcn} ufill",
            Some("rangecheck"),
        ),
        // Its ends and the top are inside, whatever its curves' control
        // points do.
        ("{0 0 10 10 setbbox 5 5 5 45 135 arc} ufill", None),
        ("{0 0 10 10 setbbox 5 5 5 135 45 arcn} ufill", None),
        // On the edge is inside.
        (
            "{0 0 10 10 setbbox 0 0 moveto 10 10 lineto 10 0 lineto} ufill",
            None,
        ),
        ("{5 5 5 5 setbbox 5 5 moveto} ufill", None),
        ("{0 0 10 10 setbbox} ufill", None),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), error, "{program}");
    }
}

/// The box is checked in default user space, enclosing the transformed
/// corners, so a rotated box admits what its device-aligned envelope
/// does.
#[test]
fn the_box_is_taken_through_the_ctm() {
    let run = exec("45 rotate {0 0 10 10 setbbox 5 5 moveto 10 10 lineto} ufill");
    assert_eq!(run.outcome, Outcome::Ok);
    // The point (12, -2) lies outside the user box but inside its
    // rotated envelope, whose x extent runs to 10·√2 ≈ 14.1 at y = 0.
    let run = exec("45 rotate {0 0 10 10 setbbox 5 5 moveto 7.5 -2.5 lineto} ufill");
    assert_eq!(run.outcome, Outcome::Ok);
    let run = exec("45 rotate {0 0 10 10 setbbox 5 5 moveto 15 -5 lineto} ufill");
    assert_eq!(run.error(), Some("rangecheck"));
    // Under a scale the same numbers mean the same thing.
    let run = exec("3 3 scale {0 0 10 10 setbbox 5 5 moveto 10 10 lineto 11 11 lineto} ufill");
    assert_eq!(run.error(), Some("rangecheck"));
}

// --- the encoded form -------------------------------------------------------------------

#[test]
fn encoded_paths_drive_the_same_calls() {
    let literal = exec(
        "{0 0 100 100 setbbox 10 10 moveto 90 10 lineto 90 90 lineto 10 90 lineto closepath} ufill",
    );
    // Sixteen-bit integers, then the same with a repeat count, then the
    // numbers as an array, then 32-bit fixed point with 8 fraction bits
    // low-order byte first.
    for program in [
        "[<9520000c 0000 0000 0064 0064 000a 000a 005a 000a 005a 005a 000a 005a> <00 01 03 03 03 0a>] ufill",
        "[<9520000c 0000 0000 0064 0064 000a 000a 005a 000a 005a 005a 000a 005a> <00 01 23 03 0a>] ufill",
        "[[0 0 100 100 10 10 90 10 90 90 10 90] <00 01 03 03 03 0a>] ufill",
        "[<95880c00 00000000 00000000 00640000 00640000 000a0000 000a0000 005a0000 000a0000 \
           005a0000 005a0000 000a0000 005a0000> <00 01 03 03 03 0a>] ufill",
    ] {
        let run = exec(program);
        assert_eq!(run.outcome, Outcome::Ok, "{program}");
        assert_eq!(run.calls(), literal.calls(), "{program}");
    }
}

#[test]
fn encoded_path_errors() {
    for (program, error) in [
        // An operator code outside the table.
        ("[[0 0 10 10 5 5] <00 01 0c>] ufill", "typecheck"),
        ("[[0 0 10 10 5 5] <00 01 20>] ufill", "typecheck"),
        // The data runs out.
        (
            "[<95200004 0000 0000 000a 000a> <00 01>] ufill",
            "typecheck",
        ),
        ("[[0 0 10 10 5] <00 01>] ufill", "typecheck"),
        // The elements are not a number sequence and an operator string.
        ("[<95200004 0000 0000 000a 000a> 5] ufill", "typecheck"),
        ("[5 <00>] ufill", "typecheck"),
        ("[[0 0 10 (x)] <00>] ufill", "typecheck"),
        ("[<> <00>] ufill", "typecheck"),
        // The number string is malformed.
        ("[<94200004 0000 0000 000a 000a> <00>] ufill", "typecheck"),
        ("[<95400004 0000 0000 000a 000a> <00>] ufill", "rangecheck"),
        ("[<95200004 0000 0000 000a> <00>] ufill", "rangecheck"),
        (
            "[<95200004 0000 0000 000a 000a 0001> <00>] ufill",
            "rangecheck",
        ),
        // The structure rules apply to the encoded form too.
        ("[[0 0 10 10 5 5] <01 00>] ufill", "typecheck"),
        ("[[0 0 10 10 5 5] <00 0b>] ufill", "typecheck"),
        ("[[0 0 10 10] <>] ufill", "typecheck"),
        ("[[0 0 10 10 5 5 20 20] <00 01 03>] ufill", "rangecheck"),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
}

/// A repeat count with nothing after it, and numbers the operators
/// never consume, are ignored, as other interpreters do.
#[test]
fn encoded_leftovers_are_ignored() {
    for program in [
        "[[0 0 10 10 5 5 6 6] <0b 00 01 03 ff>] ufill",
        "[[0 0 10 10 5 5 6 6 7 7 8 8] <00 01 03>] ufill",
        "[[0 0 10 10 5 5 6 6] <00 01 21 21 03 0a 21>] ufill",
    ] {
        let run = exec(program);
        assert_eq!(run.outcome, Outcome::Ok, "{program}");
        assert!(
            calls(&run).contains(&Call::LineTo(p(6.0, 6.0))),
            "{program}"
        );
    }
}

// --- the other operators ----------------------------------------------------------------

#[test]
fn arct_is_arcto_without_results() {
    let run = exec("0 0 moveto 10 10 10 0 2 arct count =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    assert_eq!(
        calls(&run),
        [
            Call::MoveTo(p(0.0, 0.0)),
            Call::ArcTo(p(10.0, 10.0), p(10.0, 0.0), 2.0),
        ]
    );
    assert_eq!(exec("10 10 10 0 2 arct").error(), Some("nocurrentpoint"));
    assert_eq!(
        exec("0 0 moveto 10 10 10 0 (r) arct").error(),
        Some("typecheck")
    );
}

#[test]
fn setbbox_outside_a_user_path_is_recorded_only() {
    let run = exec("0 0 moveto 1 2 30 40 setbbox 100 100 lineto count =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    assert_eq!(
        run.interp.declared_path_bbox(),
        Some(Bounds::new(1.0, 2.0, 30.0, 40.0))
    );
    assert!(calls(&run).contains(&Call::LineTo(p(100.0, 100.0))));
    let run = exec("1 2 30 40 setbbox newpath");
    assert_eq!(run.interp.declared_path_bbox(), None);
    assert_eq!(exec("10 10 0 0 setbbox").error(), Some("rangecheck"));
    assert_eq!(exec("0 0 (a) 10 setbbox").error(), Some("typecheck"));
    assert_eq!(exec("0 0 10 setbbox").error(), Some("stackunderflow"));
}

#[test]
fn cache_operators_report_nothing_cached() {
    let run = exec(
        "ucache ucachestatus counttomark == cleartomark \
         mark 1000 setucacheparams count == \
         mark 1 2 3 setucacheparams count == \
         mark setucacheparams count ==",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "5\n0\n0\n0\n");
    assert_eq!(calls(&run), []);
    assert_eq!(exec("1 2 setucacheparams").error(), Some("unmatchedmark"));
    let run = exec("ucachestatus pop pop pop pop pop ==");
    assert_eq!(run.output, "-mark-\n");
}

/// Without a backend the user-path operators are not defined, like the
/// rest of the graphics group.
#[test]
fn user_path_operators_belong_to_the_graphics_group() {
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(b"{0 0 1 1 setbbox} ufill"));
    assert!(matches!(&outcome, Outcome::Error(e) if e.name == "undefined" && e.command == "ufill"));
    for name in [
        "ufill",
        "upath",
        "setbbox",
        "ucache",
        "ucachestatus",
        "arct",
    ] {
        assert!(interp.operator(name).is_none(), "{name}");
    }
}

// --- ustrokepath ---------------------------------------------------------------------

/// The first form is `newpath uappend strokepath`; the second
/// concatenates its matrix before the outline and puts the CTM back,
/// so `ustrokepath` never changes it (PLRM3 §8.2). Nothing is saved:
/// the outline is meant to stay as the current path.
#[test]
fn ustrokepath_outlines_in_place_and_leaves_the_ctm_alone() {
    let run = exec(
        "/u {0 0 100 100 setbbox 10 10 moveto 90 90 lineto} cvlit def \
         u ustrokepath u [2 0 0 2 0 0] ustrokepath \
         matrix currentmatrix == count =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "[1.0 0.0 0.0 1.0 0.0 0.0]\n0\n");
    assert_eq!(
        calls(&run),
        [
            Call::NewPath,
            Call::MoveTo(p(10.0, 10.0)),
            Call::LineTo(p(90.0, 90.0)),
            Call::StrokeOutline,
            Call::NewPath,
            Call::MoveTo(p(10.0, 10.0)),
            Call::LineTo(p(90.0, 90.0)),
            Call::Concat(Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0])),
            Call::StrokeOutline,
            Call::SetMatrix(Matrix::IDENTITY),
        ]
    );
    let run = exec("5 5 moveto strokepath count =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    assert!(calls(&run).ends_with(&[Call::MoveTo(p(5.0, 5.0)), Call::StrokeOutline]));
}

#[test]
fn ustrokepath_checks_its_operands_like_ustroke() {
    for (program, error) in [
        (
            "{0 0 9 9 setbbox 1 1 moveto} [2 0 0 2 0] ustrokepath",
            "typecheck",
        ),
        (
            "{1 1 moveto 2 2 lineto 3 3 lineto 4 4 lineto} ustrokepath",
            "typecheck",
        ),
        ("[1 0 0 1 0 0] ustrokepath", "stackunderflow"),
        (
            "{0 0 9 9 setbbox 1 1 moveto 50 50 lineto} ustrokepath",
            "rangecheck",
        ),
    ] {
        let run = exec(program);
        assert_eq!(run.error(), Some(error), "{program}");
    }
    // The operands stay when the walk fails; the CTM was never touched.
    let run = exec("{0 0 9 9 setbbox 1 1 moveto 50 50 lineto} [2 0 0 2 0 0] ustrokepath");
    assert_eq!(run.error(), Some("rangecheck"));
    assert_eq!(run.interp.ostack().len(), 2);
    assert!(!calls(&run).contains(&Call::Concat(Matrix([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]))));
}

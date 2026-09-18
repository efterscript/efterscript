// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Forms at the boundary (PLRM3 §4.7): `execform`'s validation and
//! alterations, the capture of a body once per page and its placement
//! every time, nesting with pattern cells, and what an error inside the
//! body leaves behind.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_vm::{Bounds, Config, FormInfo, Interp, Io, Matrix, Outcome, Rect, SliceSource};

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

const FORM: &str = "<< /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 100 100] \
     /PaintProc { pop 0 0 5 5 rectfill } >>";

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

#[test]
fn execform_captures_once_per_page_and_places_every_time() {
    let run = exec(&format!(
        "/F {FORM} def 2 2 scale F execform F execform showpage F execform count ="
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "0\n");
    let calls = run.calls();
    let at = calls
        .iter()
        .position(|c| matches!(c, Call::BeginForm(_)))
        .unwrap();
    let Call::BeginForm(info) = calls[at] else {
        unreachable!()
    };
    assert!(matrix_approx(
        info.matrix,
        Matrix([2.0, 0.0, 0.0, 2.0, 200.0, 200.0])
    ));
    assert_eq!(info.bbox, Bounds::new(0.0, 0.0, 10.0, 10.0));
    assert_eq!(
        &calls[at..],
        [
            Call::BeginForm(info),
            Call::GSave,
            cell(0.0, 0.0, 5.0, 5.0),
            Call::EndForm,
            Call::GRestoreTo(0),
            Call::PlaceForm(info),
            Call::BeginForm(info),
            Call::PlaceForm(info),
            Call::ShowPage,
            Call::BeginForm(info),
            Call::GSave,
            cell(0.0, 0.0, 5.0, 5.0),
            Call::EndForm,
            Call::GRestoreTo(0),
            Call::PlaceForm(info),
        ]
    );
    assert!(run.interp.estack().is_empty());
}

#[test]
fn a_form_is_its_dictionary() {
    // Two dictionaries with equal entries are two forms; the same
    // dictionary through two names is one.
    let run = exec(&format!(
        "/F {FORM} def /G {FORM} def /H F def F execform G execform H execform"
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    let ids: Vec<u64> = run
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::PlaceForm(info) => Some(info.id),
            _ => None,
        })
        .collect();
    assert_eq!(ids.len(), 3);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(ids[0], ids[2]);
}

#[test]
fn execform_makes_the_dictionary_read_only_with_an_implementation_entry() {
    let run = exec(&format!(
        "/F {FORM} def F wcheck = F execform F wcheck = F /Implementation known = \
         F execform /G {FORM} readonly def G execform G /Implementation known ="
    ));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "true\nfalse\ntrue\ntrue\n");
}

#[test]
fn execform_errors() {
    let entries = [
        "/FormType 1",
        "/BBox [0 0 10 10]",
        "/Matrix [1 0 0 1 0 0]",
        "/PaintProc { pop }",
    ];
    for missing in 0..entries.len() {
        let dict: Vec<&str> = entries
            .iter()
            .enumerate()
            .filter(|(k, _)| *k != missing)
            .map(|(_, e)| *e)
            .collect();
        let run = exec(&format!("<< {} >> execform", dict.join(" ")));
        assert_eq!(
            run.error(),
            Some("undefined"),
            "without {}",
            entries[missing]
        );
        assert_eq!(run.interp.ostack().len(), 1, "the operand stays");
    }
    for (dict, error) in [
        (
            "/FormType 2 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] /PaintProc { pop }",
            "rangecheck",
        ),
        (
            "/FormType 1 /BBox [0 0 0 10] /Matrix [1 0 0 1 0 0] /PaintProc { pop }",
            "rangecheck",
        ),
        (
            "/FormType 1 /BBox [0 0 10] /Matrix [1 0 0 1 0 0] /PaintProc { pop }",
            "typecheck",
        ),
        (
            "/FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0] /PaintProc { pop }",
            "typecheck",
        ),
        (
            "/FormType 1 /BBox [0 0 10 10] /Matrix (a) /PaintProc { pop }",
            "typecheck",
        ),
        (
            "/FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] /PaintProc [1]",
            "typecheck",
        ),
        (
            "/FormType (1) /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] /PaintProc { pop }",
            "typecheck",
        ),
    ] {
        let run = exec(&format!("<< {dict} >> execform"));
        assert_eq!(run.error(), Some(error), "{dict}");
    }
    assert_eq!(exec("5 execform").error(), Some("typecheck"));
    assert_eq!(exec("execform").error(), Some("stackunderflow"));
    // Without a backend the name is not defined.
    let (io, _, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let outcome = interp.run(&mut SliceSource::new(format!("{FORM} execform").as_bytes()));
    assert!(matches!(outcome, Outcome::Error(e) if e.name == "undefined"));
}

#[test]
fn nested_forms_capture_through_the_stack() {
    let run = exec(
        "/Inner << /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop 0 0 10 10 rectfill } >> def \
         /Outer << /FormType 1 /BBox [0 0 100 100] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop 20 30 translate Inner execform } >> def \
         200 200 translate Outer execform Inner execform",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let calls: Vec<Call> = run
        .calls()
        .into_iter()
        .filter(|c| {
            matches!(
                c,
                Call::BeginForm(_)
                    | Call::EndForm
                    | Call::PlaceForm(_)
                    | Call::GRestoreTo(_)
                    | Call::RectFill(_)
            )
        })
        .collect();
    let Call::BeginForm(outer) = calls[0] else {
        panic!("{calls:?}");
    };
    let Call::BeginForm(inner) = calls[1] else {
        panic!("{calls:?}");
    };
    assert!(matrix_approx(
        outer.matrix,
        Matrix([1.0, 0.0, 0.0, 1.0, 200.0, 200.0])
    ));
    assert!(matrix_approx(
        inner.matrix,
        Matrix([1.0, 0.0, 0.0, 1.0, 220.0, 230.0])
    ));
    assert_eq!(
        calls[2..],
        [
            cell(0.0, 0.0, 10.0, 10.0),
            Call::EndForm,
            Call::GRestoreTo(1),
            Call::PlaceForm(inner),
            Call::EndForm,
            Call::GRestoreTo(0),
            Call::PlaceForm(outer),
            // The inner form is held by the page now: placed under the
            // page's CTM, not run.
            Call::BeginForm(FormInfo {
                matrix: outer.matrix,
                ..inner
            }),
            Call::PlaceForm(FormInfo {
                matrix: outer.matrix,
                ..inner
            }),
        ]
    );
}

#[test]
fn forms_and_cells_nest_either_way() {
    let program = "/P << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 10 10] \
         /XStep 10 /YStep 10 /PaintProc { pop 0 0 5 5 rectfill } >> matrix makepattern def \
         /F << /FormType 1 /BBox [0 0 100 100] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop P setpattern 0 0 100 100 rectfill } >> def \
         /Q << /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 100 100] \
         /XStep 100 /YStep 100 /PaintProc { pop F execform } >> matrix makepattern def \
         F execform Q setpattern 0 0 300 300 rectfill";
    let run = exec(program);
    assert_eq!(run.outcome, Outcome::Ok);
    let kinds: Vec<&str> = run
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::BeginForm(_) => Some("begin-form"),
            Call::EndForm => Some("end-form"),
            Call::PlaceForm(_) => Some("place-form"),
            Call::BeginPatternCell(_) => Some("begin-cell"),
            Call::EndPatternCell => Some("end-cell"),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            // The form's first execution: its body sets P and fills, which
            // captures P's cell inside the form.
            "begin-form",
            "begin-cell",
            "end-cell",
            "begin-cell",
            "end-form",
            "place-form",
            // Q's cell executes F, which the page already holds.
            "begin-cell",
            "begin-form",
            "place-form",
            "end-cell",
            "begin-cell",
        ]
    );
    assert!(run.interp.estack().is_empty());
}

#[test]
fn a_raising_body_restores_the_state_and_places_nothing() {
    let run = exec(
        "3 setlinewidth { << /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop 9 setlinewidth 1 0 div } >> execform } stopped pop \
         $error /errorname get == currentlinewidth = count =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output, "/undefinedresult\n3.0\n2\n");
    let calls = run.calls();
    let end = calls
        .iter()
        .position(|c| matches!(c, Call::EndForm))
        .unwrap();
    assert_eq!(calls[end + 1], Call::GRestoreTo(0));
    assert!(!calls.iter().any(|c| matches!(c, Call::PlaceForm(_))));
    assert!(run.interp.estack().is_empty());
    assert!(!run.interp.in_paint_procedure());
}

#[test]
fn page_operators_are_undefined_inside_a_body() {
    for inside in ["showpage", "copypage", "erasepage"] {
        let run = exec(&format!(
            "<< /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] \
             /PaintProc {{ pop {inside} }} >> execform"
        ));
        assert_eq!(run.error(), Some("undefined"), "{inside}");
    }
    // Colour operators are fine in a form, and inside an uncoloured
    // cell a form's body is bound by the cell's rule.
    let run = exec(
        "<< /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop 0.5 setgray 0 0 5 5 rectfill } >> execform",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    let run = exec(
        "/F << /FormType 1 /BBox [0 0 10 10] /Matrix [1 0 0 1 0 0] \
         /PaintProc { pop 0.5 setgray 0 0 5 5 rectfill } >> def \
         0 << /PatternType 1 /PaintType 2 /TilingType 1 /BBox [0 0 10 10] \
         /XStep 10 /YStep 10 /PaintProc { pop F execform } >> matrix makepattern setpattern \
         0 0 100 100 rectfill",
    );
    assert_eq!(run.error(), Some("undefined"));
}

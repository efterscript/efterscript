// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `pdfmark` operator at the boundary: what reaches the backend as
//! a kind and values, the errors for malformed marks, and the guarded
//! idiom with and without a backend.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_vm::{Config, Interp, Io, MarkValue, Outcome, SliceSource};

struct Run {
    outcome: Outcome,
    output: String,
    log: Log,
    stack_depth: usize,
}

impl Run {
    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }

    fn marks(&self) -> Vec<(Vec<u8>, Vec<MarkValue>)> {
        self.log
            .borrow()
            .iter()
            .filter_map(|call| match call {
                Call::PdfMark(kind, entries) => Some((kind.clone(), entries.clone())),
                _ => None,
            })
            .collect()
    }
}

fn run(program: &str, backend: bool) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    if backend {
        interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    }
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        outcome,
        output: out.text(),
        log,
        stack_depth: interp.ostack().len(),
    }
}

fn exec(program: &str) -> Run {
    run(program, true)
}

fn name(text: &str) -> MarkValue {
    MarkValue::Name(text.as_bytes().to_vec())
}

fn string(text: &str) -> MarkValue {
    MarkValue::String(text.as_bytes().to_vec())
}

#[test]
fn the_kind_is_the_last_name_and_the_rest_are_values_in_order() {
    let run = exec("[ /Title (Chapter) /Count 2 /OUT pdfmark");
    assert_eq!(run.error(), None);
    assert_eq!(
        run.marks(),
        [(
            b"OUT".to_vec(),
            vec![
                name("Title"),
                string("Chapter"),
                name("Count"),
                MarkValue::Int(2)
            ]
        )]
    );
    assert_eq!(run.stack_depth, 0, "everything down to the mark is popped");
}

#[test]
fn every_value_type_converts_and_nests() {
    let run = exec(
        "1 2 [ /R 1.5 /B true /N null /A [ 1 (s) [ /x ] ] /D << /k (v) /n << /m 2 >> >> \
         /P { 1 } cvlit /KIND pdfmark",
    );
    assert_eq!(run.error(), None);
    let (kind, values) = &run.marks()[0];
    assert_eq!(kind, b"KIND");
    assert_eq!(
        *values,
        vec![
            name("R"),
            MarkValue::Real(1.5),
            name("B"),
            MarkValue::Bool(true),
            name("N"),
            MarkValue::Null,
            name("A"),
            MarkValue::Array(vec![
                MarkValue::Int(1),
                string("s"),
                MarkValue::Array(vec![name("x")]),
            ]),
            name("D"),
            MarkValue::Dict(vec![
                (b"k".to_vec(), string("v")),
                (
                    b"n".to_vec(),
                    MarkValue::Dict(vec![(b"m".to_vec(), MarkValue::Int(2))])
                ),
            ]),
            name("P"),
            MarkValue::Array(vec![MarkValue::Int(1)]),
        ]
    );
    assert_eq!(run.stack_depth, 2, "objects below the mark stay");
}

#[test]
fn malformed_marks_raise_the_usual_errors() {
    assert_eq!(exec("/OUT pdfmark").error(), Some("unmatchedmark"));
    assert_eq!(
        exec("[ /Title (x) (OUT) pdfmark").error(),
        Some("typecheck")
    );
    assert_eq!(exec("[ pdfmark").error(), Some("typecheck"));
    assert_eq!(
        exec("[ /Proc { 1 add } /OUT pdfmark").error(),
        Some("typecheck"),
        "a procedure has no value form"
    );
    assert_eq!(
        exec("[ /Op /add load /OUT pdfmark").error(),
        Some("typecheck")
    );
    assert_eq!(
        exec("[ /D << 1 (one) >> /OUT pdfmark").error(),
        Some("typecheck"),
        "dictionary keys must be names"
    );
    let failed = exec("[ /Title (x) (OUT) pdfmark");
    assert_eq!(failed.marks(), []);
    assert_eq!(
        failed.stack_depth, 4,
        "a failing operator leaves its operands"
    );
}

#[test]
fn the_guarded_idiom_runs_both_ways() {
    let program = "/pdfmark where { pop } { userdict /pdfmark /cleartomark load put } ifelse \
                   [ /Title (Report) /DOCINFO pdfmark (done) =";
    let with = exec(program);
    assert_eq!(with.error(), None);
    assert_eq!(with.output, "done\n");
    assert_eq!(with.marks().len(), 1);
    assert_eq!(with.marks()[0].0, b"DOCINFO");
    let without = run(program, false);
    assert_eq!(without.error(), None);
    assert_eq!(without.output, "done\n");
    assert_eq!(without.stack_depth, 0);
}

#[test]
fn without_a_backend_the_name_is_undefined() {
    let bare = run("[ /Title (x) /OUT pdfmark", false);
    assert_eq!(bare.error(), Some("undefined"));
    match &bare.outcome {
        Outcome::Error(summary) => assert_eq!(summary.command, "pdfmark"),
        other => panic!("expected an error, got {other:?}"),
    }
}

#[test]
fn unknown_kinds_reach_the_backend_unchanged() {
    let run = exec("[ /Foo 1 /NOSUCH pdfmark");
    assert_eq!(run.error(), None);
    assert_eq!(run.marks()[0].0, b"NOSUCH");
}

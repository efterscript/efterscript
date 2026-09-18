// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `setdistillerparams` and `currentdistillerparams` at the boundary:
//! the tolerant dictionary, the type table, what reaches the backend,
//! and the operators' presence without one.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{Call, Log, Recording};
use efterscript_vm::{Config, Interp, Io, MarkValue, Outcome, SliceSource};

struct Run {
    outcome: Outcome,
    output: String,
    log: Log,
}

impl Run {
    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }

    fn params(&self) -> Vec<Vec<(Vec<u8>, MarkValue)>> {
        self.log
            .borrow()
            .iter()
            .filter_map(|call| match call {
                Call::DistillerParams(entries) => Some(entries.clone()),
                _ => None,
            })
            .collect()
    }
}

fn run_with(program: &str, backend: bool, seed: Option<&[(Vec<u8>, MarkValue)]>) -> Run {
    let (io, out, _) = Io::capture();
    let mut interp = Interp::with_config(Config {
        io,
        ..Default::default()
    });
    let log: Log = Rc::new(RefCell::new(Vec::new()));
    if backend {
        interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    }
    if let Some(entries) = seed {
        interp.set_distiller_params(entries).unwrap();
    }
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        outcome,
        output: out.text(),
        log,
    }
}

fn exec(program: &str) -> Run {
    run_with(program, true, None)
}

fn name(text: &str) -> MarkValue {
    MarkValue::Name(text.as_bytes().to_vec())
}

fn entry(key: &str, value: MarkValue) -> (Vec<u8>, MarkValue) {
    (key.as_bytes().to_vec(), value)
}

// params-round-trip.ps
#[test]
fn set_values_read_back_including_unknown_keys() {
    let run = exec(
        "<< /CompressPages false /Foo 1 >> setdistillerparams\n\
         currentdistillerparams /CompressPages get =\n\
         currentdistillerparams /Foo get =",
    );
    assert_eq!(run.error(), None);
    assert_eq!(run.output, "false\n1\n");
}

#[test]
fn the_defaults_are_readable_before_any_request() {
    let run = exec(
        "currentdistillerparams /CompressPages get =\n\
         currentdistillerparams /SubsetFonts get =\n\
         currentdistillerparams /CompatibilityLevel get =\n\
         currentdistillerparams /MonoImageResolution get =\n\
         currentdistillerparams /ColorConversionStrategy get =",
    );
    assert_eq!(run.output, "true\ntrue\n1.7\n300\nLeaveColorUnchanged\n");
    let defaults = efterscript_vm::default_distiller_params();
    assert_eq!(defaults.len(), 15);
    assert!(defaults.iter().all(|(key, _)| !key.is_empty()));
}

// params-typecheck.ps
#[test]
fn a_recognised_key_of_the_wrong_type_is_typecheck_and_changes_nothing() {
    for request in [
        "/CompressPages 3",
        "/EmbedAllFonts (yes)",
        "/CompatibilityLevel /High",
        "/ColorImageResolution true",
        "/ColorImageDownsampleType (Average)",
        "/AutoRotatePages 1",
    ] {
        let run = exec(&format!(
            "<< {request} /Foo 2 >> setdistillerparams\n\
             currentdistillerparams /Foo known ="
        ));
        assert_eq!(run.error(), Some("typecheck"), "{request}");
        assert!(run.params().is_empty(), "{request}");
    }
    // The request stays on the stack, as an operator that fails leaves it.
    let run = exec("<< /CompressPages 3 >> setdistillerparams");
    assert_eq!(run.error(), Some("typecheck"));
}

#[test]
fn numbers_take_either_form_and_unknown_keys_take_anything() {
    let run = exec(
        "<< /CompatibilityLevel 1.4 /ColorImageResolution 72 /GrayImageResolution 100.0\n\
            /Anything { 1 } /Other [ 1 2 ] >> setdistillerparams\n\
         currentdistillerparams /Anything get length =",
    );
    assert_eq!(run.error(), None);
    assert_eq!(run.output, "1\n");
    let sent = run.params();
    assert_eq!(sent.len(), 1);
    // The procedure has no value form and is left out; the array is
    // carried.
    assert_eq!(
        sent[0],
        vec![
            entry("CompatibilityLevel", MarkValue::Real(1.4)),
            entry("ColorImageResolution", MarkValue::Int(72)),
            entry("GrayImageResolution", MarkValue::Real(100.0)),
            entry(
                "Other",
                MarkValue::Array(vec![MarkValue::Int(1), MarkValue::Int(2)])
            ),
        ]
    );
}

#[test]
fn every_request_reaches_the_backend_in_order() {
    let run = exec(
        "<< /CompressPages false >> setdistillerparams\n\
         << /EmbedAllFonts true /ColorImageDownsampleType /Subsample >> setdistillerparams",
    );
    assert_eq!(run.error(), None);
    assert_eq!(
        run.params(),
        vec![
            vec![entry("CompressPages", MarkValue::Bool(false))],
            vec![
                entry("EmbedAllFonts", MarkValue::Bool(true)),
                entry("ColorImageDownsampleType", name("Subsample")),
            ],
        ]
    );
}

#[test]
fn each_current_call_yields_a_fresh_copy() {
    let run = exec(
        "currentdistillerparams currentdistillerparams eq =\n\
         currentdistillerparams dup /CompressPages false put setdistillerparams\n\
         currentdistillerparams /CompressPages get =",
    );
    assert_eq!(run.error(), None);
    assert_eq!(run.output, "false\nfalse\n");
}

#[test]
fn the_operators_work_without_a_backend() {
    let run = run_with(
        "<< /CompressPages false >> setdistillerparams\n\
         currentdistillerparams /CompressPages get =",
        false,
        None,
    );
    assert_eq!(run.error(), None);
    assert_eq!(run.output, "false\n");
}

#[test]
fn an_embedder_seeds_its_own_values_without_a_backend_call() {
    let seed = [
        entry("CompressPages", MarkValue::Bool(false)),
        entry("Custom", MarkValue::String(b"x".to_vec())),
    ];
    let run = run_with(
        "currentdistillerparams /CompressPages get =\n\
         currentdistillerparams /Custom get =\n\
         currentdistillerparams /SubsetFonts get =",
        true,
        Some(&seed),
    );
    assert_eq!(run.error(), None);
    assert_eq!(run.output, "false\nx\ntrue\n");
    assert!(run.params().is_empty());
}

#[test]
fn a_non_dictionary_operand_is_typecheck() {
    let run = exec("[ ] setdistillerparams");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("setdistillerparams");
    assert_eq!(run.error(), Some("stackunderflow"));
}

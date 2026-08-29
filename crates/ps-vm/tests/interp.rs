// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Interpreter scenarios, each naming the `corpus/unit/interp/*.ps` file it
//! mirrors where one exists, plus the property test that random control
//! nesting keeps the host recursion depth constant, and the stack-limit
//! tests.

use proptest::prelude::*;
use ps_vm::{
    Access, Capture, ChunkSource, Config, ErrorSummary, Interp, Io, Limits, Object, Outcome,
    SliceSource, Type, VmError,
};

// --- helpers ---------------------------------------------------------------

fn interp_with(limits: Limits) -> (Interp, Capture, Capture) {
    let (io, out, err) = Io::capture();
    let config = Config {
        limits,
        io,
        ..Default::default()
    };
    (Interp::with_config(config), out, err)
}

fn run_in(interp: &mut Interp, program: &str) -> Outcome {
    interp.run(&mut SliceSource::new(program.as_bytes()))
}

fn run(program: &str) -> (Interp, Outcome, Capture, Capture) {
    let (mut interp, out, err) = interp_with(Limits::default());
    let outcome = run_in(&mut interp, program);
    (interp, outcome, out, err)
}

fn ints(interp: &Interp) -> Vec<i32> {
    interp
        .ostack()
        .iter()
        .map(|o| o.as_i32().expect("integer"))
        .collect()
}

fn reals(interp: &Interp) -> Vec<f32> {
    interp
        .ostack()
        .iter()
        .map(|o| o.as_f32().expect("real"))
        .collect()
}

fn name_text(interp: &Interp, object: Object) -> String {
    let atom = object.as_name().expect("name");
    String::from_utf8_lossy(interp.memory().name_text(atom)).into_owned()
}

fn error_entry(interp: &Interp, key: &str) -> Object {
    let atom = interp
        .memory()
        .names()
        .lookup(key.as_bytes())
        .expect("known");
    interp
        .memory()
        .dict(interp.dicts().error)
        .unwrap()
        .get(Object::name(atom))
        .expect("entry")
}

fn error_name(outcome: &Outcome) -> Option<&str> {
    match outcome {
        Outcome::Error(ErrorSummary { name, .. }) => Some(name),
        _ => None,
    }
}

fn top(interp: &Interp) -> Object {
    *interp.ostack().last().expect("non-empty stack")
}

// --- execution without native recursion ----------------------------------------

// deep-recursion.ps
#[test]
fn deep_recursion_is_an_error_not_a_crash() {
    let (interp, outcome, _, err) = run("/f { f } def f");
    assert_eq!(error_name(&outcome), Some("execstackoverflow"));
    assert!(interp.estack().is_empty());
    assert_eq!(interp.exec_count(), 0);
    assert_eq!(
        err.text(),
        "%%[ Error: execstackoverflow; OffendingCommand: f ]%%\n"
    );

    let (_, outcome, _, _) = run("/g { /g load exec } def g");
    assert_eq!(error_name(&outcome), Some("execstackoverflow"));

    let (_, outcome, _, _) = run("/h { { h } exec } def h");
    assert_eq!(error_name(&outcome), Some("execstackoverflow"));
}

fn nested_loops(depth: usize) -> String {
    let mut body = String::from("exit");
    for level in 0..depth {
        body = format!("{{ {body} }} loop");
        if level + 1 < depth {
            body.push_str(" exit");
        }
    }
    body
}

// deep-loop-nesting.ps
#[test]
fn deep_loop_nesting_completes() {
    let program = format!("{} 7", nested_loops(200));
    let (interp, outcome, _, _) = run(&program);
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [7]);
    assert!(interp.estack().is_empty());
}

#[test]
fn execution_stack_limit_is_configurable_upward() {
    let (mut interp, _, _) = interp_with(Limits {
        exec: 5000,
        ..Default::default()
    });
    let outcome = run_in(&mut interp, &nested_loops(900));
    assert_eq!(outcome, Outcome::Ok);
    let (mut interp, _, _) = interp_with(Limits {
        exec: 100,
        ..Default::default()
    });
    let outcome = run_in(&mut interp, &nested_loops(150));
    assert_eq!(error_name(&outcome), Some("execstackoverflow"));
}

#[cfg(debug_assertions)]
#[test]
fn the_loop_is_never_reentered() {
    let (mut interp, _, _) = interp_with(Limits::default());
    run_in(
        &mut interp,
        "/f { 1 { 2 { 3 } stopped pop exit } loop } def 5 { f } repeat { 1 0 div } stopped pop",
    );
    assert_eq!(interp.max_host_depth(), 1);
}

// --- control operators ------------------------------------------------------------

// exit-nested-procedure.ps
#[test]
fn exit_from_a_nested_procedure() {
    let (interp, outcome, _, _) = run("0 1 1 10 { dup 5 eq { exit } if add } for");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [10, 5]);
}

// for-real-control.ps
#[test]
fn for_with_real_control() {
    let (interp, outcome, _, _) = run("0 0.5 2 { } for");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(reals(&interp), [0.0, 0.5, 1.0, 1.5, 2.0]);
    let (interp, _, _, _) = run("1 1 3.0 { } for");
    assert_eq!(reals(&interp), [1.0, 2.0, 3.0]);
}

#[test]
fn for_counts_down_and_stops_at_the_limit() {
    let (interp, _, _, _) = run("5 -2 0 { } for 3 1 1 { } for 1 1 1 { } for");
    assert_eq!(ints(&interp), [5, 3, 1, 1]);
    let (interp, _, _, _) = run("2147483640 5 2147483647 { } for");
    assert_eq!(ints(&interp), [2147483640, 2147483645]);
}

#[test]
fn conditionals_repeat_and_loop() {
    let (interp, _, _, _) =
        run("true { 1 } { 2 } ifelse false { 3 } if 2 { 4 } repeat { 5 exit } loop 0 { 6 } repeat");
    assert_eq!(ints(&interp), [1, 4, 4, 5]);
    let (_, outcome, _, _) = run("1 { 2 } if");
    assert_eq!(error_name(&outcome), Some("typecheck"));
    let (_, outcome, _, _) = run("-1 { } repeat");
    assert_eq!(error_name(&outcome), Some("rangecheck"));
}

#[test]
fn exec_runs_procedures_and_pushes_literals() {
    let (interp, _, _, _) = run("{ 1 2 add } exec 4 exec { { 9 } } exec { 8 }");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(3));
    assert_eq!(stack[1].as_i32(), Some(4));
    assert_eq!(stack[2].ty(), Type::Array);
    assert!(stack[2].is_executable());
    assert_eq!(stack[3].ty(), Type::Array);
    assert_eq!(stack.len(), 4);
}

#[test]
fn exit_outside_a_loop_and_across_stopped_is_invalidexit() {
    let (_, outcome, _, _) = run("exit");
    assert_eq!(error_name(&outcome), Some("invalidexit"));
    let (interp, outcome, _, _) = run("{ { exit } stopped exit } loop");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(interp.ostack().len(), 1);
    assert_eq!(top(&interp).as_bool(), Some(true));
    assert_eq!(
        name_text(&interp, error_entry(&interp, "errorname")),
        "invalidexit"
    );
}

#[test]
fn stop_and_stopped() {
    let (interp, outcome, _, _) = run("{ 1 2 add } stopped { stop 9 } stopped 7 stopped");
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(3));
    assert_eq!(stack[1].as_bool(), Some(false));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(7));
    assert_eq!(stack[4].as_bool(), Some(false));
    let (interp, outcome, _, err) = run("1 stop 2");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [1]);
    assert!(err.text().is_empty());
    let (interp, outcome, _, _) = run("{ { 1 0 div } stopped pop 5 } stopped");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(interp.ostack()[0].as_i32(), Some(1));
    assert_eq!(interp.ostack()[1].as_i32(), Some(0));
    assert_eq!(interp.ostack()[2].as_i32(), Some(5));
    assert_eq!(interp.ostack()[3].as_bool(), Some(false));
}

#[test]
fn execstack_reports_program_objects_only() {
    let (interp, _, _, _) =
        run("countexecstack { countexecstack { null null null null } execstack length } exec");
    assert_eq!(ints(&interp), [0, 1, 1]);
    let (interp, _, _, _) = run("{ { { countexecstack } stopped pop exit } loop } stopped pop");
    assert_eq!(ints(&interp), [3]);
    let (_, outcome, _, _) = run("{ { { } execstack } exec } exec");
    assert_eq!(error_name(&outcome), Some("rangecheck"));
}

#[test]
fn quit_ends_everything() {
    let (interp, outcome, _, _) = run("1 { 2 quit 3 } exec 4");
    assert_eq!(outcome, Outcome::Ok);
    assert!(interp.has_quit());
    assert_eq!(ints(&interp), [1, 2]);
    assert!(interp.estack().is_empty());
}

// --- error machinery ------------------------------------------------------------

// stopped-catches-error.ps
#[test]
fn stopped_catches_an_error() {
    let (interp, outcome, _, err) = run("{ 1 0 div } stopped");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(top(&interp).as_bool(), Some(true));
    assert_eq!(
        name_text(&interp, error_entry(&interp, "errorname")),
        "undefinedresult"
    );
    assert!(error_entry(&interp, "command").eq(interp.operator("div").unwrap()));
    assert_eq!(error_entry(&interp, "newerror").as_bool(), Some(true));
    assert!(err.text().is_empty());
    let snapshot = error_entry(&interp, "ostack");
    let items = interp.memory().array(snapshot).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].as_i32(), Some(1));
    assert_eq!(items[1].as_i32(), Some(0));
    let dicts = error_entry(&interp, "dstack");
    assert_eq!(interp.memory().array(dicts).unwrap().len(), 3);
    assert!(
        interp
            .memory()
            .array(error_entry(&interp, "estack"))
            .is_some()
    );
}

// program-defined-handler.ps
#[test]
fn program_defined_handler_runs() {
    let (interp, outcome, out, err) =
        run("errordict /undefined { pop (caught) print } put nosuchname 42");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "caught");
    assert!(err.text().is_empty());
    assert_eq!(ints(&interp), [42]);
    assert_eq!(error_entry(&interp, "newerror").as_bool(), Some(false));
}

// uncaught-error-report.ps
#[test]
fn uncaught_error_ends_the_job_with_a_report() {
    let (interp, outcome, out, err) = run("1 0 div 5");
    assert_eq!(
        outcome,
        Outcome::Error(ErrorSummary {
            name: "undefinedresult".into(),
            command: "div".into()
        })
    );
    assert_eq!(
        err.text(),
        "%%[ Error: undefinedresult; OffendingCommand: div ]%%\n"
    );
    assert!(out.text().is_empty());
    assert_eq!(ints(&interp), [1, 0]);
    assert_eq!(error_entry(&interp, "newerror").as_bool(), Some(false));
    assert!(interp.estack().is_empty());
}

#[test]
fn a_caught_error_is_not_reported_at_the_boundary() {
    let (_, outcome, _, err) = run("{ 1 0 div } stopped pop 1 2 add");
    assert_eq!(outcome, Outcome::Ok);
    assert!(err.text().is_empty());
    let (_, outcome, _, err) = run("{ 1 0 div } stopped pop stop");
    assert_eq!(error_name(&outcome), Some("undefinedresult"));
    assert!(err.text().contains("OffendingCommand: div"));
}

#[test]
fn handleerror_reports_once_and_can_be_redefined() {
    let (interp, _, _, err) = run("{ nosuchname } stopped pop handleerror handleerror");
    assert_eq!(
        err.text(),
        "%%[ Error: undefined; OffendingCommand: nosuchname ]%%\n"
    );
    assert_eq!(error_entry(&interp, "newerror").as_bool(), Some(false));
    let (_, outcome, out, err) = run("errordict /handleerror { (mine) print } put 1 0 div");
    assert_eq!(error_name(&outcome), Some("undefinedresult"));
    assert_eq!(out.text(), "mine");
    assert!(err.text().is_empty());
}

#[test]
fn every_error_has_an_errordict_entry() {
    let mut interp = Interp::new();
    for error in VmError::ALL {
        let key = interp.intern(error.name());
        let entry = interp
            .memory()
            .dict(interp.dicts().errordict)
            .unwrap()
            .get(key)
            .unwrap_or_else(|| panic!("{}", error.name()));
        assert_eq!(entry.ty(), Type::Operator);
    }
}

#[test]
fn recordstacks_false_skips_the_snapshots() {
    let (interp, _, _, _) = run("$error /recordstacks false put { 1 0 div } stopped");
    let key = Object::name(interp.memory().names().lookup(b"ostack").unwrap());
    assert!(
        interp
            .memory()
            .dict(interp.dicts().error)
            .unwrap()
            .get(key)
            .is_none()
    );
    assert_eq!(
        name_text(&interp, error_entry(&interp, "errorname")),
        "undefinedresult"
    );
}

#[test]
fn a_handler_that_keeps_failing_ends_the_job() {
    let (interp, outcome, _, _) = run("errordict /undefined { pop nosuchname } put nosuchname 1");
    assert_eq!(error_name(&outcome), Some("undefined"));
    assert!(interp.estack().is_empty());
}

#[test]
fn scanner_errors_enter_the_machinery() {
    let (interp, outcome, _, _) = run("1 ) 2");
    assert_eq!(error_name(&outcome), Some("syntaxerror"));
    assert_eq!(ints(&interp), [1]);
    let (interp, outcome, _, _) = run("errordict /syntaxerror { pop } put 1 ) 2");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [1, 2]);
    let (interp, _, _, _) = run("{ //nosuch } stopped");
    assert_eq!(
        name_text(&interp, error_entry(&interp, "errorname")),
        "undefined"
    );
}

#[test]
fn undefined_names_report_the_name() {
    let (interp, outcome, _, err) = run("frobnicate");
    assert_eq!(error_name(&outcome), Some("undefined"));
    assert_eq!(
        err.text(),
        "%%[ Error: undefined; OffendingCommand: frobnicate ]%%\n"
    );
    assert!(error_entry(&interp, "command").is_executable());
}

// --- name lookup and binding --------------------------------------------------------

// bind-operators-only.ps
#[test]
fn bind_replaces_operators_only() {
    let (interp, outcome, _, _) = run("/x 1 def /p { x add { x sub } } bind def /p load");
    assert_eq!(outcome, Outcome::Ok);
    let proc_ = top(&interp);
    let items = interp.memory().array(proc_).unwrap().to_vec();
    assert_eq!(items[0].ty(), Type::Name);
    assert_eq!(name_text(&interp, items[0]), "x");
    assert!(items[1].eq(interp.operator("add").unwrap()));
    let inner = interp.memory().array(items[2]).unwrap().to_vec();
    assert_eq!(inner[0].ty(), Type::Name);
    assert!(inner[1].eq(interp.operator("sub").unwrap()));
}

#[test]
fn bind_leaves_read_only_procedures_and_survives_cycles() {
    let mut interp = Interp::new();
    let add = interp.intern("add").as_executable();
    let inner = interp
        .memory_mut()
        .alloc_array(vec![add])
        .unwrap()
        .as_executable()
        .with_access(Access::ReadOnly)
        .unwrap();
    let outer = interp
        .memory_mut()
        .alloc_array(vec![inner, add, Object::null()])
        .unwrap()
        .as_executable();
    interp.memory_mut().array_put(outer, 2, outer).unwrap();
    interp.define("p", outer).unwrap();
    assert_eq!(run_in(&mut interp, "/p load bind pop"), Outcome::Ok);
    let items = interp.memory().array(outer).unwrap().to_vec();
    assert!(items[1].eq(interp.operator("add").unwrap()));
    assert_eq!(interp.memory().array(inner).unwrap()[0].ty(), Type::Name);
    assert_eq!(
        run_in(&mut interp, "/q { 1 } def /q load readonly"),
        Outcome::Error(ErrorSummary {
            name: "undefined".into(),
            command: "readonly".into()
        })
    );
}

#[test]
fn names_resolve_top_down_and_through_names() {
    let (interp, _, _, _) = run("/a 1 def 2 dict begin /a 2 def a end a");
    assert_eq!(ints(&interp), [2, 1]);
    let mut interp = Interp::new();
    let a = interp.intern("a");
    interp.define("a", Object::integer(5)).unwrap();
    interp.define("b", a.as_executable()).unwrap();
    interp.define("z", Object::null().as_executable()).unwrap();
    let text = interp
        .memory_mut()
        .alloc_string(b"1 2 add".to_vec())
        .as_executable();
    interp.define("s", text).unwrap();
    assert_eq!(run_in(&mut interp, "b z s"), Outcome::Ok);
    assert_eq!(ints(&interp), [5, 3]);
}

#[test]
fn systemdict_holds_constants_and_is_read_only() {
    let (interp, _, _, _) = run("languagelevel true false null systemdict /add known");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(2));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(stack[2].as_bool(), Some(false));
    assert_eq!(stack[3].ty(), Type::Null);
    assert_eq!(stack[4].as_bool(), Some(true));
    let (_, outcome, _, _) = run("systemdict /x 1 put");
    assert_eq!(error_name(&outcome), Some("invalidaccess"));
    let (interp, outcome, _, _) = run(
        "userdict /u 1 put u globaldict /g 2 put g errordict /typecheck known statusdict length $error /newerror get",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(1));
    assert_eq!(stack[1].as_i32(), Some(2));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_i32(), Some(0));
    assert_eq!(stack[4].as_bool(), Some(false));
}

// --- stack limits -------------------------------------------------------------------

// operand-stack-overflow.ps
#[test]
fn operand_stack_overflow() {
    let (interp, outcome, _, err) = run("{ 1 } loop");
    assert_eq!(error_name(&outcome), Some("stackoverflow"));
    assert_eq!(interp.ostack().len(), 500);
    assert!(err.text().contains("OffendingCommand: 1"));
    let (mut interp, _, _) = interp_with(Limits {
        operand: 1000,
        ..Default::default()
    });
    run_in(&mut interp, "{ 1 } loop");
    assert_eq!(interp.ostack().len(), 1000);
}

#[test]
fn dictionary_stack_limits() {
    let (interp, outcome, _, _) = run("{ 1 dict begin } loop");
    assert_eq!(error_name(&outcome), Some("dictstackoverflow"));
    assert_eq!(interp.dstack().len(), 20);
    let (interp, outcome, _, _) = run("end");
    assert_eq!(error_name(&outcome), Some("dictstackunderflow"));
    assert_eq!(interp.dstack().len(), 3);
    let (_, outcome, _, _) = run("1 dict begin end end");
    assert_eq!(error_name(&outcome), Some("dictstackunderflow"));
}

#[test]
fn popping_an_empty_stack_is_stackunderflow() {
    for program in [
        "pop",
        "1 add",
        "exch",
        "3 index",
        "1 2 3 5 1 roll",
        "2 copy",
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some("stackunderflow"), "{program}");
    }
}

// --- output -------------------------------------------------------------------------

#[test]
fn captured_output() {
    let (_, outcome, out, err) = run("2 3 add = (x) print 1.5 = /n = true = { } = (a\\n) print");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "5\nx1.5\nn\ntrue\n--nostringval--\na\n");
    assert!(err.text().is_empty());
    let mut silent = Interp::new();
    assert_eq!(run_in(&mut silent, "1 = (x) print"), Outcome::Ok);
    assert!(silent.stdout_file().is_none());
}

#[test]
fn a_suspended_run_resumes() {
    let (mut interp, _, _) = interp_with(Limits::default());
    let mut source = ChunkSource::new();
    source.append(b"1 2 (ab");
    assert_eq!(interp.run(&mut source), Outcome::Suspended);
    assert_eq!(ints(&interp), [1, 2]);
    source.append(b"c) 3");
    source.finish();
    assert_eq!(interp.resume(&mut source), Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack.len(), 4);
    assert_eq!(interp.memory().string(stack[2]), Some(&b"abc"[..]));
    assert_eq!(stack[3].as_i32(), Some(3));
    assert_eq!(run_in(&mut interp, "pop pop add"), Outcome::Ok);
    assert_eq!(ints(&interp), [3]);
}

// --- operators ---------------------------------------------------------------------

#[test]
fn stack_operators() {
    let (interp, _, _, _) = run("1 2 3 exch 3 -1 roll 2 copy 1 index count");
    assert_eq!(ints(&interp), [3, 2, 1, 2, 1, 2, 6]);
    let (interp, _, _, _) = run("1 2 3 4 4 1 roll 4 2 roll");
    assert_eq!(ints(&interp), [2, 3, 4, 1]);
    let (interp, _, _, _) = run("1 2 3 3 4 roll 1 2 3 3 -4 roll 1 2 0 5 roll");
    assert_eq!(ints(&interp), [3, 1, 2, 2, 3, 1, 1, 2]);
    let (interp, _, _, _) = run("mark 1 2 counttomark 3 cleartomark 5 dup pop clear 6 0 copy");
    assert_eq!(ints(&interp), [6]);
    for (program, error) in [
        ("cleartomark", "unmatchedmark"),
        ("counttomark", "unmatchedmark"),
        ("-1 index", "rangecheck"),
        ("1 2 -1 0 roll", "rangecheck"),
        ("-1 copy", "rangecheck"),
        ("1 (a) copy", "typecheck"),
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
    }
    let (interp, _, _, _) =
        run("{ 1 2 3 } { 0 0 0 0 } copy length 2 dict dup /a 1 put 3 dict copy length");
    assert_eq!(ints(&interp), [3, 1]);
}

#[test]
fn arithmetic_overflows_to_real() {
    let (interp, _, _, _) =
        run("2147483647 1 add -2147483648 1 sub 65536 65536 mul -2147483648 neg -2147483648 abs");
    let stack = interp.ostack();
    assert!(stack.iter().all(|o| o.ty() == Type::Real));
    assert_eq!(
        reals(&interp),
        [
            2147483648.0,
            -2147483649.0,
            4294967296.0,
            2147483648.0,
            2147483648.0
        ]
    );
    let (interp, _, _, _) =
        run("1 2 add 1.5 2 add 3 2 sub 3 2 mul 6 4 div 7 2 idiv -7 2 mod 3 neg -3 abs");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(3));
    assert_eq!(stack[1].as_f32(), Some(3.5));
    assert_eq!(stack[2].as_i32(), Some(1));
    assert_eq!(stack[3].as_i32(), Some(6));
    assert_eq!(stack[4].as_f32(), Some(1.5));
    assert_eq!(stack[5].as_i32(), Some(3));
    assert_eq!(stack[6].as_i32(), Some(-1));
    assert_eq!(stack[7].as_i32(), Some(-3));
    assert_eq!(stack[8].as_i32(), Some(3));
}

#[test]
fn math_operators() {
    let (interp, _, _, _) =
        run("16 sqrt 0 1 atan 1 0 atan -1 0 atan 0 cos 90 sin 2 3 exp 1 ln 100 log");
    assert_eq!(
        reals(&interp),
        [4.0, 0.0, 90.0, 270.0, 1.0, 1.0, 8.0, 0.0, 2.0]
    );
    let (interp, _, _, _) = run("2.5 round -2.5 round 2.5 ceiling 2.5 floor -2.5 truncate 3 round");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_f32(), Some(3.0));
    assert_eq!(stack[1].as_f32(), Some(-2.0));
    assert_eq!(stack[2].as_f32(), Some(3.0));
    assert_eq!(stack[3].as_f32(), Some(2.0));
    assert_eq!(stack[4].as_f32(), Some(-2.0));
    assert_eq!(stack[5].as_i32(), Some(3));
    for (program, error) in [
        ("1 0 div", "undefinedresult"),
        ("1 0 idiv", "undefinedresult"),
        ("1 0 mod", "undefinedresult"),
        ("-2147483648 -1 idiv", "undefinedresult"),
        ("0 0 atan", "undefinedresult"),
        ("-1 sqrt", "rangecheck"),
        ("0 ln", "rangecheck"),
        ("-1 log", "rangecheck"),
        ("-8 0.5 exp", "undefinedresult"),
        ("1e38 1e38 mul", "undefinedresult"),
        ("(a) 1 add", "typecheck"),
        ("1.5 2 idiv", "typecheck"),
    ] {
        let (interp, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
        let operands = program.split_whitespace().count() - 1;
        assert_eq!(
            interp.ostack().len(),
            operands,
            "{program} leaves its operands"
        );
    }
}

#[test]
fn relational_boolean_and_bitwise_operators() {
    let (interp, _, _, _) = run(
        "1 1.0 eq 1 2 ne (abc) (abc) eq (abc) /abc eq /abc (abd) ne 1 2 lt 2 1.5 ge (abc) (abd) lt (b) (a) gt 1 1 le 1 1 gt { 1 } dup eq { 1 } { 1 } eq",
    );
    let flags: Vec<bool> = interp
        .ostack()
        .iter()
        .map(|o| o.as_bool().unwrap())
        .collect();
    assert_eq!(
        flags,
        [
            true, true, true, true, true, true, true, true, true, true, false, true, false
        ]
    );
    let (interp, _, _, _) =
        run("true false and true false or true true xor false not 5 3 and 5 3 or 5 3 xor 0 not");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(false));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(stack[2].as_bool(), Some(false));
    assert_eq!(stack[3].as_bool(), Some(true));
    assert_eq!(stack[4].as_i32(), Some(1));
    assert_eq!(stack[5].as_i32(), Some(7));
    assert_eq!(stack[6].as_i32(), Some(6));
    assert_eq!(stack[7].as_i32(), Some(-1));
    let (interp, _, _, _) =
        run("1 3 bitshift -1 -1 bitshift 8 -2 bitshift 1 32 bitshift 1 -40 bitshift");
    assert_eq!(ints(&interp), [8, 0x7fff_ffff, 2, 0, 0]);
    for program in ["1 true and", "(a) 1 lt", "1 (a) gt", "/a not"] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some("typecheck"), "{program}");
    }
}

#[test]
fn dictionary_operators() {
    let (interp, outcome, _, _) = run(
        "3 dict dup /a 1 put begin a /b 2 def b currentdict /b known currentdict /c known /a where exch pop /a load /zz where 1 dict dup /x 1 put dup length exch maxlength countdictstack end countdictstack",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(1));
    assert_eq!(stack[1].as_i32(), Some(2));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_bool(), Some(false));
    assert_eq!(stack[4].as_bool(), Some(true));
    assert_eq!(stack[5].as_i32(), Some(1));
    assert_eq!(stack[6].as_bool(), Some(false));
    assert_eq!(stack[7].as_i32(), Some(1));
    assert_eq!(stack[8].as_i32(), Some(1));
    assert_eq!(stack[9].as_i32(), Some(4));
    assert_eq!(stack[10].as_i32(), Some(3));
    let (interp, outcome, _, _) = run(
        "/k 1 def 2 dict begin /k 2 store k end k 2 dict begin /m 3 store end /m where pop userdict eq",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack.len(), 2);
    assert_eq!(stack[0].as_i32(), Some(2));
    assert_eq!(stack[1].as_bool(), Some(false));
    let (interp, outcome, _, _) = run(
        "<< /a 1 /b 2 >> dup /b get exch dup /b undef length { null null null null null } dictstack length 1 dict begin 1 dict begin cleardictstack countdictstack",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [2, 1, 3, 3]);
    let (interp, _, _, _) = run(
        "{ 1 2 3 } dup 1 get exch dup 1 9 put 1 get (abc) dup 1 get exch dup 1 66 put 1 get /abc length",
    );
    assert_eq!(ints(&interp), [2, 9, 98, 66, 3]);
    for (program, error) in [
        ("1 dict /a get", "undefined"),
        ("/nosuch load", "undefined"),
        ("<< /a >>", "rangecheck"),
        (">>", "unmatchedmark"),
        ("1 begin", "typecheck"),
        ("-1 dict", "rangecheck"),
        ("1 length", "typecheck"),
        ("(abc) 1 300 put", "rangecheck"),
        ("{ 1 } 5 get", "rangecheck"),
        ("{ null } dictstack", "rangecheck"),
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
    }
}

#[test]
fn definitions_persist_across_runs() {
    let mut interp = Interp::new();
    assert_eq!(run_in(&mut interp, "/v 41 def"), Outcome::Ok);
    assert_eq!(run_in(&mut interp, "v 1 add"), Outcome::Ok);
    assert_eq!(ints(&interp), [42]);
}

// --- property test: random control nesting ---------------------------------------

#[derive(Clone, Debug)]
enum Stmt {
    Push(i32),
    Exec(Vec<Stmt>),
    Repeat(u8, Vec<Stmt>),
    For(u8, Vec<Stmt>),
    LoopExit(Vec<Stmt>),
    Stopped(Vec<Stmt>),
    Stop,
    Exit,
    Undefined,
}

fn stmt() -> impl Strategy<Value = Stmt> {
    let leaf = prop_oneof![
        4 => (0..100i32).prop_map(Stmt::Push),
        1 => Just(Stmt::Stop),
        1 => Just(Stmt::Exit),
        1 => Just(Stmt::Undefined),
    ];
    leaf.prop_recursive(4, 32, 4, |inner| {
        let block = prop::collection::vec(inner, 0..4);
        prop_oneof![
            block.clone().prop_map(Stmt::Exec),
            (0..4u8, block.clone()).prop_map(|(n, b)| Stmt::Repeat(n, b)),
            (0..3u8, block.clone()).prop_map(|(k, b)| Stmt::For(k, b)),
            block.clone().prop_map(Stmt::LoopExit),
            block.prop_map(Stmt::Stopped),
        ]
    })
}

fn serialize(block: &[Stmt], out: &mut String) {
    for stmt in block {
        match stmt {
            Stmt::Push(n) => out.push_str(&format!("{n} ")),
            Stmt::Exec(b) => {
                out.push_str("{ ");
                serialize(b, out);
                out.push_str("} exec ");
            }
            Stmt::Repeat(n, b) => {
                out.push_str(&format!("{n} {{ "));
                serialize(b, out);
                out.push_str("} repeat ");
            }
            Stmt::For(k, b) => {
                out.push_str(&format!("0 1 {k} {{ "));
                serialize(b, out);
                out.push_str("} for ");
            }
            Stmt::LoopExit(b) => {
                out.push_str("{ ");
                serialize(b, out);
                out.push_str("exit } loop ");
            }
            Stmt::Stopped(b) => {
                out.push_str("{ ");
                serialize(b, out);
                out.push_str("} stopped pop ");
            }
            Stmt::Stop => out.push_str("stop "),
            Stmt::Exit => out.push_str("exit "),
            Stmt::Undefined => out.push_str("nosuchname "),
        }
    }
}

#[derive(PartialEq, Debug)]
enum Flow {
    Normal,
    Exit,
    Stop,
}

#[derive(Default)]
struct Model {
    out: Vec<i32>,
    last_error: Option<&'static str>,
}

impl Model {
    fn block(&mut self, block: &[Stmt]) -> Flow {
        for stmt in block {
            let flow = self.stmt(stmt);
            if flow != Flow::Normal {
                return flow;
            }
        }
        Flow::Normal
    }

    fn stmt(&mut self, stmt: &Stmt) -> Flow {
        match stmt {
            Stmt::Push(n) => {
                self.out.push(*n);
                Flow::Normal
            }
            Stmt::Exec(b) => self.block(b),
            Stmt::Repeat(n, b) => {
                for _ in 0..*n {
                    match self.block(b) {
                        Flow::Normal => {}
                        Flow::Exit => return Flow::Normal,
                        Flow::Stop => return Flow::Stop,
                    }
                }
                Flow::Normal
            }
            Stmt::For(k, b) => {
                for i in 0..=i32::from(*k) {
                    self.out.push(i);
                    match self.block(b) {
                        Flow::Normal => {}
                        Flow::Exit => return Flow::Normal,
                        Flow::Stop => return Flow::Stop,
                    }
                }
                Flow::Normal
            }
            Stmt::LoopExit(b) => match self.block(b) {
                Flow::Normal | Flow::Exit => Flow::Normal,
                Flow::Stop => Flow::Stop,
            },
            Stmt::Stopped(b) => match self.block(b) {
                Flow::Normal | Flow::Stop => Flow::Normal,
                Flow::Exit => {
                    self.last_error = Some("invalidexit");
                    Flow::Normal
                }
            },
            Stmt::Stop => Flow::Stop,
            Stmt::Exit => Flow::Exit,
            Stmt::Undefined => {
                self.last_error = Some("undefined");
                Flow::Stop
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn random_control_nesting_keeps_the_host_depth_constant(program in prop::collection::vec(stmt(), 0..6)) {
        let mut text = String::new();
        serialize(&program, &mut text);
        let (mut interp, _, _) = interp_with(Limits { operand: 100_000, ..Default::default() });
        let outcome = run_in(&mut interp, &text);

        let mut model = Model::default();
        let expected = match model.block(&program) {
            Flow::Normal => None,
            Flow::Exit => Some("invalidexit"),
            Flow::Stop => model.last_error,
        };
        prop_assert_eq!(error_name(&outcome), expected, "program: {}", text);
        prop_assert_eq!(ints(&interp), model.out, "program: {}", text);
        prop_assert!(interp.estack().is_empty());
        prop_assert_eq!(interp.dstack().len(), 3);
        #[cfg(debug_assertions)]
        prop_assert_eq!(interp.max_host_depth(), 1);
    }
}

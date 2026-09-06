// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Interpreter scenarios, each naming the `corpus/unit/interp/*.ps` file it
//! mirrors where one exists, plus the property test that random control
//! nesting keeps the host recursion depth constant, and the stack-limit
//! tests.

use proptest::prelude::*;
use ps_vm::{
    Access, Capabilities, Capture, ChunkSource, Config, ErrorSummary, FileCapability, Interp, Io,
    Limits, Object, Outcome, SliceSource, Stream, Type, VmError,
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
        run_in(&mut interp, "/q { 1 } def /q load readonly bind 0 get"),
        Outcome::Ok
    );
    assert_eq!(top(&interp).as_i32(), Some(1));
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

// --- part 2: arrays, strings, types, VM, files ------------------------------------

/// An injected input stream over fixed bytes.
struct Input(Vec<u8>, usize);

impl Stream for Input {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        let n = buf.len().min(self.0.len() - self.1);
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }
}

/// A file capability answering one name with fixed bytes and recording
/// what was written to it.
struct OneFile {
    name: &'static str,
    content: &'static [u8],
    written: Capture,
}

impl FileCapability for OneFile {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError> {
        if name != self.name.as_bytes() {
            return Err(VmError::UndefinedFileName);
        }
        match mode {
            b"r" => Ok(Box::new(Input(self.content.to_vec(), 0))),
            b"w" => Ok(Box::new(self.written.clone())),
            _ => Err(VmError::InvalidFileAccess),
        }
    }
}

fn run_with(io: Io, file: Option<Box<dyn FileCapability>>, program: &str) -> (Interp, Outcome) {
    let config = Config {
        io,
        capabilities: Capabilities { file },
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = run_in(&mut interp, program);
    (interp, outcome)
}

// array-and-string-operators.ps
#[test]
fn array_and_string_operators() {
    let (interp, outcome, out, _) = run(
        "3 array dup length exch 0 get [ 1 2 ] length 1 2 2 array astore aload pop \
         (abcdef) 2 3 getinterval (abc) dup 1 (Z) putinterval [ 1 2 3 ] 1 1 getinterval \
         (hello) (l) search { pop pop pop 1 } { pop 0 } ifelse (hello) (lo) anchorsearch \
         { pop pop 1 } { pop 0 } ifelse 3 string length 1 2 2 packedarray length \
         [ 1 2 ] { == } forall (ab) { = } forall << /k 1 >> { == == } forall",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "1\n2\n97\n98\n1\n/k\n");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(3));
    assert_eq!(stack[1].ty(), Type::Null);
    assert_eq!(stack[2].as_i32(), Some(2));
    assert_eq!(stack[3].as_i32(), Some(1));
    assert_eq!(stack[4].as_i32(), Some(2));
    assert_eq!(interp.memory().string(stack[5]), Some(&b"cde"[..]));
    assert_eq!(interp.memory().string(stack[6]), Some(&b"aZc"[..]));
    assert_eq!(interp.memory().array(stack[7]).unwrap().len(), 1);
    assert_eq!(stack[8].as_i32(), Some(1));
    assert_eq!(stack[9].as_i32(), Some(0));
    assert_eq!(stack[10].as_i32(), Some(3));
    assert_eq!(stack[11].as_i32(), Some(2));
    assert_eq!(stack.len(), 12);

    let (interp, outcome, _, _) = run("(hello) (l) search");
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(interp.memory().string(stack[0]), Some(&b"lo"[..]));
    assert_eq!(interp.memory().string(stack[1]), Some(&b"l"[..]));
    assert_eq!(interp.memory().string(stack[2]), Some(&b"he"[..]));
    assert_eq!(stack[3].as_bool(), Some(true));
    let (interp, _, _, _) = run("(hello) (lo) anchorsearch (hello) () anchorsearch");
    let stack = interp.ostack();
    assert_eq!(interp.memory().string(stack[0]), Some(&b"hello"[..]));
    assert_eq!(stack[1].as_bool(), Some(false));
    assert_eq!(interp.memory().string(stack[2]), Some(&b"hello"[..]));
    assert_eq!(interp.memory().string(stack[3]), Some(&b""[..]));
    assert_eq!(stack[4].as_bool(), Some(true));

    let (interp, _, _, _) = run("[ 1 2 3 4 ] { dup 3 eq { exit } if pop } forall");
    assert_eq!(ints(&interp), [3]);
    let (interp, _, _, _) = run("0 [ 1 2 3 ] { add } forall 0 (abc) { add } forall");
    assert_eq!(ints(&interp), [6, 294]);
    let (interp, _, _, _) = run("[ ] { 1 } forall 0 dict { 1 } forall 7");
    assert_eq!(ints(&interp), [7]);

    for (program, error) in [
        ("-1 array", "rangecheck"),
        ("70000 string", "limitcheck"),
        ("1 ]", "unmatchedmark"),
        ("1 2 array astore", "stackunderflow"),
        ("(abc) 2 3 getinterval", "rangecheck"),
        ("[ 1 ] 0 (a) putinterval", "typecheck"),
        ("1 { } forall", "typecheck"),
        ("[ 1 ] noaccess { } forall", "invalidaccess"),
        ("[ 1 ] noaccess aload", "invalidaccess"),
        ("[ 1 ] readonly 0 1 getinterval 0 2 put", "invalidaccess"),
        ("[ (x) true setglobal ]", "invalidaccess"),
        (
            "true setglobal 1 array false setglobal 0 (x) put",
            "invalidaccess",
        ),
        ("(a) 1 search", "typecheck"),
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
    }
    let (interp, outcome, _, _) = run("1 2 mark 3 4 ]");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(interp.ostack().len(), 3);
    assert_eq!(interp.memory().array(top(&interp)).unwrap().len(), 2);
}

#[test]
fn setpacking_governs_scanned_procedures() {
    let (interp, outcome, _, _) = run(
        "currentpacking true setpacking currentpacking { 1 } [ 1 ] 1 1 packedarray false setpacking { 1 }",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(false));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(stack[2].ty(), Type::PackedArray);
    assert!(stack[2].is_executable());
    assert_eq!(stack[2].access(), Some(Access::ReadOnly));
    assert_eq!(stack[3].ty(), Type::Array);
    assert_eq!(stack[4].ty(), Type::PackedArray);
    assert!(stack[4].is_literal());
    assert_eq!(stack[5].ty(), Type::Array);
    let (interp, _, _, _) = run("true setpacking { 1 2 add } exec { 3 } exec");
    assert_eq!(ints(&interp), [3, 3]);
    assert!(interp.memory().current_packing());
}

// type-and-conversion.ps
#[test]
fn type_attribute_and_conversion_operators() {
    let (_, outcome, out, _) = run(
        "1 type == 1.5 type == (a) type == /a type == true type == null type == mark type == \
         [ ] type == { } type == 1 2 1 packedarray type == 1 dict type == /add load type == \
         currentfile type == save type == 1 type xcheck =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        out.text(),
        "integertype\nrealtype\nstringtype\nnametype\nbooleantype\nnulltype\nmarktype\n\
         arraytype\narraytype\npackedarraytype\ndicttype\noperatortype\nfiletype\n\
         savetype\ntrue\n"
    );

    let (interp, _, _, _) = run(
        "/a cvx xcheck /a xcheck { } cvlit xcheck (a) cvx xcheck 1 cvx xcheck \
         [ 1 ] readonly dup rcheck exch wcheck [ 1 ] executeonly dup rcheck exch wcheck \
         [ 1 ] noaccess rcheck (a) rcheck 1 dict readonly rcheck 1 dict readonly wcheck \
         1 dict dup noaccess pop rcheck",
    );
    let bools: Vec<bool> = interp
        .ostack()
        .iter()
        .map(|o| o.as_bool().unwrap())
        .collect();
    assert_eq!(
        bools,
        [
            true, false, false, true, true, true, false, false, false, false, true, true, false,
            false
        ]
    );

    let (interp, outcome, _, _) = run(
        "3.7 cvi -3.7 cvi (42) cvi ( 42 ) cvi (16#ff) cvi 2147483647 cvi \
         (3.5e1) cvr 5 cvr 2.5 cvr (1.) cvr",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(
        stack[..6]
            .iter()
            .map(|o| o.as_i32().unwrap())
            .collect::<Vec<_>>(),
        [3, -3, 42, 42, 255, 2147483647]
    );
    assert_eq!(
        stack[6..]
            .iter()
            .map(|o| o.as_f32().unwrap())
            .collect::<Vec<_>>(),
        [35.0, 5.0, 2.5, 1.0]
    );

    let (interp, outcome, _, _) = run("(abc) cvn (abc) cvx cvn /abc");
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert!(stack[0].eq(stack[2]));
    assert!(stack[0].is_literal());
    assert!(stack[1].eq(stack[2]));
    assert!(stack[1].is_executable());

    let (_, outcome, out, _) = run(
        "1.5 10 string cvs == -7 10 string cvs == /abc 10 string cvs == (xy) 10 string cvs == \
         true 10 string cvs == 5.0 10 string cvs == { } 20 string cvs == /add load 10 string cvs == \
         null 20 string cvs == 2147483648.0 20 string cvs ==",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        out.text(),
        "(1.5)\n(-7)\n(abc)\n(xy)\n(true)\n(5.0)\n(--nostringval--)\n(add)\n(--nostringval--)\n(2147483648.0)\n"
    );
    let (interp, _, _, _) = run("(abcdef) 10 string cvs dup length");
    assert_eq!(top(&interp).as_i32(), Some(6));
    assert_eq!(
        interp.memory().string(interp.ostack()[0]),
        Some(&b"abcdef"[..])
    );

    let (_, outcome, out, _) = run(
        "255 16 10 string cvrs == -1 16 10 string cvrs == 10 2 10 string cvrs == \
         1.9 10 10 string cvrs == 35 36 3 string cvrs == 0 8 3 string cvrs == \
         255.9 16 4 string cvrs == -7 10 4 string cvrs ==",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        out.text(),
        "(FF)\n(FFFFFFFF)\n(1010)\n(1.9)\n(Z)\n(0)\n(FF)\n(-7)\n"
    );

    for (program, error) in [
        ("1e10 cvi", "rangecheck"),
        ("(abc) cvi", "typecheck"),
        ("(1 2) cvi", "typecheck"),
        ("(]) cvi", "typecheck"),
        ("null cvi", "typecheck"),
        ("null cvr", "typecheck"),
        ("12345 2 string cvs", "rangecheck"),
        ("1 1 3 string cvrs", "rangecheck"),
        ("1 37 3 string cvrs", "rangecheck"),
        ("256 16 1 string cvrs", "rangecheck"),
        ("1 readonly", "typecheck"),
        ("1 dict executeonly", "typecheck"),
        ("[ 1 ] executeonly readonly", "invalidaccess"),
        ("1 dict noaccess readonly", "invalidaccess"),
        ("1 rcheck", "typecheck"),
        ("(abc) readonly dup 0 65 put", "invalidaccess"),
        ("(abc) noaccess cvn", "invalidaccess"),
        ("(abc) 1 cvs", "typecheck"),
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
    }
    let long = format!("({}) cvn", "x".repeat(128));
    let (_, outcome, _, _) = run(&long);
    assert_eq!(error_name(&outcome), Some("limitcheck"));
    let (interp, _, _, _) = run("(abc) readonly (abc) executeonly noaccess");
    assert_eq!(interp.ostack()[0].access(), Some(Access::ReadOnly));
    assert_eq!(interp.ostack()[1].access(), Some(Access::None));
}

// save-restore-exec-stack.ps
#[test]
fn vm_operators() {
    let (interp, outcome, out, _) = run("/a [ 1 2 3 ] def save a 0 9 put restore a 0 get = \
         true setglobal /g [ 1 ] def currentglobal false setglobal save g 0 2 put restore g 0 get = \
         save [ 1 2 ] exch { restore } stopped pop $error /errorname get == exch pop restore \
         [ 1 ] gcheck 1 gcheck true setglobal [ 1 ] gcheck false setglobal \
         vmstatus pop pop save vmstatus pop pop exch restore");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "1\n2\n/invalidrestore\n");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(stack[1].as_bool(), Some(false));
    assert_eq!(stack[2].as_bool(), Some(true));
    assert_eq!(stack[3].as_bool(), Some(true));
    assert_eq!(stack[4].as_i32(), Some(0));
    assert_eq!(stack[5].as_i32(), Some(1));
    assert_eq!(stack.len(), 6);

    let (interp, outcome, _, _) = run("/r { restore 1 } def save r");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [1]);
    let (_, outcome, _, _) = run("save (1 { restore } repeat) cvx exec");
    assert_eq!(error_name(&outcome), Some("invalidrestore"));
    let (_, outcome, _, _) = run("save 1 dict begin restore");
    assert_eq!(error_name(&outcome), Some("invalidrestore"));
    let (_, outcome, _, _) = run("save ({ restore } exec) cvx exec 2");
    assert_eq!(error_name(&outcome), Some("invalidrestore"));
    let (interp, outcome, _, _) = run("/s ({ restore } exec) def save s cvx exec 2");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(ints(&interp), [2]);
    let (_, outcome, _, _) = run("1 restore");
    assert_eq!(error_name(&outcome), Some("typecheck"));
    let (_, outcome, _, _) = run("save dup restore restore");
    assert_eq!(error_name(&outcome), Some("invalidrestore"));
    let (_, outcome, _, _) = run("16 { save } repeat");
    assert_eq!(error_name(&outcome), Some("limitcheck"));
    let (_, outcome, _, _) = run("(x) setglobal");
    assert_eq!(error_name(&outcome), Some("typecheck"));
}

// standard-files.ps, currentfile-reads-inline-data.ps
#[test]
fn standard_files_and_currentfile() {
    let (_, outcome, out, err) = run(
        "(%stdout) (w) file dup (via stdout\\n) writestring dup 65 write 10 write flush \
         (%stderr) (w) file (oops\\n) writestring \
         currentfile 40 string readline\nthe data line\n== = \
         currentfile 5 string readstring\nXYZWV\n== = \
         currentfile 3 string readhexstring\n41 4243\n== = \
         currentfile token\n/tokenized\n== == \
         currentfile read\nA= = \
         currentfile currentfile eq = currentfile type == \
         (12 ab) token { == == } if (   ) token = (%stdout) (w) file (%stdout) (w) file eq =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        out.text(),
        "via stdout\nA\ntrue\nthe data line\ntrue\nXYZWV\ntrue\nABC\ntrue\n/tokenized\ntrue\n65\ntrue\nfiletype\n12\n(ab)\nfalse\ntrue\n"
    );
    assert_eq!(err.text(), "oops\n");

    let (interp, outcome) = run_with(
        Io::capture()
            .0
            .with_stdin(Input(b"7 (in)\nrest".to_vec(), 0)),
        None,
        "(%stdin) (r) file dup token pop exch dup token pop exch 10 string readline pop",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(7));
    assert_eq!(interp.memory().string(stack[1]), Some(&b"in"[..]));
    assert_eq!(interp.memory().string(stack[2]), Some(&b""[..]));
    assert_eq!(stack.len(), 3);
    let (interp, outcome) = run_with(
        Io::capture().0.with_stdin(Input(b"ab".to_vec(), 0)),
        None,
        "(%stdin) (r) file dup read pop exch dup read pop exch read",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(interp.ostack()[0].as_i32(), Some(97));
    assert_eq!(interp.ostack()[1].as_i32(), Some(98));
    assert_eq!(interp.ostack()[2].as_bool(), Some(false));

    for (program, error) in [
        ("(%stdin) (r) file", "undefinedfilename"),
        ("(%stdout) (r) file", "invalidfileaccess"),
        ("(%stderr) (x) file", "invalidfileaccess"),
        ("(other) (r) file", "undefinedfilename"),
        ("(%stdout) (w) file 256 write", "rangecheck"),
        ("currentfile 65 write", "invalidaccess"),
        ("currentfile 0 string readstring", "rangecheck"),
        ("currentfile 2 string readline\nabc\n", "rangecheck"),
        ("1 closefile", "typecheck"),
        ("1 token", "typecheck"),
        ("(x) 1 readline", "typecheck"),
        ("currentfile (abc) readonly readline", "invalidaccess"),
        ("eexec", "stackunderflow"),
        ("1 eexec", "typecheck"),
        ("(}) token", "syntaxerror"),
        ("currentfile token\n}", "syntaxerror"),
        (
            "(%stdout) (w) file dup closefile (x) writestring",
            "ioerror",
        ),
    ] {
        let (_, outcome, _, _) = run(program);
        assert_eq!(error_name(&outcome), Some(error), "{program}");
    }
    let mut interp = Interp::new();
    assert_eq!(
        run_in(&mut interp, "(%stdout) (w) file"),
        Outcome::Error(ErrorSummary {
            name: "undefinedfilename".into(),
            command: "file".into(),
        })
    );
}

#[test]
fn readline_handles_line_endings_and_end_of_data() {
    let (interp, outcome) = run_with(
        Io::capture()
            .0
            .with_stdin(Input(b"a\r\nb\rc\nd".to_vec(), 0)),
        None,
        "/f (%stdin) (r) file def 4 { f 10 string readline } repeat f 10 string readline",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    let text = |o| String::from_utf8(interp.memory().string(o).unwrap().to_vec()).unwrap();
    assert_eq!(text(stack[0]), "a");
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(text(stack[2]), "b");
    assert_eq!(text(stack[4]), "c");
    assert_eq!(text(stack[6]), "d");
    assert_eq!(stack[7].as_bool(), Some(false));
    assert_eq!(text(stack[8]), "");
    assert_eq!(stack[9].as_bool(), Some(false));
    let (interp, _) = run_with(
        Io::capture().0.with_stdin(Input(b"abcde".to_vec(), 0)),
        None,
        "(%stdin) (r) file dup 3 string readstring exch pop exch 3 string readstring",
    );
    let stack = interp.ostack();
    assert_eq!(stack[0].as_bool(), Some(true));
    assert_eq!(interp.memory().string(stack[1]), Some(&b"de"[..]));
    assert_eq!(stack[2].as_bool(), Some(false));
    let (interp, _) = run_with(
        Io::capture().0.with_stdin(Input(b"4x1 42>".to_vec(), 0)),
        None,
        "(%stdin) (r) file 4 string readhexstring",
    );
    let stack = interp.ostack();
    assert_eq!(interp.memory().string(stack[0]), Some(&b"\x41\x42"[..]));
    assert_eq!(stack[1].as_bool(), Some(false));
}

#[test]
fn files_open_through_the_capability_only() {
    let written = Capture::new();
    let capability = OneFile {
        name: "data",
        content: b"1 2 add\n(rest)",
        written: written.clone(),
    };
    let (interp, outcome) = run_with(
        Io::capture().0,
        Some(Box::new(capability)),
        "/f (data) (r) file def f token pop f token pop f token pop f cvx exec \
         f 10 string readline (data) (w) file dup (out) writestring closefile \
         { (data) (a) file } stopped { pop pop } if $error /errorname get \
         { (nope) (r) file } stopped { pop pop } if $error /errorname get \
         f closefile { f read } stopped { pop } if $error /errorname get",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(written.text(), "out");
    let stack = interp.ostack();
    assert_eq!(stack[0].as_i32(), Some(1));
    assert_eq!(stack[1].as_i32(), Some(2));
    assert_eq!(stack[2].ty(), Type::Name);
    assert!(stack[2].is_executable());
    assert_eq!(name_text(&interp, stack[2]), "add");
    assert_eq!(interp.memory().string(stack[3]), Some(&b"rest"[..]));
    assert_eq!(interp.memory().string(stack[4]), Some(&b""[..]));
    assert_eq!(stack[5].as_bool(), Some(false));
    assert_eq!(name_text(&interp, stack[6]), "invalidfileaccess");
    assert_eq!(name_text(&interp, stack[7]), "undefinedfilename");
    assert_eq!(name_text(&interp, stack[8]), "ioerror");
    assert_eq!(stack.len(), 9);

    let (interp, outcome) = run_with(
        Io::capture().0,
        Some(Box::new(OneFile {
            name: "prog",
            content: b"currentfile 10 string readline\ninline\n== 5",
            written: Capture::new(),
        })),
        "(prog) (r) file cvx exec currentfile",
    );
    assert_eq!(outcome, Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(interp.memory().string(stack[0]), Some(&b"inline"[..]));
    assert_eq!(stack[1].as_i32(), Some(5));
    assert!(stack[2].eq(interp.run_file()));
    assert_eq!(stack.len(), 3);
}

#[test]
fn the_run_file_is_the_job_source() {
    let (interp, outcome, _, _) = run("currentfile currentfile closefile (unreached) =");
    assert_eq!(outcome, Outcome::Ok);
    assert!(top(&interp).eq(interp.run_file()));
    assert!(interp.memory().file_is_open(interp.run_file()));

    let mut interp = Interp::new();
    assert_eq!(run_in(&mut interp, "1 quit 2 3"), Outcome::Ok);
    assert_eq!(run_in(&mut interp, "4"), Outcome::Ok);
    assert_eq!(ints(&interp), [1, 4]);
    assert_eq!(run_in(&mut interp, "currentfile closefile 5"), Outcome::Ok);
    assert_eq!(
        run_in(&mut interp, "6 currentfile 8 string readline\n7 8\n"),
        Outcome::Ok
    );
    let stack = interp.ostack();
    assert_eq!(stack[2].as_i32(), Some(6));
    assert_eq!(interp.memory().string(stack[3]), Some(&b"7 8"[..]));
    assert_eq!(stack[4].as_bool(), Some(true));
    assert_eq!(stack.len(), 5);
    assert_eq!(
        run_in(&mut interp, "save currentfile closefile"),
        Outcome::Ok
    );
    assert_eq!(run_in(&mut interp, "restore 9"), Outcome::Ok);
    assert_eq!(top(&interp).as_i32(), Some(9));

    let (mut interp, _, _) = interp_with(Limits::default());
    let mut source = ChunkSource::new();
    source.append(b"currentfile 10 string readline\nabc\n(x");
    assert_eq!(interp.run(&mut source), Outcome::Suspended);
    source.append(b"y) 1");
    source.finish();
    assert_eq!(interp.resume(&mut source), Outcome::Ok);
    let stack = interp.ostack();
    assert_eq!(interp.memory().string(stack[0]), Some(&b"abc"[..]));
    assert_eq!(stack[1].as_bool(), Some(true));
    assert_eq!(interp.memory().string(stack[2]), Some(&b"xy"[..]));
    assert_eq!(stack[3].as_i32(), Some(1));
    assert_eq!(stack.len(), 4);
}

// captured-output.ps
#[test]
fn double_equals_pstack_and_stack() {
    let (interp, outcome, out, _) = run(
        "(a(b)\\\\\\n\\t\\001\\177) == /n == /n cvx == [ 1 (x) [ ] ] == { 1 [ 2 ] { } } == \
         /add load == mark == null == 1 dict == currentfile == save == 1 2 == \
         [ 1 ] noaccess == (x) executeonly == [ 1 ] executeonly ==",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        out.text(),
        "(a\\(b\\)\\\\\\n\\t\\001\\177)\n/n\nn\n[1 (x) []]\n{1 [ 2 ] {}}\n--add--\n-mark-\nnull\n-dict-\n-file-\n-save-\n2\n--nostringval--\n--nostringval--\n--nostringval--\n"
    );
    assert_eq!(interp.ostack().len(), 1);
    let (interp, outcome, out, _) = run("1 (a) /b [ 1 ] pstack stack count");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "[1]\n/b\n(a)\n1\n--nostringval--\nb\na\n1\n");
    assert_eq!(top(&interp).as_i32(), Some(4));
    let (_, outcome, out, _) = run("/a [ 1 ] def a 0 a put a ==");
    assert_eq!(outcome, Outcome::Ok);
    assert!(out.text().starts_with("[[[["));
    assert!(out.text().contains("..."));
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

// --- the execution budget ------------------------------------------------

#[test]
fn budget_ends_an_endless_loop_with_limitcheck() {
    let (mut interp, _, err) = interp_with(Limits {
        steps: Some(10_000),
        ..Default::default()
    });
    let outcome = run_in(&mut interp, "{ } loop");
    assert_eq!(error_name(&outcome), Some("limitcheck"));
    assert!(err.text().contains("OffendingCommand: loop"));
    assert!(interp.budget_exceeded());
    assert!(interp.steps() > 10_000);
    // The budget is per interpreter, not per job: what the grace left is
    // all a later job gets.
    assert_eq!(
        error_name(&run_in(&mut interp, "{ } loop")),
        Some("limitcheck")
    );
    assert!(interp.steps() < 12_000);
}

#[test]
fn budget_is_attributed_to_the_object_being_executed() {
    let (mut interp, _, _) = interp_with(Limits {
        steps: Some(100),
        ..Default::default()
    });
    let outcome = run_in(&mut interp, "0 { 1 add } loop");
    let Outcome::Error(summary) = outcome else {
        panic!("{outcome:?}");
    };
    assert_eq!(summary.name, "limitcheck");
    assert!(
        ["1", "add", "loop"].contains(&summary.command.as_str()),
        "{summary:?}"
    );
}

#[test]
fn budget_error_is_catchable_and_a_handler_may_report() {
    let (mut interp, out, _) = interp_with(Limits {
        steps: Some(5_000),
        ..Default::default()
    });
    let outcome = run_in(
        &mut interp,
        "{ { } loop } stopped { $error /errorname get = } if (after) =",
    );
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(out.text(), "limitcheck\nafter\n");
}

#[test]
fn budget_stops_a_handler_that_loops() {
    let (mut interp, _, _) = interp_with(Limits {
        steps: Some(5_000),
        ..Default::default()
    });
    let outcome = run_in(
        &mut interp,
        "errordict /limitcheck { pop { } loop } put { } loop (unreached) =",
    );
    assert_eq!(error_name(&outcome), Some("limitcheck"));
    let (mut interp, out, _) = interp_with(Limits {
        steps: Some(5_000),
        ..Default::default()
    });
    let outcome = run_in(
        &mut interp,
        "{ { { } loop } stopped pop { } loop } stopped pop (unreached) =",
    );
    assert_eq!(error_name(&outcome), Some("limitcheck"));
    assert_eq!(out.text(), "");
}

#[test]
fn no_budget_leaves_execution_unbounded() {
    let (mut interp, _, _) = interp_with(Limits::default());
    let outcome = run_in(&mut interp, "0 1 1 100000 { add } for");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(interp.steps(), 0);
    assert!(!interp.budget_exceeded());
}

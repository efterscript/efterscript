// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The job server (PLRM3 §3.7.7): encapsulated jobs, `startjob` and
//! `exitserver` under it, administrator jobs, parameters across jobs,
//! the per-job budget and reset, and bounded growth. Each scenario runs
//! several jobs on one interpreter without a graphics backend; the
//! page-bearing ones are `platen` tests.

use efterscript_vm::{Capture, Config, Interp, Io, JobEnd, Limits, Outcome, SliceSource};

struct Server {
    interp: Interp,
    out: Capture,
}

impl Server {
    fn new() -> Self {
        Self::with(Config::default())
    }

    fn with(config: Config) -> Self {
        let (io, out, _) = Io::capture();
        let interp = Interp::try_with_config(Config { io, ..config }).expect("builds");
        Server { interp, out }
    }

    /// Runs `program` as one job; its outcome, its output, and how it ended.
    fn job(&mut self, program: &str) -> (Outcome, String, JobEnd) {
        self.out.clear();
        self.interp.begin_job().expect("no job is open");
        let outcome = self.interp.run(&mut SliceSource::new(program.as_bytes()));
        let end = self.interp.end_job();
        (outcome, self.out.text(), end)
    }

    fn output(&mut self, program: &str) -> String {
        let (outcome, text, _) = self.job(program);
        assert_eq!(outcome, Outcome::Ok, "{program}: {text}");
        text
    }
}

fn error_name(outcome: &Outcome) -> &str {
    match outcome {
        Outcome::Error(e) => &e.name,
        other => panic!("expected an error, got {other:?}"),
    }
}

// --- encapsulation -----------------------------------------------------------

#[test]
fn a_jobs_definitions_do_not_reach_the_next_job() {
    let mut s = Server::new();
    let (_, _, end) = s.job("/x 1 def true setglobal globaldict /g 2 put false setglobal");
    assert!(!end.permanent);
    assert_eq!(s.output("/x where = /g where ="), "false\nfalse\n");
}

#[test]
fn a_job_starts_from_the_initial_state() {
    let mut s = Server::new();
    s.job("true setglobal 1 2 3 userdict begin true setpacking");
    assert_eq!(
        s.output("count = currentglobal = countdictstack = currentpacking ="),
        "0\nfalse\n3\nfalse\n"
    );
}

#[test]
fn an_error_does_not_leak() {
    let mut s = Server::new();
    let (outcome, _, _) = s.job("nosuchname");
    assert_eq!(error_name(&outcome), "undefined");
    assert_eq!(s.output("$error /newerror get ="), "false\n");
}

#[test]
fn the_job_save_is_the_outermost_level() {
    let mut s = Server::new();
    assert_eq!(s.output("vmstatus pop pop ="), "1\n");
}

#[test]
fn a_second_begin_is_refused() {
    let mut s = Server::new();
    s.interp.begin_job().unwrap();
    assert!(s.interp.begin_job().is_err());
    s.interp.end_job();
    assert!(!s.interp.in_job());
}

// --- startjob ------------------------------------------------------------------

#[test]
fn an_unencapsulated_download_persists() {
    let mut s = Server::new();
    let (_, text, end) = s.job("true 0 startjob = /resident (kept) def");
    assert_eq!(text, "true\n");
    assert!(end.permanent);
    assert_eq!(s.output("resident ="), "kept\n");
}

#[test]
fn back_to_encapsulation_within_a_file() {
    let mut s = Server::new();
    s.job("true 0 startjob pop /kept 1 def false 0 startjob pop /dropped 2 def");
    assert_eq!(
        s.output("/kept where exch pop = /dropped where ="),
        "true\nfalse\n"
    );
}

#[test]
fn startjob_ends_the_current_job_first() {
    let mut s = Server::new();
    s.job("/early 1 def true 0 startjob pop /late 2 def");
    assert_eq!(
        s.output("/early where = /late where exch pop ="),
        "false\ntrue\n"
    );
}

#[test]
fn startjob_is_refused_inside_a_nested_save() {
    let mut s = Server::new();
    let (_, text, end) = s.job("save true 0 startjob = restore /x 1 def");
    assert_eq!(text, "false\n");
    assert!(!end.permanent);
    assert_eq!(s.output("/x where ="), "false\n");
}

#[test]
fn startjob_is_refused_with_a_wrong_password() {
    let mut s = Server::new();
    assert_eq!(
        s.output("true 1 startjob = true (0) startjob ="),
        "false\ntrue\n"
    );
}

#[test]
fn startjob_outside_a_job_server() {
    let mut s = Server::new();
    s.out.clear();
    let outcome = s
        .interp
        .run(&mut SliceSource::new(b"true 0 startjob =".as_slice()));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(s.out.text(), "false\n");
}

#[test]
fn startjob_resets_the_stacks() {
    let mut s = Server::new();
    assert_eq!(
        s.output("1 2 3 userdict begin true 0 startjob count = countdictstack = pop"),
        "1\n3\n"
    );
}

// --- exitserver ------------------------------------------------------------------

#[test]
fn exitserver_announces_itself_and_persists() {
    let mut s = Server::new();
    let (_, text, end) = s.job("serverdict begin 0 exitserver /persist 1 def");
    assert_eq!(text, "%%[exitserver: permanent state may be changed]%%\n");
    assert!(end.permanent);
    assert_eq!(s.output("persist ="), "1\n");
}

#[test]
fn exitserver_with_a_wrong_password() {
    let mut s = Server::new();
    let (outcome, _, end) = s.job("serverdict begin 1 exitserver /x 1 def");
    assert_eq!(error_name(&outcome), "invalidaccess");
    assert!(!end.permanent);
    assert_eq!(s.output("/x where ="), "false\n");
}

#[test]
fn exitserver_is_quiet_when_errors_are_binary() {
    let mut s = Server::new();
    let (_, text, _) = s.job("$error /binary true put serverdict begin 0 exitserver");
    assert_eq!(text, "");
}

#[test]
fn exitserver_outside_a_job_server_writes_nothing() {
    let mut s = Server::new();
    s.out.clear();
    let outcome = s.interp.run(&mut SliceSource::new(
        b"serverdict begin 0 exitserver /persist 1 def persist =".as_slice(),
    ));
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(s.out.text(), "1\n");
}

// --- administrator jobs and parameters ---------------------------------------

#[test]
fn an_administrator_job_needs_no_password() {
    let mut s = Server::new();
    let (outcome, _, _) = s.job("true 0 startjob pop << /MaxFontCache 123456 >> setsystemparams");
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(
        s.output("currentsystemparams /MaxFontCache get ="),
        "123456\n"
    );
}

#[test]
fn an_ordinary_unencapsulated_job_still_needs_the_password() {
    let mut s = Server::new();
    s.job("<< /Password 0 /StartJobPassword (sj) >> setsystemparams");
    let (outcome, _, _) = s.job("true (sj) startjob pop << /MaxFontCache 1 >> setsystemparams");
    assert_eq!(error_name(&outcome), "invalidaccess");
}

#[test]
fn job_name_does_not_outlive_its_job() {
    let mut s = Server::new();
    s.job("<< /JobName (first) >> setuserparams");
    assert_eq!(s.output("currentuserparams /JobName get length ="), "0\n");
}

#[test]
fn an_unencapsulated_user_parameter_becomes_the_default() {
    let mut s = Server::new();
    s.job("true 0 startjob pop << /MaxFontItem 777 >> setuserparams");
    assert_eq!(s.output("currentuserparams /MaxFontItem get ="), "777\n");
}

#[test]
fn a_system_parameter_set_by_an_encapsulated_job_persists() {
    let mut s = Server::new();
    s.job("<< /Password 0 /MaxFormCache 4242 >> setsystemparams");
    assert_eq!(
        s.output("currentsystemparams /MaxFormCache get ="),
        "4242\n"
    );
}

// --- pending saves, budget, quit ------------------------------------------------

#[test]
fn an_unencapsulated_jobs_own_save_is_restored_at_its_end() {
    let mut s = Server::new();
    s.job("true 0 startjob pop /a 1 def save /b 2 def");
    assert_eq!(s.output("/a where exch pop = /b where ="), "true\nfalse\n");
}

#[test]
fn the_outermost_save_of_an_unencapsulated_job_covers_global_vm() {
    let mut s = Server::new();
    let text = s.output(
        "true 0 startjob pop true setglobal globaldict /g [1] put false setglobal save \
         globaldict /g get 0 2 put restore globaldict /g get 0 get =",
    );
    assert_eq!(text, "1\n");
}

#[test]
fn budgets_do_not_accumulate() {
    let mut s = Server::with(Config {
        limits: Limits {
            steps: Some(100_000),
            ..Limits::default()
        },
        ..Config::default()
    });
    for _ in 0..3 {
        let (outcome, _, _) = s.job("0 1 20000 { pop } for");
        assert_eq!(outcome, Outcome::Ok);
    }
    assert!(!s.interp.budget_exceeded());
}

#[test]
fn quit_ends_a_job_not_the_server() {
    let mut s = Server::new();
    s.job("quit (unreached) =");
    assert_eq!(s.output("(next) ="), "next\n");
}

#[test]
fn a_job_abandoned_mid_program_is_reverted() {
    let mut s = Server::new();
    s.out.clear();
    s.interp.begin_job().unwrap();
    let outcome = s.interp.run(&mut efterscript_vm::ChunkSource::new());
    assert_eq!(outcome, Outcome::Suspended);
    let mut source = efterscript_vm::ChunkSource::new();
    source.append(b"/x 1 def { 1 } repeat");
    let _ = s.interp.resume(&mut source);
    s.interp.end_job();
    assert_eq!(s.output("/x where ="), "false\n");
}

// --- growth --------------------------------------------------------------------

#[test]
fn repeated_jobs_return_vm_to_its_size() {
    let mut s = Server::new();
    let job = "0 1 999 { 4 string cvs cvn { 1 2 add } def } for \
               /Times-Roman findfont 12 scalefont setfont \
               true setglobal globaldict /big 100 array put false setglobal";
    s.job(job);
    let after_first = s
        .interp
        .memory()
        .arena(efterscript_vm::Space::Local)
        .slot_count()
        + s.interp
            .memory()
            .arena(efterscript_vm::Space::Global)
            .slot_count();
    for _ in 0..100 {
        let (outcome, _, _) = s.job(job);
        assert_eq!(outcome, Outcome::Ok);
    }
    let after_hundred = s
        .interp
        .memory()
        .arena(efterscript_vm::Space::Local)
        .slot_count()
        + s.interp
            .memory()
            .arena(efterscript_vm::Space::Global)
            .slot_count();
    assert_eq!(after_first, after_hundred);
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Printer sessions: one interpreter serving a sequence of jobs, each
//! isolated unless `exitserver` or `startjob` made it permanent, one at
//! a time, with the prelude run once and an abandoned job reverted.

mod support;

use platen::{JobConfig, JobError, Options, Outcome, Printer};
use support::{check, kids};

fn printer() -> Printer {
    Printer::new(JobConfig {
        options: Options::compress(false),
        ..JobConfig::default()
    })
    .expect("the printer builds")
}

/// Runs `program` as one job on `printer`: its replies and whether it
/// made itself permanent.
fn run(printer: &Printer, program: &str) -> (Vec<u8>, platen::Finished) {
    let mut job = printer.job().expect("the printer is idle");
    let mut replies = job.feed(program.as_bytes()).expect("accepted").replies;
    replies.push(b'|');
    let finished = job.finish().expect("the job finishes");
    replies.extend(&finished.replies);
    (replies, finished)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn a_download_persists_across_jobs() {
    let printer = printer();
    let (replies, first) = run(&printer, "serverdict begin 0 exitserver /x 1 def\n");
    assert_eq!(
        text(&replies),
        "%%[exitserver: permanent state may be changed]%%\n|"
    );
    assert!(first.permanent);
    let (replies, second) = run(&printer, "x =\n");
    assert_eq!(text(&replies), "1\n|");
    assert!(!second.permanent);
    assert_eq!(second.outcome, Outcome::Ok);
}

#[test]
fn encapsulated_jobs_are_isolated() {
    let printer = printer();
    run(&printer, "/x 1 def\n");
    let (_, second) = run(&printer, "x\n");
    assert_eq!(
        second.outcome,
        Outcome::Error {
            name: "undefined".to_string(),
            offending: "x".to_string(),
        }
    );
}

#[test]
fn a_persisted_font_is_embedded_in_a_later_document() {
    let corpus = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../corpus/unit/fonts/embedded-font-dump.ps"
    ))
    .unwrap();
    let definition = &corpus[..corpus.find("/Syn findfont").unwrap()];
    let printer = printer();
    let (_, first) = run(
        &printer,
        &format!("serverdict begin 0 exitserver\n{definition}"),
    );
    assert_eq!(first.outcome, Outcome::Ok);
    assert!(first.permanent);
    let (_, second) = run(
        &printer,
        "/Syn findfont 12 scalefont setfont 72 700 moveto (a) show showpage\n",
    );
    assert_eq!(second.outcome, Outcome::Ok);
    assert_eq!(kids(&check(&second.pdf)).len(), 1);
    let pdf = text(&second.pdf);
    assert!(pdf.contains("/FontFile"), "the program is embedded");
    assert!(pdf.contains("+Syn"), "as a subset");
}

#[test]
fn one_job_at_a_time() {
    let printer = printer();
    let mut open = printer.job().unwrap();
    assert!(matches!(printer.job(), Err(JobError::Busy)));
    let progress = open.feed(b"(still here) =\n").unwrap();
    assert_eq!(progress.replies, b"still here\n");
    drop(open);
    assert!(printer.job().is_ok());
}

#[test]
fn the_prelude_runs_once() {
    let printer = Printer::new(JobConfig {
        prelude: Some(
            b"userdict /runs known { /runs runs 1 add def } { /runs 1 def } ifelse".to_vec(),
        ),
        ..JobConfig::default()
    })
    .unwrap();
    for _ in 0..3 {
        let (replies, _) = run(&printer, "runs =\n");
        assert_eq!(text(&replies), "1\n|");
    }
}

#[test]
fn a_connection_lost_mid_job() {
    let printer = printer();
    let mut job = printer.job().unwrap();
    job.feed(b"/x 1 def 0 0 10 10 rectfill showpage /y ")
        .unwrap();
    drop(job);
    let (replies, _) = run(&printer, "/x where = /y where =\n");
    assert_eq!(text(&replies), "false\nfalse\n|");
}

#[test]
fn a_permanent_change_survives_abandonment() {
    let printer = printer();
    let mut job = printer.job().unwrap();
    job.feed(b"serverdict begin 0 exitserver /kept 1 def ")
        .unwrap();
    drop(job);
    let (replies, _) = run(&printer, "kept =\n");
    assert_eq!(text(&replies), "1\n|");
}

#[test]
fn page_count_spans_jobs() {
    let printer = printer();
    run(&printer, "showpage\n");
    run(&printer, "showpage\n");
    let (replies, _) = run(&printer, "currentsystemparams /PageCount get =\n");
    assert_eq!(text(&replies), "2\n|");
}

#[test]
fn the_budget_applies_to_each_job() {
    let printer = Printer::new(JobConfig {
        step_budget: Some(100_000),
        ..JobConfig::default()
    })
    .unwrap();
    for _ in 0..3 {
        let (_, finished) = run(&printer, "0 1 20000 { pop } for\n");
        assert_eq!(finished.outcome, Outcome::Ok);
    }
}

#[test]
fn the_printer_may_go_before_its_job() {
    let printer = printer();
    let mut job = printer.job().unwrap();
    drop(printer);
    job.feed(b"(alone) =\n").unwrap();
    let finished = job.finish().unwrap();
    assert_eq!(finished.outcome, Outcome::Ok);
}

#[test]
fn drawing_continues_after_exitserver_in_the_same_job() {
    let printer = printer();
    let (_, finished) = run(
        &printer,
        "0 0 10 10 rectfill serverdict begin 0 exitserver \
         72 72 moveto 144 144 lineto stroke showpage\n",
    );
    assert_eq!(finished.outcome, Outcome::Ok);
    assert_eq!(kids(&check(&finished.pdf)).len(), 1);
}

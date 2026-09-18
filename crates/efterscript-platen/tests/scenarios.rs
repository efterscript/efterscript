// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The job API's scenarios: a query answered before end of data with the
//! program split inside a token, a page job, an error on the error
//! channel, no state between jobs, and the edges around them. A token
//! ends at a delimiter, so a piece meant to complete a command ends
//! with a newline.

mod support;

use platen::{Finished, Job, JobConfig, JobError, Outcome, Progress};
use support::{check, content, kids};

fn job(config: JobConfig) -> Job {
    Job::new(config).expect("the job builds")
}

fn feed(job: &mut Job, bytes: &str) -> Progress {
    job.feed(bytes.as_bytes()).expect("the job accepts bytes")
}

fn identity(product: &str) -> JobConfig {
    JobConfig {
        identity: vec![("product".to_string(), format!("({product})"))],
        ..JobConfig::default()
    }
}

#[test]
fn a_query_is_answered_before_end_of_data() {
    let mut job = job(identity("Fictional Press"));
    let first = feed(&mut job, "statusdict /pro");
    assert_eq!(first, Progress::default());
    let second = feed(&mut job, "duct get");
    assert_eq!(second, Progress::default());
    let third = feed(&mut job, " = flush");
    assert_eq!(third.replies, b"Fictional Press\n");
    assert!(third.errors.is_empty());
    assert!(!third.done);
    let finished = job.finish().unwrap();
    assert_eq!(finished.outcome, Outcome::Ok);
    assert!(finished.replies.is_empty());
    assert!(finished.errors.is_empty());
    assert_eq!(finished.report.pages, 0);
}

#[test]
fn a_page_job_leaves_a_document_with_the_fill() {
    let mut job = job(JobConfig::default());
    let progress = feed(&mut job, "0.5 setgray 100 100 200 300 rectfill showpage\n");
    assert!(!progress.done);
    assert_eq!(job.pages(), 1);
    let Finished {
        outcome,
        pdf,
        report,
        ..
    } = job.finish().unwrap();
    assert_eq!(outcome, Outcome::Ok);
    assert_eq!(report.pages, 1);
    let document = check(&pdf);
    assert_eq!(kids(&document).len(), 1);
    let stream = content(&document, 0);
    assert!(stream.contains("0.5 g\n"), "{stream}");
    assert!(
        stream.contains("100 100 m\n300 100 l\n300 400 l\n100 400 l\nh\nf\n"),
        "{stream}"
    );
}

#[test]
fn an_error_is_reported_on_the_error_channel() {
    let mut job = job(JobConfig::default());
    let progress = feed(&mut job, "1 0 div\n");
    assert!(progress.done);
    assert!(progress.replies.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&progress.errors),
        "%%[ Error: undefinedresult; OffendingCommand: div ]%%\n"
    );
    assert!(matches!(job.feed(b"1 =").unwrap_err(), JobError::Finished));
    let finished = job.finish().unwrap();
    assert_eq!(
        finished.outcome,
        Outcome::Error {
            name: "undefinedresult".to_string(),
            offending: "div".to_string(),
        }
    );
    assert!(finished.errors.is_empty(), "already drained");
}

#[test]
fn an_error_at_the_end_of_data_is_reported_by_finish() {
    let mut job = job(JobConfig::default());
    let progress = feed(&mut job, "(unterminated");
    assert!(!progress.done);
    let finished = job.finish().unwrap();
    assert_eq!(
        finished.outcome,
        Outcome::Error {
            name: "syntaxerror".to_string(),
            offending: "--nostringval--".to_string(),
        }
    );
    assert!(
        String::from_utf8_lossy(&finished.errors).starts_with("%%[ Error: syntaxerror;"),
        "{:?}",
        finished.errors
    );
}

#[test]
fn nothing_persists_between_jobs() {
    let config = identity("Fictional Press");
    let mut first = job(config.clone());
    let progress = feed(&mut first, "serverdict begin 0 exitserver /x 1 def x =\n");
    assert_eq!(progress.replies, b"1\n");
    assert_eq!(first.finish().unwrap().outcome, Outcome::Ok);
    let mut second = job(config);
    let progress = feed(&mut second, "x\n");
    assert!(progress.done);
    assert_eq!(
        second.finish().unwrap().outcome,
        Outcome::Error {
            name: "undefined".to_string(),
            offending: "x".to_string(),
        }
    );
}

#[test]
fn the_budget_is_its_own_outcome() {
    let mut job = job(JobConfig {
        step_budget: Some(1000),
        ..JobConfig::default()
    });
    let progress = feed(&mut job, "{ } loop\n");
    assert!(progress.done);
    assert!(
        String::from_utf8_lossy(&progress.errors).contains("limitcheck"),
        "{:?}",
        progress.errors
    );
    assert_eq!(job.finish().unwrap().outcome, Outcome::Budget);

    let mut job = Job::new(JobConfig {
        step_budget: Some(1000),
        ..JobConfig::default()
    })
    .unwrap();
    feed(&mut job, "[ 1 2 3 4 5 ] 0 get pop 4 array 5 get");
    let finished = job.finish().unwrap();
    assert_eq!(
        finished.outcome,
        Outcome::Error {
            name: "rangecheck".to_string(),
            offending: "get".to_string(),
        },
        "a program's own limit is not the budget's"
    );
}

#[test]
fn the_prelude_and_the_identity_shape_the_device() {
    let config = JobConfig {
        identity: vec![
            ("product".to_string(), "(Fictional Press)".to_string()),
            ("version".to_string(), "(47.0)".to_string()),
            ("manualfeed".to_string(), "false".to_string()),
            ("sizes".to_string(), "[612 792]".to_string()),
        ],
        prelude: Some(b"statusdict begin /waittimeout 300 def end (banner) =".to_vec()),
        server_password: 7,
        ..JobConfig::default()
    };
    let mut job = job(config);
    let progress = feed(
        &mut job,
        "statusdict begin product = version cvr = manualfeed = sizes 1 get = waittimeout = end \
         serverdict begin 7 exitserver (in) =\n",
    );
    assert_eq!(
        String::from_utf8_lossy(&progress.replies),
        "Fictional Press\n47.0\nfalse\n792\n300\nin\n",
        "the prelude's own output is not the job's"
    );
    let finished = job.finish().unwrap();
    assert_eq!(finished.outcome, Outcome::Ok);
    assert!(finished.report.prelude_ran);
    assert!(
        finished
            .report
            .identity
            .iter()
            .any(|(k, v)| k == "waittimeout" && v == "300")
    );
}

#[test]
fn a_bad_identity_value_and_a_failing_prelude_refuse_the_job() {
    let bad = Job::new(JobConfig {
        identity: vec![("product".to_string(), "1 2".to_string())],
        ..JobConfig::default()
    });
    match bad {
        Err(JobError::Identity { key, detail }) => {
            assert_eq!(key, "product");
            assert_eq!(detail, "more than one value");
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
    let failing = Job::new(JobConfig {
        prelude: Some(b"1 0 div".to_vec()),
        ..JobConfig::default()
    });
    match failing {
        Err(JobError::Prelude { name, offending }) => {
            assert_eq!(name, "undefinedresult");
            assert_eq!(offending, "div");
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn an_empty_job_finishes_clean() {
    let finished = job(JobConfig::default()).finish().unwrap();
    assert_eq!(finished.outcome, Outcome::Ok);
    assert_eq!(finished.report.pages, 0);
    assert!(check(&finished.pdf).trailer_get("Root").is_some());
}

#[test]
fn the_writer_options_reach_the_document() {
    let mut job = job(JobConfig {
        options: platen::Options::compress(true),
        ..JobConfig::default()
    });
    feed(&mut job, "0 0 10 10 rectfill showpage");
    let finished = job.finish().unwrap();
    assert!(finished.report.params.compress_pages);
    assert!(finished.pdf.windows(11).any(|w| w == b"FlateDecode"));
}

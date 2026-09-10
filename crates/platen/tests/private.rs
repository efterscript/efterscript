// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The private tier: a captured driver job, fed to a `Job` in 512-byte
//! pieces with the host prelude, shows one page, ends well, and leaves
//! the same document as the whole-file distillation. The two files are
//! named by `EFTERSCRIPT_PRIVATE_JOB` and `EFTERSCRIPT_PRIVATE_PRELUDE`;
//! without them the test skips. Ignored by default since the material is
//! not in the repository.

use platen::{Job, JobConfig, Outcome};
use ps_vm::{Capture, Config, Io};
use remelt::{Options, PdfSink};

#[test]
#[ignore = "private tier: set EFTERSCRIPT_PRIVATE_JOB and EFTERSCRIPT_PRIVATE_PRELUDE"]
fn the_captured_driver_job_in_pieces() {
    let (Ok(job_path), Ok(prelude_path)) = (
        std::env::var("EFTERSCRIPT_PRIVATE_JOB"),
        std::env::var("EFTERSCRIPT_PRIVATE_PRELUDE"),
    ) else {
        eprintln!(
            "private tier skipped: EFTERSCRIPT_PRIVATE_JOB / EFTERSCRIPT_PRIVATE_PRELUDE unset"
        );
        return;
    };
    let program = std::fs::read(&job_path).expect("the captured job");
    let prelude = std::fs::read(&prelude_path).expect("the host prelude");

    let mut job = Job::new(JobConfig {
        prelude: Some(prelude.clone()),
        ..JobConfig::default()
    })
    .expect("the prelude runs");
    let mut replies = Vec::new();
    let mut errors = Vec::new();
    let mut feeds = 0;
    for piece in program.chunks(512) {
        let progress = job.feed(piece).expect("the job accepts bytes");
        feeds += 1;
        replies.extend(progress.replies);
        errors.extend(progress.errors);
        if progress.done {
            break;
        }
    }
    let finished = job.finish().unwrap();
    replies.extend(finished.replies);
    errors.extend(finished.errors);
    eprintln!(
        "private: {feeds} feeds, {} reply bytes, {} error bytes, {} pages, outcome {:?}",
        replies.len(),
        errors.len(),
        finished.report.pages,
        finished.outcome
    );
    assert_eq!(finished.outcome, Outcome::Ok);
    assert_eq!(finished.report.pages, 1);

    let (io, out, err) = Io::capture();
    let config = Config {
        io: io.with_stdin(Capture::new()),
        prelude: Some(prelude),
        ..Config::default()
    };
    let sink = PdfSink::new(Vec::new(), Options::default()).unwrap();
    let (report, pdf) = remelt::distill_into(&program, config, sink).unwrap();
    assert_eq!(report.outcome, ps_vm::Outcome::Ok);
    assert_eq!(report.pages, 1);
    assert_eq!(replies, out.bytes(), "replies");
    assert_eq!(errors, err.bytes(), "errors");
    assert!(
        finished.pdf == pdf,
        "the documents differ: {} vs {} bytes",
        finished.pdf.len(),
        pdf.len()
    );
}

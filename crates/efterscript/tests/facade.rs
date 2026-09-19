// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Depending on the facade alone suffices: a corpus file distilled
//! through it equals its golden byte for byte, and a session job runs a
//! program to a page.

use std::io::Cursor;

use efterscript::distill::PdfSink;
use efterscript::session::Outcome;
use efterscript::{Config, Job, JobConfig, Options};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// The comment lines every PDF golden carries after the header; the
/// corpus runner writes them the same way.
const PROVENANCE: [&str; 3] = [
    "SPDX-FileCopyrightText: 2026 EfterScript contributors",
    "SPDX-License-Identifier: MIT",
    "GENERATED-BY: difftest --update-pdf",
];

#[test]
fn a_corpus_file_distilled_through_the_facade_equals_its_golden() {
    let program = std::fs::read(format!("{ROOT}/corpus/unit/graphics/gray-image.ps")).unwrap();
    let golden =
        std::fs::read(format!("{ROOT}/corpus/golden/pdf/graphics/gray-image.pdf")).unwrap();
    let options = Options::compress(false)
        .lock("CompressPages")
        .unversioned_producer();
    let mut sink = PdfSink::new_seekable(Cursor::new(Vec::new()), options).unwrap();
    for line in PROVENANCE {
        sink.comment(line).unwrap();
    }
    let (report, out) = efterscript::distill_into(&program, Config::default(), sink).unwrap();
    assert_eq!(report.pages, 1);
    let pdf = out.into_inner();
    assert!(
        pdf == golden,
        "the facade's document differs from the golden:\n{}",
        String::from_utf8_lossy(&pdf)
    );
}

#[test]
fn a_session_job_runs_a_program_to_one_page() {
    let mut job = Job::new(JobConfig::default()).unwrap();
    let progress = job
        .feed(b"0.5 setgray 100 100 200 300 rectfill showpage\n")
        .unwrap();
    assert!(!progress.done);
    assert!(progress.replies.is_empty());
    assert!(progress.errors.is_empty());
    assert_eq!(job.pages(), 1);
    let finished = job.finish().unwrap();
    assert_eq!(finished.outcome, Outcome::Ok);
    assert_eq!(finished.report.pages, 1);
    assert!(finished.pdf.starts_with(b"%PDF-"));
}

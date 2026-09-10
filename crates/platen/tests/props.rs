// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A job fed in pieces agrees with the unsplit distillation: for a
//! sample of the corpus programs, random cut points produce the same
//! outcome, replies, error output, and document bytes as
//! `remelt::distill_into` on the whole program. The corpus's own
//! expectations are not checked here — `difftest run` does that — only
//! the split's transparency.

use std::path::PathBuf;

use platen::{Job, JobConfig, Outcome};
use proptest::prelude::*;
use ps_vm::{Capture, Config, Io};
use remelt::{Options, PdfSink};

/// The programs sampled: every interpreter corpus file, which is where
/// `currentfile` reads, error reports, and output live.
fn programs() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus/unit/interp");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .expect("the corpus is in the tree")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "ps"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).unwrap())
        })
        .collect()
}

struct Run {
    outcome: Outcome,
    replies: Vec<u8>,
    errors: Vec<u8>,
    pdf: Vec<u8>,
}

fn whole(program: &[u8]) -> Run {
    let (io, out, err) = Io::capture();
    let config = Config {
        io: io.with_stdin(Capture::new()),
        ..Config::default()
    };
    let sink = PdfSink::new(Vec::new(), Options::default()).unwrap();
    let (report, pdf) = remelt::distill_into(program, config, sink).unwrap();
    let outcome = match report.outcome {
        ps_vm::Outcome::Ok => Outcome::Ok,
        ps_vm::Outcome::Error(s) => Outcome::Error {
            name: s.name,
            offending: s.command,
        },
        ps_vm::Outcome::Suspended => unreachable!("a slice never suspends"),
    };
    Run {
        outcome,
        replies: out.bytes(),
        errors: err.bytes(),
        pdf,
    }
}

fn split(program: &[u8], cuts: &[usize]) -> Run {
    let mut job = Job::new(JobConfig::default()).unwrap();
    let mut replies = Vec::new();
    let mut errors = Vec::new();
    let mut at = 0;
    for &cut in cuts.iter().chain(std::iter::once(&program.len())) {
        let cut = cut.clamp(at, program.len());
        let Ok(progress) = job.feed(&program[at..cut]) else {
            break;
        };
        replies.extend(progress.replies);
        errors.extend(progress.errors);
        at = cut;
        if progress.done {
            break;
        }
    }
    let finished = job.finish().unwrap();
    replies.extend(finished.replies);
    errors.extend(finished.errors);
    Run {
        outcome: finished.outcome,
        replies,
        errors,
        pdf: finished.pdf,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn a_split_job_agrees_with_the_whole(
        index in 0usize..1000,
        raw_cuts in prop::collection::vec(0usize..4000, 0..6),
    ) {
        let programs = programs();
        let (name, program) = &programs[index % programs.len()];
        let mut cuts: Vec<usize> = raw_cuts.iter().map(|c| c % (program.len() + 1)).collect();
        cuts.sort_unstable();
        let expected = whole(program);
        let actual = split(program, &cuts);
        prop_assert_eq!(&actual.outcome, &expected.outcome, "{} cut at {:?}", name, cuts);
        prop_assert_eq!(&actual.replies, &expected.replies, "{} cut at {:?}", name, cuts);
        prop_assert_eq!(&actual.errors, &expected.errors, "{} cut at {:?}", name, cuts);
        prop_assert_eq!(&actual.pdf, &expected.pdf, "{} cut at {:?}", name, cuts);
    }
}

/// Every program cut into two pieces at a stride of bytes chosen so the
/// sample stays quick, the stride offset varying per program so the
/// cuts land inside tokens as well as between them.
#[test]
fn two_piece_splits_agree() {
    let programs = programs();
    let total: usize = programs.iter().map(|(_, p)| p.len()).sum();
    let stride = (total / 400).max(1);
    for (index, (name, program)) in programs.iter().enumerate() {
        let expected = whole(program);
        for cut in (index % stride..=program.len()).step_by(stride) {
            let actual = split(program, &[cut]);
            assert_eq!(actual.outcome, expected.outcome, "{name} cut at {cut}");
            assert_eq!(actual.replies, expected.replies, "{name} cut at {cut}");
            assert_eq!(actual.errors, expected.errors, "{name} cut at {cut}");
            assert_eq!(actual.pdf, expected.pdf, "{name} cut at {cut}");
        }
    }
}

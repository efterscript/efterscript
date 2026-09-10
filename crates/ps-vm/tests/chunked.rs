// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A job that arrives in pieces behaves as one that arrived whole: the
//! scanner suspends mid-token, and an operator whose read of the job's
//! source finds no byte with more to come is undone and run again once
//! bytes have arrived — `readline`, `readstring`, `readhexstring`,
//! `read`, `token`, `eexec` in both forms, image data from
//! `currentfile`, and `closefile` on it. Every program is split at every
//! point and compared with its unsplit run: outcome, output, error
//! output, and the backend's log.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::Recording;
use ps_fonts::testing::{TRAILER, eexec_binary, eexec_hex};
use ps_vm::{ChunkSource, Config, Interp, Io, Limits, Outcome, SliceSource};

struct Run {
    outcome: Outcome,
    out: String,
    err: String,
    log: String,
}

fn interp(steps: Option<u64>) -> (Interp, ps_vm::Capture, ps_vm::Capture, common::Log) {
    let (io, out, err) = Io::capture();
    let config = Config {
        io,
        limits: Limits {
            steps,
            ..Limits::default()
        },
        ..Config::default()
    };
    let mut interp = Interp::with_config(config);
    let log = Rc::new(RefCell::new(Vec::new()));
    interp.set_graphics_backend(Box::new(Recording::new(log.clone())));
    (interp, out, err, log)
}

fn whole(program: &[u8], steps: Option<u64>) -> Run {
    let (mut interp, out, err, log) = interp(steps);
    let outcome = interp.run(&mut SliceSource::new(program));
    Run {
        outcome,
        out: out.text(),
        err: err.text(),
        log: format!("{:?}", log.borrow()),
    }
}

/// Feeds `program` in pieces of the given lengths, the last piece
/// followed by end of data; asserts that no piece but the last completes
/// the job unless it really is complete.
fn pieces(program: &[u8], cuts: &[usize], steps: Option<u64>) -> Run {
    let (mut interp, out, err, log) = interp(steps);
    let mut source = ChunkSource::new();
    let mut outcome = None;
    let mut at = 0;
    for &cut in cuts {
        source.append(&program[at..cut]);
        at = cut;
        let step = match outcome {
            None => interp.run(&mut source),
            Some(Outcome::Suspended) => interp.resume(&mut source),
            Some(_) => break,
        };
        outcome = Some(step);
    }
    if !matches!(outcome, Some(Outcome::Ok | Outcome::Error(_))) {
        source.append(&program[at..]);
        source.finish();
        let step = match outcome {
            None => interp.run(&mut source),
            _ => interp.resume(&mut source),
        };
        outcome = Some(step);
    }
    Run {
        outcome: outcome.expect("ran"),
        out: out.text(),
        err: err.text(),
        log: format!("{:?}", log.borrow()),
    }
}

/// Every split into two pieces agrees with the whole; a handful of
/// three-piece splits too.
fn agrees_at_every_split(program: &[u8]) -> Run {
    let expected = whole(program, None);
    for cut in 0..=program.len() {
        let split = pieces(program, &[cut], None);
        assert_eq!(split.outcome, expected.outcome, "cut at {cut}");
        assert_eq!(split.out, expected.out, "cut at {cut}");
        assert_eq!(split.err, expected.err, "cut at {cut}");
        assert_eq!(split.log, expected.log, "cut at {cut}");
    }
    for cut in (0..program.len()).step_by(7) {
        let split = pieces(program, &[cut, (cut + 3).min(program.len())], None);
        assert_eq!(split.outcome, expected.outcome, "cuts at {cut}");
        assert_eq!(split.out, expected.out, "cuts at {cut}");
    }
    expected
}

#[test]
fn readline_readstring_and_read() {
    let run = agrees_at_every_split(
        b"currentfile 20 string readline first line here\npop =\n\
          currentfile 6 string readstring abcdef pop = currentfile read Z pop =\n",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "first line here\nabcdef\n90\n");
    let run = agrees_at_every_split(b"currentfile 4 string readstring ab");
    assert_eq!(run.outcome, Outcome::Ok);
}

#[test]
fn readhexstring_and_token() {
    let run = agrees_at_every_split(
        b"currentfile 3 string readhexstring 41 4\n2 43 pop { = } forall \
          currentfile token /name pop == currentfile token (str) pop == \
          currentfile token {1 2} pop == currentfile token 12.5 = =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "65\n66\n67\n/name\n(str)\n{1 2}\ntrue\n12.5\n");
}

#[test]
fn eexec_sections() {
    let mut program = b"currentfile eexec\n".to_vec();
    program.extend_from_slice(
        eexec_hex(b"userdict /y 7 put y = mark currentfile closefile\n").as_bytes(),
    );
    program.extend_from_slice(TRAILER.as_bytes());
    program.extend_from_slice(b"y 1 add =\ncurrentfile eexec\n");
    program.extend_from_slice(&eexec_binary(
        b"userdict /z 9 put z = currentfile 3 string readstring xyz pop = mark currentfile closefile\n",
    ));
    program.extend_from_slice(TRAILER.as_bytes());
    program.extend_from_slice(b"z 1 add =\n");
    let run = agrees_at_every_split(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "7\n8\n9\nxyz\n10\n");
}

#[test]
fn image_data_from_the_job_source() {
    let run = agrees_at_every_split(
        b"4 2 8 [4 0 0 -2 0 2] currentfile image 01234567 currentfile 4 string readstring wxyz pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "wxyz\n");
    assert!(run.log.contains("Image"), "{}", run.log);
}

#[test]
fn errors_and_closefile() {
    let run = agrees_at_every_split(b"1 0 div currentfile 3 string readstring abc pop =");
    assert!(run.err.contains("undefinedresult"), "{}", run.err);
    let run = agrees_at_every_split(b"1 = currentfile closefile 2 = 3 =");
    assert_eq!(run.out, "1\n");
    let run = agrees_at_every_split(b"currentfile 5 string readline no newline at the end");
    assert!(run.err.contains("rangecheck"), "{}", run.err);
    let run = agrees_at_every_split(b"currentfile 3 string readline ab");
    assert_eq!(run.outcome, Outcome::Ok);
    agrees_at_every_split(b"(open string");
}

/// The budget sees one execution of an operator that had to wait: a
/// budget just above the unsplit count passes every split, one just
/// below fails every split.
#[test]
fn a_re_run_operator_is_charged_once() {
    let program = b"0 1 100 { pop } for currentfile 3 string readstring abc pop = 1 2 add =";
    let (mut probe, _, _, _) = interp(Some(1_000_000));
    assert_eq!(probe.run(&mut SliceSource::new(program)), Outcome::Ok);
    let steps = probe.steps();
    for (budget, ok) in [(steps, true), (steps - 1, false)] {
        let expected = whole(program, Some(budget));
        assert_eq!(matches!(expected.outcome, Outcome::Ok), ok);
        for cut in 0..=program.len() {
            let split = pieces(program, &[cut], Some(budget));
            assert_eq!(split.outcome, expected.outcome, "cut at {cut}");
            assert_eq!(split.out, expected.out, "cut at {cut}");
        }
    }
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A job that arrives in pieces behaves as one that arrived whole: the
//! scanner suspends mid-token, and an operator whose read of the job's
//! source finds no byte with more to come is undone and run again once
//! bytes have arrived — `readline`, `readstring`, `readhexstring`,
//! `read`, `token`, `eexec` in both forms, image data from
//! `currentfile`, `closefile` on it, a filter chain over it, and a
//! reusable stream read from it. Every program is split at every point
//! and compared with its unsplit run: outcome, output, error output,
//! and the backend's log.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::Recording;
use efterscript_fonts::testing::{TRAILER, eexec_binary, eexec_hex};
use efterscript_vm::{ChunkSource, Config, Interp, Io, Limits, Outcome, SliceSource};

struct Run {
    outcome: Outcome,
    out: String,
    err: String,
    log: String,
}

fn interp(
    steps: Option<u64>,
) -> (
    Interp,
    efterscript_vm::Capture,
    efterscript_vm::Capture,
    common::Log,
) {
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

#[test]
fn filters_over_the_job_source() {
    // A program section wrapped in Flate then base-85, executed through
    // the chain; then image samples through a hexadecimal filter; then a
    // filter read by readstring and closed, the close consuming its
    // marker.
    let mut program = b"currentfile /ASCII85Decode filter /FlateDecode filter cvx exec\n\
                        GQH8a/'?1<8BXRU8_$eE<<*\"Ie-)r~>\n(after) =\n\
                        4 2 8 [4 0 0 -2 0 2] currentfile /ASCIIHexDecode filter image\n\
                        30313233 34353637>\n(next) =\n\
                        /readit { currentfile /ASCII85Decode filter dup 5 string readstring pop = closefile } def\n\
                        readit\nBOu!rDZ~>\n(last) ="
        .to_vec();
    let run = agrees_at_every_split(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "from flate\nafter\nnext\nhello\nlast\n");
    assert!(run.log.contains("Image"), "{}", run.log);
    // A filter over a procedure that itself reads the job's source, four
    // bytes at a time: the data follows the outer read, and the `>` the
    // second delivery brings ends the filter.
    program.clear();
    program.extend_from_slice(
        b"{ currentfile 4 string readstring pop } /ASCIIHexDecode filter \
          3 string readstring 414243> pop = (tail) =",
    );
    let run = agrees_at_every_split(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "ABC\ntail\n");
}

#[test]
fn a_reusable_stream_from_the_job_source() {
    // The whole encoded section is read when the stream is made, through
    // the hexadecimal pre-filter to its marker, so the scanner resumes
    // after it; the stream is then read twice and used as image data.
    let run = agrees_at_every_split(
        b"currentfile << /Filter /ASCIIHexDecode >> /ReusableStreamDecode filter\n\
          48656C6C 6F2C2073 747265616D>\n\
          /s exch def s 20 string readstring pop = s resetfile s 5 string readstring pop = \
          s 0 setfileposition 4 2 8 [4 0 0 -2 0 2] s image s bytesavailable = (after) =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "Hello, stream\nHello\n5\nafter\n");
    assert!(run.log.contains("Image"), "{}", run.log);
    // Without a pre-filter the stream takes the rest of the job, the
    // code after the operator included, so nothing more runs.
    let run = agrees_at_every_split(
        b"(before) = currentfile /ReusableStreamDecode filter 100 string readstring pop ==\nrest of job",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "before\n");
    // A run-length pre-filter over base-85 over the job: two layers
    // whose state is rolled back and rebuilt at every cut.
    let run = agrees_at_every_split(
        b"currentfile << /Filter [/ASCII85Decode /RunLengthDecode] >> /ReusableStreamDecode filter\n\
          !b#PJrciq~>\n\
          10 string readstring pop = (tail) =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "abcxxx\ntail\n");
}

#[test]
fn a_mesh_read_from_the_job_source() {
    // The shading's data comes through a hexadecimal filter over the
    // job: a read that starves fails shfill with NeedMore, the filter's
    // state is rolled back, and the operator runs again over its
    // operand once bytes arrive; the scanner resumes after the marker.
    let run = agrees_at_every_split(
        b"<< /ShadingType 4 /ColorSpace /DeviceRGB /BitsPerCoordinate 8 /BitsPerComponent 8 \
          /BitsPerFlag 8 /Decode [0 255 0 255 0 1 0 1 0 1] \
          /DataSource currentfile /ASCIIHexDecode filter >> shfill\n\
          00 0000 ff0000  00 6400 00ff00  00 3250 0000ff>\n\
          (after) =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "after\n");
    assert!(run.log.contains("Shade"), "{}", run.log);
    // A type 2 pattern's shading is read at makepattern the same way.
    let run = agrees_at_every_split(
        b"<< /PatternType 2 /Shading << /ShadingType 4 /ColorSpace /DeviceGray \
          /BitsPerCoordinate 8 /BitsPerComponent 8 /BitsPerFlag 8 /Decode [0 255 0 255 0 1] \
          /DataSource currentfile /ASCIIHexDecode filter >> >> matrix makepattern\n\
          00 0000 00  00 6400 80  00 3250 ff>\n\
          setpattern 0 0 10 10 rectfill (done) =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "done\n");
    assert!(run.log.contains("SetPattern"), "{}", run.log);
}

#[test]
fn a_dct_image_over_the_job_source() {
    // The stream's bytes are read from under the DCT filter as far as
    // its end-of-image marker: through a hexadecimal layer whose `>` is
    // then consumed, and straight from the job, where the scanner
    // resumes right after the marker.
    let dict =
        b"<< /ImageType 1 /Width 2 /Height 2 /BitsPerComponent 8 /ImageMatrix [2 0 0 2 0 0] ";
    let mut program = dict.to_vec();
    program.extend_from_slice(
        b"/DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image\n\
          FFD8FFDB0004AABBFFDA0003011234FF00FFD9>\n(hex) =\n",
    );
    program.extend_from_slice(dict);
    program.extend_from_slice(b"/DataSource currentfile /DCTDecode filter >> image\n");
    program.extend_from_slice(&[
        0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x04, 0xAA, 0xBB, 0xFF, 0xDA, 0x00, 0x03, 0x01, 0x12, 0x34,
        0xFF, 0x00, 0xFF, 0xD9,
    ]);
    program.extend_from_slice(b"(raw) =\n");
    let run = agrees_at_every_split(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.out, "hex\nraw\n");
    assert_eq!(run.log.matches("Image(ImageSpec").count(), 2, "{}", run.log);
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

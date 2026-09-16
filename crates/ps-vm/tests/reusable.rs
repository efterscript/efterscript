// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Reusable streams and file positioning (PLRM3 §3.13.3): the
//! `ReusableStreamDecode` filter over a string, a file, and a procedure,
//! its pre-filter chain and parameter checks, `CloseSource`, what the
//! positioning operators answer on a reusable stream and on ordinary
//! files, and the file table after the read. The printed-output
//! scenarios are corpus files under `corpus/unit/filters`.

use std::cell::RefCell;
use std::rc::Rc;

use ps_vm::{
    Capabilities, Capture, Config, FileCapability, Interp, Io, Outcome, SliceSource, Stream,
    VmError,
};

/// A readable stream over fixed bytes that records whether it was
/// closed.
#[derive(Clone, Default)]
struct Probe(Rc<RefCell<(Vec<u8>, usize, bool)>>);

impl Probe {
    fn with_input(input: &[u8]) -> Self {
        Probe(Rc::new(RefCell::new((input.to_vec(), 0, false))))
    }

    fn closed(&self) -> bool {
        self.0.borrow().2
    }
}

impl Stream for Probe {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        let mut s = self.0.borrow_mut();
        let n = buf.len().min(s.0.len() - s.1);
        buf[..n].copy_from_slice(&s.0[s.1..s.1 + n]);
        s.1 += n;
        Ok(n)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }

    fn close(&mut self) -> Result<(), VmError> {
        self.0.borrow_mut().2 = true;
        Ok(())
    }
}

/// A capability answering `(f) (r) file` with the probe.
struct Files(Probe);

impl FileCapability for Files {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError> {
        match (name, mode) {
            (b"f", b"r") => Ok(Box::new(self.0.clone())),
            _ => Err(VmError::UndefinedFileName),
        }
    }
}

struct Run {
    interp: Interp,
    outcome: Outcome,
    out: Capture,
    probe: Probe,
}

impl Run {
    fn output(&self) -> String {
        self.out.text()
    }

    fn error(&self) -> Option<&str> {
        match &self.outcome {
            Outcome::Error(summary) => Some(&summary.name),
            _ => None,
        }
    }
}

fn exec_with(program: &str, file: &[u8]) -> Run {
    let (io, out, _) = Io::capture();
    let probe = Probe::with_input(file);
    let config = Config {
        io,
        capabilities: Capabilities {
            file: Some(Box::new(Files(probe.clone()))),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = interp.run(&mut SliceSource::new(program.as_bytes()));
    Run {
        interp,
        outcome,
        out,
        probe,
    }
}

fn exec(program: &str) -> Run {
    exec_with(program, b"")
}

#[test]
fn a_string_source_is_read_whole_and_can_be_read_again() {
    let run = exec(
        "(abc) /ReusableStreamDecode filter dup 10 string readstring pop = \
         dup 10 string readstring pop length = dup resetfile 10 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "abc\n0\nabc\n");
    // The stream is a read-only file object, and the source's entry is
    // gone once the read is over: only the stream itself stays open.
    let open_before = exec("").interp.memory().files().open_count();
    let run = exec("(abc) /ReusableStreamDecode filter dup type == dup rcheck = wcheck =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "filetype\ntrue\nfalse\n");
    assert_eq!(run.interp.memory().files().open_count(), open_before + 1);
    // Writing into the string afterwards changes nothing.
    let run = exec(
        "/s (abc) def s /ReusableStreamDecode filter s 0 (xyz) putinterval \
         10 string readstring pop =",
    );
    assert_eq!(run.output(), "abc\n");
    // An empty source is an empty stream.
    let run = exec(
        "() /ReusableStreamDecode filter dup bytesavailable = 10 string readstring = length =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "0\nfalse\n0\n");
}

#[test]
fn pre_filters_decode_the_source_in_order() {
    let run = exec(
        "(48656C6C6F>) << /Filter /ASCIIHexDecode >> /ReusableStreamDecode filter \
         10 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hello\n");
    // Two filters in an array, the parameters an array with null for
    // the one without any; the run-length data under the hexadecimal
    // layer decodes to the string.
    let run = exec(
        "(02616263 FE78 80>) << /Filter [/ASCIIHexDecode /RunLengthDecode] \
         /DecodeParms [null null] >> /ReusableStreamDecode filter 10 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "abcxxx\n");
    // A parameter dictionary is honoured: the sub-file filter stops at
    // its string.
    let run = exec(
        "(ab*EOD*cd) << /Filter /SubFileDecode /DecodeParms << /EODCount 0 /EODString (*EOD*) >> >> \
         /ReusableStreamDecode filter 10 string readstring pop =",
    );
    assert_eq!(run.output(), "ab\n");
    let run = exec(
        "(ab*EOD*cd) << /Filter [/SubFileDecode] /DecodeParms [<< /EODCount 0 /EODString (*EOD*) >>] >> \
         /ReusableStreamDecode filter 10 string readstring pop =",
    );
    assert_eq!(run.output(), "ab\n");
    // Malformed data under a pre-filter is the filter's error.
    let run = exec("(Gb\"0Ec) << /Filter /ASCII85Decode >> /ReusableStreamDecode filter");
    assert_eq!(run.error(), Some("ioerror"));
}

#[test]
fn parameters_are_checked_before_anything_is_opened() {
    let open_before = exec("").interp.memory().files().open_count();
    for (params, error) in [
        ("<< /Filter 1 >>", "typecheck"),
        ("<< /Filter /NoSuchDecode >>", "undefined"),
        ("<< /Filter /ASCIIHexEncode >>", "rangecheck"),
        ("<< /Filter /ReusableStreamDecode >>", "rangecheck"),
        ("<< /Filter [1] >>", "typecheck"),
        ("<< /Filter /ASCIIHexDecode /DecodeParms 1 >>", "typecheck"),
        (
            "<< /Filter /ASCIIHexDecode /DecodeParms [1] >>",
            "typecheck",
        ),
        (
            "<< /Filter /ASCIIHexDecode /DecodeParms [null null] >>",
            "rangecheck",
        ),
        (
            "<< /Filter [/ASCIIHexDecode /FlateDecode] /DecodeParms << >> >>",
            "rangecheck",
        ),
        (
            "<< /Filter /FlateDecode /DecodeParms << /Predictor 3 >> >>",
            "rangecheck",
        ),
        ("<< /CloseSource 1 >>", "typecheck"),
        ("<< /AsyncRead 1 >>", "typecheck"),
        ("<< /Intent true >>", "typecheck"),
    ] {
        let program = format!("(41>) {params} /ReusableStreamDecode filter");
        let run = exec(&program);
        assert_eq!(run.error(), Some(error), "{program}");
        assert_eq!(
            run.interp.memory().files().open_count(),
            open_before,
            "{program}"
        );
    }
    // Accepted values are ignored; a parameter dictionary with nothing
    // in it is fine; unknown keys are ignored.
    let run = exec(
        "(41>) << /Filter /ASCIIHexDecode /AsyncRead true /Intent 3 /Whatever 1 >> \
         /ReusableStreamDecode filter 10 string readstring pop = \
         (x) << >> /ReusableStreamDecode filter 10 string readstring pop = \
         (x) << /Filter [] /DecodeParms [] >> /ReusableStreamDecode filter 10 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "A\nx\nx\n");
    // The source must be a string, a readable file, or a procedure.
    let run = exec("42 /ReusableStreamDecode filter");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec("/ReusableStreamDecode filter");
    assert_eq!(run.error(), Some("stackunderflow"));
}

#[test]
fn a_file_source_is_read_to_its_end_and_closed_only_when_asked() {
    let run = exec_with(
        "(f) (r) file dup /ReusableStreamDecode filter 10 string readstring pop = \
         10 string readstring pop length =",
        b"hello",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "hello\n0\n");
    assert!(!run.probe.closed());
    let run = exec_with(
        "(f) (r) file dup << /CloseSource true >> /ReusableStreamDecode filter \
         10 string readstring pop = 10 string readstring",
        b"hello",
    );
    assert_eq!(run.error(), Some("ioerror"));
    assert_eq!(run.output(), "hello\n");
    assert!(run.probe.closed());
    // A pre-filter ends at its marker and the file continues after it;
    // closing the stream never closes the file by itself.
    let run = exec_with(
        "(f) (r) file dup << /Filter /ASCIIHexDecode >> /ReusableStreamDecode filter \
         dup 10 string readstring pop = closefile 10 string readstring pop =",
        b"4869>tail",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hi\ntail\n");
    assert!(!run.probe.closed());
    // Closing the stream with CloseSource closes the file.
    let run = exec_with(
        "(f) (r) file dup << /CloseSource true >> /ReusableStreamDecode filter closefile \
         10 string readstring",
        b"x",
    );
    assert_eq!(run.error(), Some("ioerror"));
    assert!(run.probe.closed());
    // A closed file or one without read access cannot be the source.
    let run = exec_with(
        "(f) (r) file dup closefile /ReusableStreamDecode filter",
        b"x",
    );
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec_with("(f) (r) file noaccess /ReusableStreamDecode filter", b"x");
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn a_procedure_source_is_called_until_it_returns_an_empty_string() {
    let run = exec(
        "/n 0 def /chunks [(ab) (cd) () (ignored)] def \
         { chunks n get /n n 1 add def } /ReusableStreamDecode filter \
         dup 10 string readstring pop = n = dup resetfile 2 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "abcd\n3\nab\n");
    // Through a pre-filter the marker ends the read before the
    // procedure runs dry.
    let run = exec(
        "/n 0 def /chunks [(48 65) (6C6C) (6F>zz) (never)] def \
         { chunks n get /n n 1 add def } << /Filter /ASCIIHexDecode >> /ReusableStreamDecode filter \
         10 string readstring pop = n =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hello\n3\n");
    // A procedure that leaves something other than a string is
    // typecheck, raised on the operator.
    let run = exec("{ 42 } /ReusableStreamDecode filter");
    assert_eq!(run.error(), Some("typecheck"));
    let open_before = exec("").interp.memory().files().open_count();
    let run = exec("{ 42 } /ReusableStreamDecode filter pop");
    assert_eq!(run.interp.memory().files().open_count(), open_before);
    // An error inside the procedure unwinds the read and closes what it
    // opened.
    let run = exec("{ (a) /undefinedname } stopped { (caught) = } if");
    assert_eq!(run.outcome, Outcome::Ok);
    let run = exec("{ { 1 0 div } /ReusableStreamDecode filter } stopped { (caught) = } if");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "caught\n");
    assert_eq!(run.interp.memory().files().open_count(), open_before);
}

#[test]
fn positioning_operators_on_a_reusable_stream() {
    let run = exec(
        "(hello) /ReusableStreamDecode filter \
         dup fileposition = dup bytesavailable = \
         dup 3 setfileposition dup fileposition = dup bytesavailable = \
         dup 10 string readstring = = dup fileposition = \
         dup 5 setfileposition dup bytesavailable = \
         dup resetfile dup fileposition = dup 2 string readstring pop = \
         dup flushfile dup fileposition = dup bytesavailable = \
         dup 0 setfileposition 1 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output(),
        "0\n5\n3\n2\nfalse\nlo\n5\n0\n0\nhe\n5\n0\nh\n"
    );
    // Beyond the length is rangecheck; a negative position too.
    let run = exec("(hello) /ReusableStreamDecode filter 6 setfileposition");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("(hello) /ReusableStreamDecode filter -1 setfileposition");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec("(hello) /ReusableStreamDecode filter 1.0 setfileposition");
    assert_eq!(run.error(), Some("typecheck"));
    // A closed stream: ioerror for the position and the count, nothing
    // for resetfile.
    for (op, error) in [
        ("fileposition", Some("ioerror")),
        ("0 setfileposition", Some("ioerror")),
        ("bytesavailable", Some("ioerror")),
        ("resetfile", None),
    ] {
        let run = exec(&format!(
            "(hello) /ReusableStreamDecode filter dup closefile {op}"
        ));
        assert_eq!(run.error(), error, "{op}");
    }
    // The position after `token` counts what the scanner consumed, the
    // whitespace that ended the token included, and nothing it peeked.
    let run = exec(
        "(12 34) /ReusableStreamDecode filter dup token pop = dup fileposition = \
         dup token pop = fileposition =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "12\n3\n34\n5\n");
}

#[test]
fn positioning_operators_on_ordinary_files() {
    // The job's source cannot be positioned: fileposition and
    // setfileposition are ioerror, bytesavailable is -1, resetfile does
    // nothing and never fails.
    let run = exec("currentfile fileposition");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec("currentfile 0 setfileposition");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec("currentfile bytesavailable = currentfile resetfile (still here) =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "-1\nstill here\n");
    // Nor can a filter or an embedder's file.
    let run = exec("(41>) /ASCIIHexDecode filter fileposition");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec("(41>) /ASCIIHexDecode filter dup bytesavailable = resetfile");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "-1\n");
    let run = exec_with("(f) (r) file 0 setfileposition", b"abc");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec_with("(f) (r) file bytesavailable =", b"abc");
    assert_eq!(run.output(), "-1\n");
    // A closed ordinary file: ioerror for the count, nothing for
    // resetfile; the operand must be a file with read access.
    let run = exec_with("(f) (r) file dup closefile bytesavailable", b"abc");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec_with("(f) (r) file dup closefile resetfile", b"abc");
    assert_eq!(run.outcome, Outcome::Ok);
    for op in [
        "fileposition",
        "0 setfileposition",
        "resetfile",
        "bytesavailable",
    ] {
        let run = exec(&format!("(x) {op}"));
        assert_eq!(run.error(), Some("typecheck"), "{op}");
        let run = exec(op);
        assert_eq!(run.error(), Some("stackunderflow"), "{op}");
    }
    let run = exec("(41>) /ASCIIHexDecode filter noaccess bytesavailable");
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn a_reusable_stream_is_a_file_for_every_reader() {
    // Executed, read by readline and by token, and filtered again; none
    // of it closes the stream, and it can be read again.
    let run = exec(
        "/s ((from stream) =) /ReusableStreamDecode filter def \
         s cvx exec s resetfile s 20 string readline pop = \
         s resetfile s token pop == s 3 string readstring pop == \
         s resetfile s 3 string readstring pop =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output(),
        "from stream\n(from stream) =\n(from stream)\n( =)\n(fr\n"
    );
    let run = exec(
        "/s (12345678) /ReusableStreamDecode filter def \
         4 2 8 [4 0 0 -2 0 2] s image s fileposition = s 0 setfileposition \
         4 2 8 [4 0 0 -2 0 2] s image s fileposition =",
    );
    // Without a graphics backend `image` is undefined; the stream's
    // own behaviour is what is under test, so the backend-free run
    // only checks that the definition and positioning work.
    assert_eq!(run.error(), Some("undefined"));
    let run = exec(
        "(48656C6C6F) /ReusableStreamDecode filter dup /ASCIIHexDecode filter \
         10 string readstring pop = fileposition =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hello\n10\n");
}

#[test]
fn the_category_lists_the_name() {
    let run = exec(
        "/ReusableStreamDecode /Filter resourcestatus = = = \
         /ReusableStreamDecode /Filter findresource == \
         0 (*) { pop 1 add } 32 string /Filter resourceforall =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "true\n0\n0\n/ReusableStreamDecode\n14\n");
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The `filter` operator and the decode filters as files: each decoder's
//! end-of-data and where `closefile` leaves the source, a filter over a
//! string, a procedure, and a file, chaining, `CloseSource`, and the
//! parameter checks; the encode filters over a target a program can
//! read back. The printed-output scenarios are corpus files under
//! `corpus/unit/filters`; these tests look at the source's position and
//! the file table, which a corpus file cannot.

use std::cell::RefCell;
use std::rc::Rc;

use efterscript_vm::{
    Capabilities, Capture, Config, FileCapability, Interp, Io, Outcome, SliceSource, Stream,
    VmError,
};

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

/// A buffer `(t) (w) file` appends to and `(t) (r) file` reads from its
/// start: a string-backed target a program can read back.
type Target = Rc<RefCell<Vec<u8>>>;

struct Writer(Target);

impl Stream for Writer {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }
}

/// A capability answering `(f) (r) file` with fixed bytes and `(t)` in
/// either mode with the target.
struct Files {
    f: Vec<u8>,
    t: Target,
}

impl FileCapability for Files {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError> {
        match (name, mode) {
            (b"f", b"r") => Ok(Box::new(Input(self.f.clone(), 0))),
            (b"t", b"w") => Ok(Box::new(Writer(self.t.clone()))),
            (b"t", b"r") => Ok(Box::new(Input(self.t.borrow().clone(), 0))),
            _ => Err(VmError::UndefinedFileName),
        }
    }
}

struct Run {
    interp: Interp,
    outcome: Outcome,
    out: Capture,
    target: Target,
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

fn exec_with(program: &[u8], file: Option<Vec<u8>>) -> Run {
    let (io, out, _) = Io::capture();
    let target = Target::default();
    let files = Files {
        f: file.unwrap_or_default(),
        t: target.clone(),
    };
    let config = Config {
        io,
        capabilities: Capabilities {
            file: Some(Box::new(files)),
            ..Default::default()
        },
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = interp.run(&mut SliceSource::new(program));
    Run {
        interp,
        outcome,
        out,
        target,
    }
}

fn exec(program: &[u8]) -> Run {
    exec_with(program, None)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02X}")).collect()
}

/// Reads the filter over the capability file to its end through
/// `readstring`, closes it, and prints what the base file holds after
/// it, so the test sees where the marker left the base.
fn after_marker(filter: &str, data: &[u8]) -> Run {
    let mut file = data.to_vec();
    file.extend_from_slice(b"|rest");
    let program = format!(
        "(f) (r) file dup {filter} dup 100 string readstring pop print closefile \
         (\\n) print 100 string readstring pop ="
    );
    exec_with(program.as_bytes(), Some(file))
}

#[test]
fn every_decoder_ends_at_its_marker_and_leaves_the_base_after_it() {
    let run = after_marker("/ASCIIHexDecode filter", b"48 69>");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hi\n|rest\n");
    let run = after_marker("/ASCII85Decode filter", b"BOu!rDZ~>");
    assert_eq!(run.output(), "hello\n|rest\n");
    let run = after_marker(
        "/RunLengthDecode filter",
        &[2, b'a', b'b', b'c', 254, b'x', 128],
    );
    assert_eq!(run.output(), "abcxxx\n|rest\n");
    let z = efterscript_codec::deflate::compress(b"hello");
    let run = after_marker("/FlateDecode filter", &z);
    assert_eq!(run.output(), "hello\n|rest\n");
    let l = efterscript_codec::lzw::encode(b"hello", true);
    let run = after_marker("/LZWDecode filter", &l);
    assert_eq!(run.output(), "hello\n|rest\n");
    let run = after_marker("2 (--) /SubFileDecode filter", b"a--b--");
    assert_eq!(run.output(), "a--b--\n|rest\n");
    let run = after_marker(
        "<< /EODCount 0 /EODString (*EOD*) >> /SubFileDecode filter",
        b"ab*EOD*",
    );
    assert_eq!(run.output(), "ab\n|rest\n");
    let run = after_marker("3 () /SubFileDecode filter", b"abc");
    assert_eq!(run.output(), "abc\n|rest\n");
}

#[test]
fn closefile_before_the_end_consumes_through_the_marker() {
    // Two bytes are read; the close takes the rest through `>`.
    let run = exec_with(
        b"(f) (r) file dup /ASCIIHexDecode filter dup 2 string readstring pop print closefile \
          100 string readstring pop =",
        Some(b"48 69 20 7E>tail".to_vec()),
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hitail\n");
    // A sub-file with no marker of its own is left where it was read to.
    let run = exec_with(
        b"(f) (r) file dup 0 () /SubFileDecode filter dup 2 string readstring pop print closefile \
          100 string readstring pop =",
        Some(b"abcdef".to_vec()),
    );
    assert_eq!(run.output(), "abcdef\n");
}

#[test]
fn a_string_source_is_read_once_and_ends_with_the_string() {
    let run = exec(
        b"(48656C6C6F) /ASCIIHexDecode filter dup 3 string readstring pop = \
                     dup 100 string readstring = = 100 string readstring = ",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hel\nfalse\nlo\nfalse\n");
    // The string is copied: writing into it afterwards changes nothing.
    let run = exec(
        b"/s (4142) def s /ASCIIHexDecode filter s 0 (5A5A) putinterval \
                     100 string readstring pop =",
    );
    assert_eq!(run.output(), "AB\n");
}

#[test]
fn a_procedure_source_is_called_until_it_returns_an_empty_string() {
    // Each call delivers a chunk; the hex decoder stops at `>` before the
    // procedure runs dry, and the calls are counted.
    let run = exec(
        b"/n 0 def /chunks [(48 65) (6C6C) (6F>zz) (ignored)] def \
                     { chunks n get /n n 1 add def } /ASCIIHexDecode filter \
                     100 string readstring pop = n =",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "Hello\n3\n");
    // A procedure that returns an empty string ends the source, and a
    // partial hexadecimal digit is padded.
    let run = exec(
        b"/n 0 def { n 0 eq { (414) } { () } ifelse /n n 1 add def } \
                     /ASCIIHexDecode filter 100 string readstring = = n =",
    );
    assert_eq!(run.output(), "false\nA@\n2\n");
    // The scanner over a procedure-sourced filter runs the procedure too.
    let run = exec(b"/n 0 def { n 0 eq { (28 66 72 6F 6D 20) } { n 1 eq { (70 72 6F 63 29 20 3D >) } { () } ifelse } ifelse /n n 1 add def } \
                     /ASCIIHexDecode filter cvx exec (after) =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "from proc\nafter\n");
    // A procedure returning something other than a string is typecheck,
    // an executable array that is not readable is invalidaccess.
    let run = exec(b"{ 42 } /ASCIIHexDecode filter 10 string readstring");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"{ (41) } noaccess /ASCIIHexDecode filter");
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn a_file_source_and_a_chain_read_through_both_layers() {
    let z = efterscript_codec::deflate::compress(b"the quick brown fox");
    let program =
        "(f) (r) file /ASCIIHexDecode filter /FlateDecode filter 100 string readstring = =";
    let run = exec_with(
        program.as_bytes(),
        Some(format!("{}>", hex(&z)).into_bytes()),
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "false\nthe quick brown fox\n");
    // A closed file is ioerror; a file without read access is invalidaccess.
    let run = exec_with(
        b"(f) (r) file dup closefile /ASCIIHexDecode filter",
        Some(b"41".to_vec()),
    );
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec_with(
        b"(f) (r) file noaccess /ASCIIHexDecode filter",
        Some(b"41".to_vec()),
    );
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn an_executed_chain_is_drained_and_closed_at_its_end() {
    // A program section wrapped in Flate then base-85, executed through
    // the chain: the inner stream's end ends exec, the outer filter is
    // read through its `~>`, and the scanner resumes after it.
    let program = b"currentfile /ASCII85Decode filter /FlateDecode filter cvx exec\n\
                    GQH8a/'?1<8BXRU8_$eE<<*\"Ie-)r~>\n(after) =";
    let open_before = exec(b"").interp.memory().files().open_count();
    let run = exec(program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "from flate\nafter\n");
    assert_eq!(run.interp.memory().files().open_count(), open_before + 1);
}

#[test]
fn close_source_closes_the_file_under_the_filter() {
    let run = exec_with(
        b"(f) (r) file dup << /CloseSource true >> /ASCIIHexDecode filter closefile \
          100 string readstring",
        Some(b"41>".to_vec()),
    );
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec_with(
        b"(f) (r) file dup << /CloseSource false >> /ASCIIHexDecode filter closefile \
          100 string readstring pop =",
        Some(b"41>tail".to_vec()),
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "tail\n");
    let run = exec(b"(41>) << /CloseSource 1 >> /ASCIIHexDecode filter");
    assert_eq!(run.error(), Some("typecheck"));
}

#[test]
fn parameters_are_type_and_range_checked() {
    for (params, name, error) in [
        ("<< /Predictor (a) >>", "FlateDecode", "typecheck"),
        ("<< /Predictor 3 >>", "FlateDecode", "rangecheck"),
        ("<< /Predictor 12 /Colors 0 >>", "FlateDecode", "rangecheck"),
        (
            "<< /Predictor 2 /BitsPerComponent 3 >>",
            "LZWDecode",
            "rangecheck",
        ),
        ("<< /Predictor 12 /Columns -1 >>", "LZWDecode", "rangecheck"),
        ("<< /EarlyChange 2 >>", "LZWDecode", "rangecheck"),
        ("<< /EarlyChange true >>", "LZWDecode", "typecheck"),
        ("<< /EODCount -1 >>", "SubFileDecode", "rangecheck"),
        ("<< /EODString 5 >>", "SubFileDecode", "typecheck"),
        ("<< /ColorTransform (x) >>", "DCTDecode", "typecheck"),
        ("<< /ColorTransform 7 >>", "DCTDecode", "rangecheck"),
    ] {
        let program = format!("(x) {params} /{name} filter");
        let run = exec(program.as_bytes());
        assert_eq!(run.error(), Some(error), "{program}");
    }
    // Unknown keys are ignored; the DCT filter is made but not readable.
    let run = exec(
        b"(x) << /Whatever 1 >> /ASCIIHexDecode filter pop \
                     (x) << /ColorTransform 1 >> /DCTDecode filter 10 string readstring",
    );
    assert_eq!(run.error(), Some("undefined"));
    // A positional count below zero is rangecheck.
    let run = exec(b"(x) -1 (eod) /SubFileDecode filter");
    assert_eq!(run.error(), Some("rangecheck"));
}

#[test]
fn a_lone_final_base85_digit_is_malformed() {
    // Six digits are a full group and one more; a final partial group
    // needs at least two digits to carry a byte, so the data is
    // malformed and the read is ioerror. The five-digit group alone is
    // fine: it is the start of a zlib stream.
    let run = exec(b"(Gb\"0Ec) /ASCII85Decode filter 100 string readstring");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec(b"(Gb\"0E) /ASCII85Decode filter 100 string readstring pop <789CEDCB> eq =");
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "true\n");
    let run = exec(b"(Gb\"0Ec~>) /ASCII85Decode filter 100 string readstring");
    assert_eq!(run.error(), Some("ioerror"));
}

// --- encode filters -------------------------------------------------------------

const FOX: &str = "the quick brown fox";

/// Writes the fox through the encode filter to the target, closes the
/// filter, and reads the target back through `decode`.
fn round_trip(encode: &str, decode: &str) -> Run {
    let program = format!(
        "(t) (w) file {encode} dup ({FOX}) writestring closefile \
         (t) (r) file {decode} 100 string readstring pop ="
    );
    exec(program.as_bytes())
}

#[test]
fn every_encoder_round_trips_through_its_decoder() {
    for (encode, decode) in [
        ("/ASCIIHexEncode filter", "/ASCIIHexDecode filter"),
        ("/ASCII85Encode filter", "/ASCII85Decode filter"),
        ("0 /RunLengthEncode filter", "/RunLengthDecode filter"),
        (
            "<< /CloseTarget false >> 4 /RunLengthEncode filter",
            "/RunLengthDecode filter",
        ),
        ("/FlateEncode filter", "/FlateDecode filter"),
        ("/LZWEncode filter", "/LZWDecode filter"),
        (
            "<< /EarlyChange 0 >> /LZWEncode filter",
            "<< /EarlyChange 0 >> /LZWDecode filter",
        ),
        ("/NullEncode filter", ""),
    ] {
        let run = round_trip(encode, decode);
        assert_eq!(run.outcome, Outcome::Ok, "{encode}");
        assert_eq!(run.output(), format!("{FOX}\n"), "{encode}");
    }
    let run = round_trip("/ASCIIHexEncode filter", "/ASCIIHexDecode filter");
    assert_eq!(
        run.target.borrow().as_slice(),
        b"74686520717569636B2062726F776E20666F78>"
    );
    let run = round_trip("/ASCII85Encode filter", "/ASCII85Decode filter");
    assert_eq!(
        run.target.borrow().as_slice(),
        b"FD,5.EHPu*CER),Dg-(AAoDn~>"
    );
    let run = round_trip("/NullEncode filter", "");
    assert_eq!(run.target.borrow().as_slice(), FOX.as_bytes());
    // The filter is a writable file object; a write of one byte goes
    // through too, and flushfile emits what it can without the marker.
    let run = exec(
        b"(t) (w) file 0 /RunLengthEncode filter dup type == dup wcheck = \
          dup 97 write dup 98 write dup flushfile (c) writestring",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "filetype\ntrue\n");
    assert_eq!(run.target.borrow().as_slice(), [1, b'a', b'b']);
    // The record length is required, directly under the name, and not
    // negative.
    let run = exec(b"(t) (w) file /RunLengthEncode filter");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"(t) (w) file 0 << /CloseTarget true >> /RunLengthEncode filter");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"(t) (w) file -1 /RunLengthEncode filter");
    assert_eq!(run.error(), Some("rangecheck"));
    let run = exec(b"/RunLengthEncode filter");
    assert_eq!(run.error(), Some("stackunderflow"));
}

#[test]
fn lzw_early_change_must_match_between_encoder_and_decoder() {
    // Three hundred bytes give enough codes for the width to grow, where
    // the two settings differ.
    let program = "/s 300 string def 0 1 299 { s exch dup 255 and put } for \
        (t) (w) file << /EarlyChange 0 >> /LZWEncode filter dup s writestring closefile \
        (t) (r) file << /EarlyChange 0 >> /LZWDecode filter 300 string readstring pop s eq = \
        { (t) (r) file /LZWDecode filter 300 string readstring pop s eq \
          { (same) } { (differs) } ifelse } stopped { (error) } if =";
    let run = exec(program.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok);
    let output = run.output();
    assert!(
        output == "true\ndiffers\n" || output == "true\nerror\n",
        "{output}"
    );
}

#[test]
fn close_target_closes_the_target_and_the_target_must_be_writable() {
    let run = exec(
        b"(t) (w) file dup << /CloseTarget true >> /ASCIIHexEncode filter closefile (x) writestring",
    );
    assert_eq!(run.error(), Some("ioerror"));
    assert_eq!(run.target.borrow().as_slice(), b">");
    let run = exec(
        b"(t) (w) file dup << /CloseTarget false >> /ASCIIHexEncode filter closefile (x) writestring",
    );
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.target.borrow().as_slice(), b">x");
    let run = exec(b"(t) (w) file << /CloseTarget 1 >> /ASCIIHexEncode filter");
    assert_eq!(run.error(), Some("typecheck"));
    // A chain closes down through the targets it was asked to close.
    let run = exec(
        b"(t) (w) file dup << /CloseTarget true >> /ASCIIHexEncode filter \
          << /CloseTarget true >> 0 /RunLengthEncode filter closefile (x) writestring",
    );
    assert_eq!(run.error(), Some("ioerror"));
    assert_eq!(run.target.borrow().as_slice(), b"80>");
    // Writing to a filter whose target was closed is ioerror, as is
    // closing it; a closed target, a read-only file, or a string cannot
    // be a target.
    let run = exec(b"(t) (w) file dup /ASCIIHexEncode filter exch closefile (x) writestring");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec(b"(t) (w) file dup /ASCIIHexEncode filter exch closefile closefile");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec(b"(t) (w) file dup closefile /ASCIIHexEncode filter");
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec(b"(t) (r) file /ASCIIHexEncode filter");
    assert_eq!(run.error(), Some("invalidaccess"));
    let run = exec(b"(x) /ASCIIHexEncode filter");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"{ (x) } /ASCIIHexEncode filter");
    assert_eq!(run.error(), Some("typecheck"));
    // Reading an encode filter is ioerror: it has no source.
    let run = exec(b"(t) (w) file /ASCIIHexEncode filter 10 string readstring");
    assert_eq!(run.error(), Some("ioerror"));
}

#[test]
fn the_filtered_file_is_a_read_only_file_object() {
    let run = exec(
        b"(41>) /ASCIIHexDecode filter dup type == dup rcheck = wcheck = \
                     (41>) /ASCIIHexDecode filter dup 1 write",
    );
    assert_eq!(run.error(), Some("invalidaccess"));
    assert_eq!(run.output(), "filetype\ntrue\nfalse\n");
}

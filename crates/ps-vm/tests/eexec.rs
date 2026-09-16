// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `eexec` as a layered file: decrypted bytes reach the scanner and the
//! read operators, `currentfile` is the layer, `closefile` ends it with
//! the base positioned exactly after the section, `systemdict` is pushed
//! for its duration and popped however the section ends, and a string
//! operand runs the same way. The hexadecimal and string scenarios are
//! corpus files too; the binary section lives here because the corpus
//! scanner survey cannot read binary bytes as tokens. `systemdict` is
//! read-only, so a section defines through `userdict`, as a font program
//! defines into the dictionaries it begins.

use ps_fonts::testing::{TRAILER, eexec_binary, eexec_hex};
use ps_vm::{
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

/// A capability answering `(f) (r) file` with fixed bytes.
struct OneFile(Vec<u8>);

impl FileCapability for OneFile {
    fn open(&mut self, name: &[u8], mode: &[u8]) -> Result<Box<dyn Stream>, VmError> {
        if name != b"f" || mode != b"r" {
            return Err(VmError::UndefinedFileName);
        }
        Ok(Box::new(Input(self.0.clone(), 0)))
    }
}

struct Run {
    interp: Interp,
    outcome: Outcome,
    out: Capture,
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
    let config = Config {
        io,
        capabilities: Capabilities {
            file: file.map(|bytes| Box::new(OneFile(bytes)) as Box<dyn FileCapability>),
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
    }
}

fn exec(program: &[u8]) -> Run {
    exec_with(program, None)
}

fn hex_program(head: &str, plain: &[u8], tail: &str) -> Vec<u8> {
    let mut program = head.as_bytes().to_vec();
    program.extend_from_slice(eexec_hex(plain).as_bytes());
    program.extend_from_slice(tail.as_bytes());
    program
}

#[test]
fn a_hex_section_runs_and_the_trailer_runs_in_the_clear() {
    let program = hex_program(
        "currentfile eexec\n",
        b"userdict /x 42 put mark currentfile closefile\n",
        &format!("{TRAILER}x =\n"),
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "42\n");
    assert_eq!(run.interp.dstack().len(), 3);
    assert!(run.interp.ostack().is_empty());
}

#[test]
fn a_binary_section_runs_the_same_way() {
    let mut program = b"currentfile eexec\n".to_vec();
    program.extend_from_slice(&eexec_binary(
        b"userdict /x 42 put mark currentfile closefile\n",
    ));
    program.extend_from_slice(format!("\n{TRAILER}x =\n").as_bytes());
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "42\n");
    assert_eq!(run.interp.dstack().len(), 3);
}

#[test]
fn readstring_token_and_currentfile_work_through_the_layer() {
    let program = hex_program(
        "currentfile /outer exch def currentfile eexec\n",
        b"currentfile 5 string readstring\nabcde pop ==\n\
          currentfile token\n/tok\npop ==\n\
          currentfile outer eq = currentfile currentfile eq =\n\
          currentdict systemdict eq =\n\
          mark currentfile closefile",
        &format!("\n(tail) =\n{TRAILER}(after) =\n"),
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(
        run.output(),
        "(abcde)\n/tok\nfalse\ntrue\ntrue\ntail\nafter\n"
    );
}

#[test]
fn a_section_ending_by_end_of_data_pops_systemdict() {
    let program = hex_program("currentfile eexec\n", b"userdict begin /y 7 def y =\n", "");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "7\n");
    assert_eq!(run.interp.dstack().len(), 3);
    let base = run.interp.run_file();
    let position = run.interp.memory().files().position(base.handle().unwrap());
    assert_eq!(position, Some(program.len()));
}

#[test]
fn a_string_section_runs_in_hex_or_binary_form() {
    let mut program = b"<".to_vec();
    for byte in eexec_binary(b"(hi) print") {
        program.extend_from_slice(format!("{byte:02x}").as_bytes());
    }
    program.extend_from_slice(b"> eexec (!) print\n");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "hi!");
    assert_eq!(run.interp.dstack().len(), 3);

    let program = format!(
        "({}) eexec currentdict systemdict eq =",
        eexec_hex(b"currentdict systemdict eq =").replace('\n', " ")
    );
    let run = exec(program.as_bytes());
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "true\nfalse\n");
}

#[test]
fn an_error_unwinds_the_section_closing_the_layer() {
    let open_before = exec(b"").interp.memory().files().open_count();
    let program = hex_program("{ currentfile eexec } stopped\n", b"1 0 div", "");
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.interp.dstack().len(), 3);
    // The operands `div` did not take are still there under `true`.
    assert_eq!(run.interp.ostack().len(), 3);
    assert_eq!(run.interp.ostack()[2].as_bool(), Some(true));
    assert_eq!(run.interp.memory().files().open_count(), open_before);

    let program = hex_program("currentfile eexec\n", b"nosuchname", "");
    let run = exec(&program);
    assert_eq!(run.error(), Some("undefined"));
    assert_eq!(run.interp.dstack().len(), 3);
    assert_eq!(run.interp.memory().files().open_count(), open_before);
}

#[test]
fn a_capability_file_can_be_a_base_and_a_closed_one_is_ioerror() {
    let cipher = eexec_binary(b"(from file) =");
    let run = exec_with(b"(f) (r) file eexec", Some(cipher.clone()));
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "from file\n");
    let run = exec_with(b"(f) (r) file dup closefile eexec", Some(cipher));
    assert_eq!(run.error(), Some("ioerror"));
    let run = exec(b"42 eexec");
    assert_eq!(run.error(), Some("typecheck"));
    let run = exec(b"(abc) noaccess eexec");
    assert_eq!(run.error(), Some("invalidaccess"));
}

#[test]
fn closefile_on_the_layer_leaves_the_base_after_the_consumed_bytes() {
    // The layer's last token is ended by the end of the section, so the
    // byte after the section is the first the base reads.
    let program = hex_program(
        "currentfile eexec\n",
        b"mark currentfile closefile",
        "(x) = cleartomark\n",
    );
    let run = exec(&program);
    assert_eq!(run.outcome, Outcome::Ok);
    assert_eq!(run.output(), "x\n");
    assert!(run.interp.ostack().is_empty());
}

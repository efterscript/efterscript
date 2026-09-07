// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Command-line front end. The library holds no ambient authority, so this
//! binary is where host files and host stdio are read and handed to the
//! interpreter as streams.

use std::cell::RefCell;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;

use ps_graphics::{Collected, Graphics};
use ps_vm::{Config, Interp, Io, Outcome, SliceSource, Stream, VmError};

struct HostStdout;

impl Stream for HostStdout {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        std::io::stdout()
            .write_all(buf)
            .map(|()| buf.len())
            .map_err(|_| VmError::IoError)
    }

    fn flush(&mut self) -> Result<(), VmError> {
        std::io::stdout().flush().map_err(|_| VmError::IoError)
    }
}

struct HostStderr;

impl Stream for HostStderr {
    fn read(&mut self, _: &mut [u8]) -> Result<usize, VmError> {
        Ok(0)
    }

    fn write(&mut self, buf: &[u8]) -> Result<usize, VmError> {
        std::io::stderr()
            .write_all(buf)
            .map(|()| buf.len())
            .map_err(|_| VmError::IoError)
    }
}

struct HostStdin;

impl Stream for HostStdin {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, VmError> {
        std::io::stdin().read(buf).map_err(|_| VmError::IoError)
    }

    fn write(&mut self, _: &[u8]) -> Result<usize, VmError> {
        Err(VmError::InvalidAccess)
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: efterscript run <file.ps>");
    eprintln!("       efterscript ir <file.ps>");
    eprintln!("       efterscript pdf <file.ps> [<out.pdf> | -]");
    eprintln!("       efterscript --version");
    ExitCode::from(2)
}

fn exit_code(outcome: Outcome) -> ExitCode {
    match outcome {
        Outcome::Ok | Outcome::Suspended => ExitCode::SUCCESS,
        Outcome::Error(_) => ExitCode::FAILURE,
    }
}

/// Runs the program in `bytes` against the host streams with a graphics
/// backend whose pages are discarded; the exit code is 0 for a job that
/// ended normally and 1 for one ended by an error.
fn run(bytes: &[u8]) -> ExitCode {
    let config = Config {
        io: Io::new(HostStdout, HostStderr).with_stdin(HostStdin),
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    interp.set_graphics_backend(Box::new(Graphics::new(())));
    let outcome = interp.run(&mut SliceSource::new(bytes));
    let _ = std::io::stdout().flush();
    exit_code(outcome)
}

/// Runs the program and prints the IR dump of every page it produced,
/// pages separated by a blank line, then the document marks after the
/// pages when there are any. Only the dump goes to standard output; the
/// program's own output joins the error report on standard error, so
/// the dump can be piped.
fn ir(bytes: &[u8]) -> ExitCode {
    let config = Config {
        io: Io::new(HostStderr, HostStderr).with_stdin(HostStdin),
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let collected = Rc::new(RefCell::new(Collected::default()));
    interp.set_graphics_backend(Box::new(Graphics::new(collected.clone())));
    let outcome = interp.run(&mut SliceSource::new(bytes));
    let text = collected.borrow().dump();
    print!("{text}");
    let _ = std::io::stdout().flush();
    exit_code(outcome)
}

/// Distils the program into `out`. The exit code follows the job's
/// outcome once the document is written, so a partially distilled job is
/// still inspectable; 2 means the document itself could not be written.
fn distill_to<W: Write + 'static>(bytes: &[u8], io: Io, out: W) -> ExitCode {
    let config = Config {
        io,
        ..Default::default()
    };
    let options = remelt::Options::default();
    let result = remelt::distill(bytes, config, &options, BufWriter::new(out));
    let _ = std::io::stdout().flush();
    match result {
        Ok((report, _)) => {
            if !report.substitutions.is_empty() {
                let pairs: Vec<String> = report
                    .substitutions
                    .iter()
                    .map(|s| {
                        format!(
                            "{} -> {}",
                            String::from_utf8_lossy(&s.requested),
                            s.substitute
                        )
                    })
                    .collect();
                let plural = if pairs.len() == 1 { "" } else { "s" };
                eprintln!(
                    "efterscript: {} font{plural} substituted: {}",
                    pairs.len(),
                    pairs.join(", ")
                );
            }
            for note in &report.notes {
                eprintln!("efterscript: {note}");
            }
            if !report.marks_ignored.is_empty() {
                let total: usize = report.marks_ignored.values().sum();
                let kinds: Vec<String> = report
                    .marks_ignored
                    .iter()
                    .map(|(kind, n)| format!("{kind}×{n}"))
                    .collect();
                eprintln!(
                    "efterscript: {total} pdfmark(s) ignored: {}",
                    kinds.join(", ")
                );
            }
            exit_code(report.outcome)
        }
        Err(e) => {
            eprintln!("efterscript: {e}");
            ExitCode::from(2)
        }
    }
}

/// Writes the PDF to `target`, or to standard output for `-`, in which
/// case the program's own output moves to standard error so the PDF can
/// be piped.
fn pdf(bytes: &[u8], target: &Path) -> ExitCode {
    if target == Path::new("-") {
        let io = Io::new(HostStderr, HostStderr).with_stdin(HostStdin);
        return distill_to(bytes, io, std::io::stdout());
    }
    match std::fs::File::create(target) {
        Ok(file) => {
            let io = Io::new(HostStdout, HostStderr).with_stdin(HostStdin);
            distill_to(bytes, io, file)
        }
        Err(e) => {
            eprintln!("efterscript: cannot create {}: {e}", target.display());
            ExitCode::from(2)
        }
    }
}

fn read_program(path: &str) -> Result<Vec<u8>, ExitCode> {
    std::fs::read(path).map_err(|e| {
        eprintln!("efterscript: cannot read {path}: {e}");
        ExitCode::from(2)
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["--version" | "-V"] => {
            println!("efterscript {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        [command @ ("run" | "ir"), path] => match read_program(path) {
            Ok(bytes) if *command == "run" => run(&bytes),
            Ok(bytes) => ir(&bytes),
            Err(code) => code,
        },
        ["pdf", input, rest @ ..] if rest.len() <= 1 => {
            let target = rest
                .first()
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(input).with_extension("pdf"));
            match read_program(input) {
                Ok(bytes) => pdf(&bytes, &target),
                Err(code) => code,
            }
        }
        _ => usage(),
    }
}

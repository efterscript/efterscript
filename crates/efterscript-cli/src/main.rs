// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Command-line front end. The library holds no ambient authority, so this
//! binary is where host files and host stdio are read and handed to the
//! interpreter as streams.

use std::io::{Read, Write};
use std::process::ExitCode;

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
    eprintln!("       efterscript --version");
    ExitCode::from(2)
}

/// Runs the program in `bytes` against the host streams; the exit code is
/// 0 for a job that ended normally and 1 for one ended by an error.
fn run(bytes: &[u8]) -> ExitCode {
    let config = Config {
        io: Io::new(HostStdout, HostStderr).with_stdin(HostStdin),
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = interp.run(&mut SliceSource::new(bytes));
    let _ = std::io::stdout().flush();
    match outcome {
        Outcome::Ok | Outcome::Suspended => ExitCode::SUCCESS,
        Outcome::Error(_) => ExitCode::FAILURE,
    }
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
        ["run", path] => match std::fs::read(path) {
            Ok(bytes) => run(&bytes),
            Err(e) => {
                eprintln!("efterscript: cannot read {path}: {e}");
                ExitCode::from(2)
            }
        },
        _ => usage(),
    }
}

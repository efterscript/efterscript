// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Session front-end: the job a host feeds when it acts as a printer.
//!
//! A [`Job`] is one interpreter seeded with the host's identity entries
//! and prelude, with a PDF sink behind it. The host [`feed`](Job::feed)s
//! the program in pieces of any size as they arrive from its transport;
//! each call executes as far as the bytes allow — suspending inside a
//! token or a read if it must — and returns the reply bytes (the
//! program's standard output) and the error-report bytes (its standard
//! error, the conventional `%%[ Error: … ]%%` lines) produced since the
//! previous call, so a query is answered while the program is still
//! arriving. [`finish`](Job::finish) signals end of data, runs to
//! completion, and returns the outcome, the PDF, and the writer's
//! report. Nothing survives a job: a host maps one connection to one
//! job and re-sends what a printer would have kept.
//!
//! Status text is the host's business: this crate reports facts (an
//! error name, an offending command, a page count) and phrases nothing.
//! Transports — a socket, a pipe, a serial line, a print protocol — are
//! the host's too. The C ABI in [`ffi`], declared by `include/platen.h`,
//! is the same job for hosts written in other languages, natively or in
//! an Emscripten program.
//!
//! Named for the plate that presses the paper against the type.

pub mod ffi;
mod identity;

use std::{error, fmt};

pub use remelt::{Options, Params, Report};

use ps_vm::{Capture, Config, Io, Limits};
use remelt::{Distillation, PdfSink};

/// How a job is set up: what the device claims to be, what it runs
/// before the program, how the document is written, and how much
/// execution it may spend.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JobConfig {
    /// `statusdict` entries, each value as PostScript literal text —
    /// `(Fictional Press)`, `47.0`, `true`, `/name`, `[612 792]`,
    /// `<< /a 1 >>` — parsed when the job is created.
    pub identity: Vec<(String, String)>,
    /// A program run once at the server level before the job, its
    /// definitions in place for the job; its output is discarded.
    pub prelude: Option<Vec<u8>>,
    /// The password `exitserver` expects.
    pub server_password: i32,
    /// The writer's parameters: compression, embedding, and the rest.
    pub options: Options,
    /// Objects the job may execute before it is stopped with the
    /// [`Outcome::Budget`]; `None` is unbounded.
    pub step_budget: Option<u64>,
}

/// What a `feed` produced.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Progress {
    /// The program's standard output since the previous call.
    pub replies: Vec<u8>,
    /// The program's error output since the previous call.
    pub errors: Vec<u8>,
    /// Whether the job has ended before its data did: it reached an
    /// uncaught error, `quit`, or the budget. Later feeds are refused.
    pub done: bool,
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The program ran to the end of its data.
    Ok,
    /// An uncaught error, as `$error` recorded it.
    Error { name: String, offending: String },
    /// The execution budget was spent; the program may or may not have
    /// been runaway, but it was not finished.
    Budget,
}

/// What `finish` returns.
#[derive(Clone, Debug, PartialEq)]
pub struct Finished {
    pub outcome: Outcome,
    /// The document, complete whatever the outcome: pages shown before
    /// an error are in it.
    pub pdf: Vec<u8>,
    pub report: Report,
    /// Standard output produced by the end of data.
    pub replies: Vec<u8>,
    /// Error output produced by the end of data.
    pub errors: Vec<u8>,
}

#[derive(Debug)]
pub enum JobError {
    /// An identity value is not one PostScript literal.
    Identity { key: String, detail: String },
    /// The prelude raised an error.
    Prelude { name: String, offending: String },
    /// The document could not be started or closed.
    Document(remelt::Error),
    /// `feed` after the job ended.
    Finished,
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Identity { key, detail } => {
                write!(f, "identity entry {key}: {detail}")
            }
            JobError::Prelude { name, offending } => {
                write!(f, "prelude failed: {name} in {offending}")
            }
            JobError::Document(e) => write!(f, "{e}"),
            JobError::Finished => f.write_str("the job has ended"),
        }
    }
}

impl error::Error for JobError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            JobError::Document(e) => Some(e),
            _ => None,
        }
    }
}

/// One job: an interpreter, its capture streams, and a document in
/// memory.
pub struct Job {
    distillation: Distillation<Vec<u8>>,
    replies: Capture,
    errors: Capture,
}

impl Job {
    /// Builds the interpreter — the identity parsed and seeded, the
    /// prelude run — and starts the document.
    pub fn new(config: JobConfig) -> Result<Self, JobError> {
        let JobConfig {
            identity,
            prelude,
            server_password,
            options,
            step_budget,
        } = config;
        let identity = identity
            .into_iter()
            .map(|(key, text)| match identity::literal(&text) {
                Ok(value) => Ok((key, value)),
                Err(detail) => Err(JobError::Identity { key, detail }),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let (io, replies, errors) = Io::capture();
        let io = io.with_stdin(Capture::new());
        let config = Config {
            io,
            limits: Limits {
                steps: step_budget,
                ..Limits::default()
            },
            identity,
            prelude,
            server_password,
            ..Config::default()
        };
        let sink = PdfSink::new(Vec::new(), options).map_err(JobError::Document)?;
        let distillation = Distillation::new(config, sink).map_err(|e| match e {
            remelt::Error::Prelude(p) => JobError::Prelude {
                name: p.name,
                offending: p.offending,
            },
            other => JobError::Document(other),
        })?;
        // The prelude's own output is the device's, not the job's.
        replies.clear();
        errors.clear();
        Ok(Job {
            distillation,
            replies,
            errors,
        })
    }

    /// Appends `bytes` to the program and executes as far as they allow.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Progress, JobError> {
        if self.distillation.is_done() {
            return Err(JobError::Finished);
        }
        self.distillation.feed(bytes);
        let (replies, errors) = self.drain();
        Ok(Progress {
            replies,
            errors,
            done: self.distillation.is_done(),
        })
    }

    /// Whether the job has ended before its data did.
    pub fn is_done(&self) -> bool {
        self.distillation.is_done()
    }

    /// Pages shown so far.
    pub fn pages(&self) -> usize {
        self.distillation.pages()
    }

    /// Signals end of data, runs the program to completion, and closes
    /// the document.
    pub fn finish(self) -> Result<Finished, JobError> {
        let Job {
            distillation,
            replies,
            errors,
        } = self;
        let (report, pdf) = distillation.finish().map_err(JobError::Document)?;
        let outcome = match &report.outcome {
            ps_vm::Outcome::Ok => Outcome::Ok,
            ps_vm::Outcome::Error(_) if report.budget_exceeded => Outcome::Budget,
            ps_vm::Outcome::Error(summary) => Outcome::Error {
                name: summary.name.clone(),
                offending: summary.command.clone(),
            },
            // Unreachable once the data has ended; reported as the
            // interpreter would report input it cannot finish.
            ps_vm::Outcome::Suspended => Outcome::Error {
                name: "ioerror".to_string(),
                offending: String::new(),
            },
        };
        let (replies, errors) = (take(&replies), take(&errors));
        Ok(Finished {
            outcome,
            pdf,
            report,
            replies,
            errors,
        })
    }

    fn drain(&self) -> (Vec<u8>, Vec<u8>) {
        (take(&self.replies), take(&self.errors))
    }
}

fn take(capture: &Capture) -> Vec<u8> {
    let bytes = capture.bytes();
    capture.clear();
    bytes
}

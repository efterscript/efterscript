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
//! report.
//!
//! A [`Printer`] is the interpreter a device keeps between jobs: it is
//! built once, with the identity seeded and the prelude run, and serves
//! one [`Job`] at a time as a job server (PLRM3 §3.7.7). Each job starts
//! from the same initial state and is reverted at its end, unless it
//! uses `startjob` or `exitserver`, whose changes every later job
//! inherits. A job created alone with [`Job::new`] is a printer serving
//! that one job, so nothing it does outlives it.
//!
//! Status text is the host's business: this crate reports facts (an
//! error name, an offending command, a page count) and phrases nothing.
//! Transports — a socket, a pipe, a serial line, a print protocol — are
//! the host's too. The C ABI in [`ffi`], declared by `include/platen.h`,
//! is the same job for hosts written in other languages, natively or in
//! an Emscripten program.
//!
//! Named for the plate that presses the paper against the type.

// Unsafe code is denied crate-wide and allowed in `ffi` alone, the C
// boundary; see that module.
#![deny(unsafe_code)]

pub mod ffi;
mod identity;

use std::cell::RefCell;
use std::rc::Rc;
use std::{error, fmt};

pub use efterscript_remelt::{Options, Params, Report};

use efterscript_remelt::{Distillation, PdfSink};
use efterscript_vm::{Capture, Config, Interp, Io, Limits};

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
    /// Whether the job changed the initial state of later jobs on its
    /// printer (`startjob` or `exitserver`).
    pub permanent: bool,
}

#[derive(Debug)]
pub enum JobError {
    /// An identity value is not one PostScript literal.
    Identity { key: String, detail: String },
    /// The prelude raised an error.
    Prelude { name: String, offending: String },
    /// The document could not be started or closed.
    Document(efterscript_remelt::Error),
    /// `feed` after the job ended.
    Finished,
    /// The printer is serving another job.
    Busy,
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
            JobError::Busy => f.write_str("a job is open on this printer"),
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

/// Where a printer keeps its interpreter between jobs; empty while a
/// job has it. Shared by the printer and its open job, so either may be
/// dropped first.
struct Slot {
    interp: Option<Interp>,
    options: Options,
    replies: Capture,
    errors: Capture,
}

/// The interpreter a device keeps between jobs, serving one job at a
/// time.
pub struct Printer {
    slot: Rc<RefCell<Slot>>,
}

impl Printer {
    /// Builds the interpreter — the identity parsed and seeded, the
    /// prelude run once, its output discarded. The execution budget
    /// applies to each job.
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
        let interp = Interp::try_with_config(config).map_err(|p| JobError::Prelude {
            name: p.name,
            offending: p.offending,
        })?;
        // The prelude's own output is the device's, not a job's.
        replies.clear();
        errors.clear();
        Ok(Printer {
            slot: Rc::new(RefCell::new(Slot {
                interp: Some(interp),
                options,
                replies,
                errors,
            })),
        })
    }

    /// Opens a job over the printer's interpreter; [`JobError::Busy`]
    /// while another is open.
    pub fn job(&self) -> Result<Job, JobError> {
        Job::open(self.slot.clone())
    }
}

/// One job: a document in memory over a printer's interpreter, and the
/// printer's capture streams.
pub struct Job {
    /// `None` once finished or abandoned.
    distillation: Option<Distillation<Vec<u8>>>,
    home: Rc<RefCell<Slot>>,
}

impl Job {
    /// A printer serving this one job: builds the interpreter — the
    /// identity parsed and seeded, the prelude run — and starts the
    /// document.
    pub fn new(config: JobConfig) -> Result<Self, JobError> {
        Printer::new(config)?.job()
    }

    fn open(home: Rc<RefCell<Slot>>) -> Result<Self, JobError> {
        let mut slot = home.borrow_mut();
        if slot.interp.is_none() {
            return Err(JobError::Busy);
        }
        let sink = PdfSink::new(Vec::new(), slot.options.clone()).map_err(JobError::Document)?;
        let mut interp = slot.interp.take().expect("checked above");
        interp
            .begin_job()
            .expect("an idle interpreter has no job open");
        slot.replies.clear();
        slot.errors.clear();
        drop(slot);
        Ok(Job {
            distillation: Some(Distillation::over(interp, sink)),
            home,
        })
    }

    fn distillation(&self) -> &Distillation<Vec<u8>> {
        self.distillation.as_ref().expect("a live job")
    }

    /// Appends `bytes` to the program and executes as far as they allow.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Progress, JobError> {
        let distillation = self.distillation.as_mut().expect("a live job");
        if distillation.is_done() {
            return Err(JobError::Finished);
        }
        distillation.feed(bytes);
        let (replies, errors) = self.drain();
        Ok(Progress {
            replies,
            errors,
            done: self.distillation().is_done(),
        })
    }

    /// Whether the job has ended before its data did.
    pub fn is_done(&self) -> bool {
        self.distillation().is_done()
    }

    /// Pages shown so far.
    pub fn pages(&self) -> usize {
        self.distillation().pages()
    }

    /// Signals end of data, runs the program to completion, and closes
    /// the document; the interpreter goes back to its printer.
    pub fn finish(mut self) -> Result<Finished, JobError> {
        let distillation = self.distillation.take().expect("a live job");
        // A document that cannot be closed takes the interpreter with it:
        // the printer is left busy for good. Writing to memory does not
        // fail.
        let (report, pdf, mut interp) = distillation.finish_keep().map_err(JobError::Document)?;
        let end = interp.end_job();
        self.home.borrow_mut().interp = Some(interp);
        let outcome = match &report.outcome {
            efterscript_vm::Outcome::Ok => Outcome::Ok,
            efterscript_vm::Outcome::Error(_) if report.budget_exceeded => Outcome::Budget,
            efterscript_vm::Outcome::Error(summary) => Outcome::Error {
                name: summary.name.clone(),
                offending: summary.command.clone(),
            },
            // Unreachable once the data has ended; reported as the
            // interpreter would report input it cannot finish.
            efterscript_vm::Outcome::Suspended => Outcome::Error {
                name: "ioerror".to_string(),
                offending: String::new(),
            },
        };
        let (replies, errors) = self.drain();
        Ok(Finished {
            outcome,
            pdf,
            report,
            replies,
            errors,
            permanent: end.permanent,
        })
    }

    fn drain(&self) -> (Vec<u8>, Vec<u8>) {
        let slot = self.home.borrow();
        (take(&slot.replies), take(&slot.errors))
    }
}

/// A job dropped before `finish` is abandoned: the document is
/// discarded, the job server ends the job — reverting it unless it made
/// itself permanent — and the interpreter goes back to its printer.
impl Drop for Job {
    fn drop(&mut self) {
        if let Some(distillation) = self.distillation.take() {
            let mut interp = distillation.abandon();
            interp.end_job();
            let mut slot = self.home.borrow_mut();
            slot.replies.clear();
            slot.errors.clear();
            slot.interp = Some(interp);
        }
    }
}

fn take(capture: &Capture) -> Vec<u8> {
    let bytes = capture.bytes();
    capture.clear();
    bytes
}

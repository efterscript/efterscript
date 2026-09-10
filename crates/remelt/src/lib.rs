// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The distillation engine — the v1 product.
//!
//! Drives the VM, consumes the vector IR, and writes PDF. Vector-preserving
//! and colour-preserving: nothing is rasterised, nothing is converted.
//! Policies reach the writer as [`Params`]: the embedder's options first,
//! then the job's own `setdistillerparams` requests, merged as they arrive
//! (see [`params`]). Text, the document structure `pdfmark` builds,
//! embed-all, and image downsampling are in; image recompression and
//! colour strategies are later layers.
//!
//! [`PdfSink`] is a [`PageSink`] that writes each delivered page into a
//! `pdf-out` document as it arrives, so a page is on disk when `showpage`
//! returns and memory is bounded by the largest page. [`distill`] is the
//! driver: it builds an interpreter around a sink, runs a program, and
//! closes the document — including after a job that ended in an error,
//! whose pages are still worth having.
//!
//! Named for the letterpress practice of remelting hellbox lead into fresh
//! type: old type in, clean type out.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::{Seek, Write};
use std::rc::Rc;
use std::{error, fmt};

use ps_graphics::{DocMark, Graphics, Page, PageSink};
use ps_vm::{Config, FontSubstitution, Interp, Outcome, PreludeError, SliceSource};

mod composite;
mod content;
mod downsample;
mod embed_all;
mod embedded;
mod fonts;
mod marks;
pub mod params;
mod resources;
mod sink;

pub use params::{Downsample, MarkValue, NotHonoured, Params};
pub use sink::PdfSink;

/// How the PDF is written: the parameters in force before the job says
/// anything. The default compresses; goldens and anything meant to be
/// read as text turn compression off.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    pub params: Params,
}

impl Options {
    /// The defaults with `CompressPages` set as given.
    pub fn compress(enabled: bool) -> Self {
        Options {
            params: Params {
                compress_pages: enabled,
                ..Params::default()
            },
        }
    }

    /// `self` with `key` locked against the job's requests.
    pub fn lock(mut self, key: &str) -> Self {
        self.params = self.params.lock(key);
        self
    }
}

/// What a distillation run produced besides the file: the interpreter's
/// outcome is data here, not an error, since a job that failed still
/// leaves a finished document.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub outcome: Outcome,
    /// Pages written to the document.
    pub pages: usize,
    /// Every font the program asked for by a name that resolved to one
    /// of the resident fonts through substitution, in order.
    pub substitutions: Vec<FontSubstitution>,
    /// What the document could not carry as it was recorded, one line
    /// each, prefixed with the page it concerns.
    pub notes: Vec<String>,
    /// `pdfmark`s honoured: document marks and link annotations.
    pub marks_written: usize,
    /// `pdfmark`s tolerated and dropped, by kind (`ANN/<Subtype>` for an
    /// annotation of another subtype).
    pub marks_ignored: BTreeMap<String, usize>,
    /// The parameters in effect when the document finished.
    pub params: Params,
    /// Parameter entries not applied, in order: unknown keys with their
    /// value, honoured keys with the value and the reason.
    pub not_honoured: Vec<NotHonoured>,
    /// Images reduced by downsampling.
    pub downsampled: usize,
    /// The `statusdict` entries in effect when the job started — the
    /// default identity, the embedder's seeding, and what the prelude
    /// added — keys and values in their syntactic forms.
    pub identity: Vec<(String, String)>,
    /// Whether a configured prelude ran.
    pub prelude_ran: bool,
}

/// Why a document could not be written. The interpreter's own failures
/// are not errors of the engine; see [`Report`].
#[derive(Debug)]
pub enum Error {
    Pdf(pdf_out::Error),
    /// The interpreter could not be built: its prelude failed.
    Prelude(PreludeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pdf(e) => write!(f, "cannot write the document: {e}"),
            Error::Prelude(e) => write!(f, "{e}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Pdf(e) => Some(e),
            Error::Prelude(e) => Some(e),
        }
    }
}

impl From<PreludeError> for Error {
    fn from(e: PreludeError) -> Self {
        Error::Prelude(e)
    }
}

impl From<pdf_out::Error> for Error {
    fn from(e: pdf_out::Error) -> Self {
        Error::Pdf(e)
    }
}

/// The sink as the interpreter's backend holds it. The backend is a
/// `'static` trait object and owns its sink, so the driver reaches the
/// sink through shared ownership and takes it back once the interpreter
/// is gone; the `Option` is what lets `finish` consume it.
struct Shared<W: Write>(Rc<RefCell<Option<PdfSink<W>>>>);

impl<W: Write> PageSink for Shared<W> {
    fn page(&mut self, page: Page) {
        if let Some(sink) = self.0.borrow_mut().as_mut() {
            sink.page(page);
        }
    }

    fn document(&mut self, mark: DocMark) {
        if let Some(sink) = self.0.borrow_mut().as_mut() {
            sink.document(mark);
        }
    }
}

/// Runs `program` in an interpreter configured by `config`, writing every
/// page it shows into `out` as it arrives, and closes the document. The
/// writer comes back with the report so a caller writing to memory keeps
/// its bytes and one writing to a file can close it. The header names
/// 1.7 whatever `CompatibilityLevel` says, since `out` need not seek;
/// see [`distill_seekable`].
pub fn distill<W: Write + 'static>(
    program: &[u8],
    config: Config,
    options: &Options,
    out: W,
) -> Result<(Report, W), Error> {
    distill_into(program, config, PdfSink::new(out, options.clone())?)
}

/// As [`distill`], for a sink the header can be revised on, so the file
/// names the `CompatibilityLevel` in effect at the end.
pub fn distill_seekable<W: Write + Seek + 'static>(
    program: &[u8],
    config: Config,
    options: &Options,
    out: W,
) -> Result<(Report, W), Error> {
    distill_into(
        program,
        config,
        PdfSink::new_seekable(out, options.clone())?,
    )
}

/// Runs `program` against a sink already started, so the caller chooses
/// how the document was opened.
pub fn distill_into<W: Write + 'static>(
    program: &[u8],
    config: Config,
    sink: PdfSink<W>,
) -> Result<(Report, W), Error> {
    let entries = sink.params().entries();
    let mut interp = Interp::try_with_config(config)?;
    let shared = Rc::new(RefCell::new(Some(sink)));
    let identity = interp.statusdict_entries();
    let prelude_ran = interp.prelude_ran();
    // A value the VM cannot hold (a name too long, nesting too deep) is
    // left out of the job's view of the parameters; the writer keeps it.
    let _ = interp.set_distiller_params(&entries);
    interp.set_graphics_backend(Box::new(Graphics::new(Shared(shared.clone()))));
    let outcome = interp.run(&mut SliceSource::new(program));
    let substitutions = interp.font_substitutions().to_vec();
    drop(interp);
    let sink = Rc::try_unwrap(shared)
        .ok()
        .and_then(RefCell::into_inner)
        .expect("the interpreter has been dropped and with it the only other handle");
    let pages = sink.pages();
    let notes = sink.notes().to_vec();
    let marks_written = sink.marks_written();
    let marks_ignored = sink.marks_ignored().clone();
    let params = sink.params().clone();
    let not_honoured = sink.not_honoured();
    let downsampled = sink.downsampled();
    let out = sink.finish()?;
    Ok((
        Report {
            outcome,
            pages,
            substitutions,
            notes,
            marks_written,
            marks_ignored,
            params,
            not_honoured,
            downsampled,
            identity,
            prelude_ran,
        },
        out,
    ))
}

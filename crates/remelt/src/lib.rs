// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The distillation engine — the v1 product.
//!
//! Drives the VM, consumes the vector IR, and writes PDF. Vector-preserving
//! and colour-preserving: nothing is rasterised, nothing is converted.
//! Policies (image recompression, font-embedding rules, colour strategy,
//! `setdistillerparams` compatibility), text, and `pdfmark` are later
//! layers on the two pieces here.
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
use std::io::Write;
use std::rc::Rc;
use std::{error, fmt};

use ps_graphics::{Graphics, Page, PageSink};
use ps_vm::{Config, Interp, Outcome, SliceSource};

mod content;
mod resources;
mod sink;

pub use sink::PdfSink;

/// How the PDF is written. Defaults to compressed streams; goldens and
/// anything meant to be read as text turn compression off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// Content and function streams go through the Flate container when
    /// set; image data always does.
    pub compress: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { compress: true }
    }
}

/// What a distillation run produced besides the file: the interpreter's
/// outcome is data here, not an error, since a job that failed still
/// leaves a finished document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub outcome: Outcome,
    /// Pages written to the document.
    pub pages: usize,
}

/// Why a document could not be written. The interpreter's own failures
/// are not errors of the engine; see [`Report`].
#[derive(Debug)]
pub enum Error {
    Pdf(pdf_out::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Pdf(e) => write!(f, "cannot write the document: {e}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Error::Pdf(e) => Some(e),
        }
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
}

/// Runs `program` in an interpreter configured by `config`, writing every
/// page it shows into `out` as it arrives, and closes the document. The
/// writer comes back with the report so a caller writing to memory keeps
/// its bytes and one writing to a file can close it.
pub fn distill<W: Write + 'static>(
    program: &[u8],
    config: Config,
    options: &Options,
    out: W,
) -> Result<(Report, W), Error> {
    let sink = PdfSink::new(out, options.clone())?;
    let shared = Rc::new(RefCell::new(Some(sink)));
    let mut interp = Interp::with_config(config);
    interp.set_graphics_backend(Box::new(Graphics::new(Shared(shared.clone()))));
    let outcome = interp.run(&mut SliceSource::new(program));
    drop(interp);
    let sink = Rc::try_unwrap(shared)
        .ok()
        .and_then(RefCell::into_inner)
        .expect("the interpreter has been dropped and with it the only other handle");
    let pages = sink.pages();
    let out = sink.finish()?;
    Ok((Report { outcome, pages }, out))
}

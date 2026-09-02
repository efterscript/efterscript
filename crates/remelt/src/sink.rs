// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The page sink: one PDF page per delivered IR page, written as it
//! arrives, and a document closed once at the end.
//!
//! `PageSink::page` cannot return an error, so the first failure is kept
//! and every later page ignored; `finish` reports it. The content stream
//! is built in memory and the page written through one `add_page` call,
//! so a page that fails is never half-written by this layer.

use std::io::Write;

use pdf_out::{Document, Filter, PageTree, write_info};
use ps_graphics::{Page, PageSink};

use crate::{Error, Options, content, resources::Objects};

/// Names the project and its version; the only Info entry, since dates
/// would cost determinism.
const PRODUCER: &str = concat!("EfterScript ", env!("CARGO_PKG_VERSION"));

pub struct PdfSink<W: Write> {
    doc: Document<W>,
    tree: PageTree,
    options: Options,
    pages: usize,
    error: Option<pdf_out::Error>,
}

impl<W: Write> PdfSink<W> {
    /// Starts a document on `out`; the header is written at once.
    pub fn new(out: W, options: Options) -> Result<Self, Error> {
        let mut doc = Document::new(out)?;
        let tree = PageTree::new(&mut doc);
        Ok(PdfSink {
            doc,
            tree,
            options,
            pages: 0,
            error: None,
        })
    }

    /// Writes a comment line between objects, for provenance marks such
    /// as a golden's generator.
    pub fn comment(&mut self, text: &str) -> Result<(), Error> {
        Ok(self.doc.comment(text)?)
    }

    /// Pages written so far.
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// Closes the document — page tree, catalog, Info, cross-reference
    /// table — and returns the writer. A failure latched while writing a
    /// page is returned instead.
    pub fn finish(self) -> Result<W, Error> {
        let PdfSink {
            mut doc,
            tree,
            error,
            ..
        } = self;
        if let Some(e) = error {
            return Err(e.into());
        }
        let root = tree.finish(&mut doc)?;
        let info = write_info(&mut doc, |d| {
            d.key("Producer").string(PRODUCER.as_bytes());
        })?;
        Ok(doc.finish(root, Some(info))?)
    }

    fn text_filter(&self) -> Filter {
        if self.options.compress {
            Filter::Flate
        } else {
            Filter::None
        }
    }

    fn write_page(&mut self, page: &Page) -> Result<(), pdf_out::Error> {
        let content = content::content(page);
        let filter = self.text_filter();
        let objects = Objects::write(&mut self.doc, page, filter)?;
        let media_box = page.media_box;
        self.tree.add_page(
            &mut self.doc,
            [media_box.llx, media_box.lly, media_box.urx, media_box.ury],
            filter,
            &content,
            |d| objects.resources(d),
        )?;
        Ok(())
    }
}

impl<W: Write> PageSink for PdfSink<W> {
    fn page(&mut self, page: Page) {
        if self.error.is_some() {
            return;
        }
        match self.write_page(&page) {
            Ok(()) => self.pages += 1,
            Err(e) => self.error = Some(e),
        }
    }
}

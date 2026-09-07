// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The page sink: one PDF page per delivered IR page, written as it
//! arrives, and a document closed once at the end.
//!
//! `PageSink::page` cannot return an error, so the first failure is kept
//! and every later page ignored; `finish` reports it. The content stream
//! is built in memory and the page written through one `add_page` call,
//! so a page that fails is never half-written by this layer. Embedded
//! fonts, the outline tree, named destinations, and link annotations
//! are written at `finish`, once every page has said which glyphs it
//! uses and every page has an id.

use std::collections::BTreeMap;
use std::io::Write;

use pdf_out::{Document, Filter, PageTree, write_info};
use ps_graphics::{DocMark, Page, PageSink};

use crate::fonts::FontTable;
use crate::marks::Marks;
use crate::{Error, Options, content, resources::Objects};

/// Names the project and its version; the only Info entry a job does
/// not supply, since dates would cost determinism.
const PRODUCER: &str = concat!("EfterScript ", env!("CARGO_PKG_VERSION"));

pub struct PdfSink<W: Write> {
    doc: Document<W>,
    tree: PageTree,
    options: Options,
    pages: usize,
    error: Option<pdf_out::Error>,
    fonts: FontTable,
    notes: Vec<String>,
    marks: Marks,
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
            fonts: FontTable::default(),
            notes: Vec::new(),
            marks: Marks::default(),
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

    /// What could not be written as it was recorded, one line each,
    /// prefixed with the page it concerns.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Marks honoured so far: document marks and link annotations.
    pub fn marks_written(&self) -> usize {
        self.marks.written
    }

    /// Marks tolerated and dropped, by kind.
    pub fn marks_ignored(&self) -> &BTreeMap<String, usize> {
        &self.marks.ignored
    }

    /// Closes the document — the embedded fonts, the annotations and
    /// document objects the marks made, page tree, catalog, Info,
    /// cross-reference table — and returns the writer. A failure latched
    /// while writing a page is returned instead.
    pub fn finish(self) -> Result<W, Error> {
        let filter = self.text_filter();
        let PdfSink {
            mut doc,
            tree,
            error,
            fonts,
            marks,
            ..
        } = self;
        if let Some(e) = error {
            return Err(e.into());
        }
        fonts.embedded.write_all(&mut doc, filter)?;
        let catalog = marks.write(&mut doc, tree.pages())?;
        let root = tree.finish_with(&mut doc, |d| catalog.entries(d))?;
        let info = write_info(&mut doc, |d| {
            marks.info(d);
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
        let filter = self.text_filter();
        let number = self.pages + 1;
        let mut notes = Vec::new();
        let objects = Objects::write(&mut self.doc, page, filter, &mut self.fonts, &mut notes)?;
        let content = content::content(page, objects.recode());
        notes.extend(content.notes);
        self.notes.extend(
            notes
                .into_iter()
                .map(|note| format!("page {number}: {note}")),
        );
        let media_box = page.media_box;
        let attrs = self.marks.attrs_for(number);
        let annots = self.marks.defer(&mut self.doc, &page.annots);
        self.tree.add_page_with(
            &mut self.doc,
            [media_box.llx, media_box.lly, media_box.urx, media_box.ury],
            filter,
            &content.bytes,
            |d| objects.resources(d),
            |d| {
                if let Some(b) = attrs.crop_box {
                    d.key("CropBox").array(|a| {
                        for coord in [b.llx, b.lly, b.urx, b.ury] {
                            a.real(coord);
                        }
                    });
                }
                if let Some(rotate) = attrs.rotate {
                    d.key("Rotate").int(i64::from(rotate));
                }
                if !annots.is_empty() {
                    d.key("Annots").array(|a| {
                        for id in &annots {
                            a.reference(*id);
                        }
                    });
                }
            },
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

    fn document(&mut self, mark: DocMark) {
        self.marks.record(mark);
    }
}

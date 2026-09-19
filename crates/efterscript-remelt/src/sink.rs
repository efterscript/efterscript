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
//!
//! Parameters arrive as `DocMark::Params` and merge into the sink's
//! [`Params`] at once: a per-page key such as `CompressPages` or the
//! downsampling keys governs the pages written after it, and the
//! finish-time keys (`SubsetFonts`, `CompatibilityLevel`,
//! `EmbedAllFonts`) take the value in force when the document closes.
//! `EmbedAllFonts` also decides, page by page, whether a resident face is
//! deferred to the end at all: a face already written unembedded before
//! the request stays so and is reported.

use std::collections::BTreeMap;
use std::io::{Seek, Write};

use efterscript_graphics::{DocMark, Page, PageSink};
use efterscript_pdf::{Document, Filter, PageTree, write_info};

use efterscript_fonts::ResidentFace;

use crate::downsample::Tally;
use crate::fonts::FontTable;
use crate::marks::Marks;
use crate::params::{NotHonoured, Params};
use crate::{Error, Options, content, resources::Objects};

/// Names the project and its version; the only Info entry a job does
/// not supply, since dates would cost determinism. [`Options::unversioned_producer`]
/// drops the version for output that must not change with a release.
const PRODUCER: &str = concat!("EfterScript ", env!("CARGO_PKG_VERSION"));
const BARE_PRODUCER: &str = "EfterScript";

pub struct PdfSink<W: Write> {
    doc: Document<W>,
    tree: PageTree,
    params: Params,
    versioned_producer: bool,
    pages: usize,
    error: Option<efterscript_pdf::Error>,
    fonts: FontTable,
    notes: Vec<String>,
    marks: Marks,
    /// What the job's requests could not change, in order.
    refused: Vec<NotHonoured>,
    tally: Tally,
}

impl<W: Write> PdfSink<W> {
    /// Starts a document on `out`; the header is written at once and
    /// names 1.7 for good, so a `CompatibilityLevel` below it is reported
    /// rather than written. See [`Self::new_seekable`].
    pub fn new(out: W, options: Options) -> Result<Self, Error> {
        Self::start(Document::new(out)?, options)
    }

    /// As [`Self::new`], for a sink the header can be revised on at
    /// finish, so the file names the `CompatibilityLevel` then in force.
    pub fn new_seekable(out: W, options: Options) -> Result<Self, Error>
    where
        W: Seek,
    {
        Self::start(Document::new_seekable(out)?, options)
    }

    fn start(mut doc: Document<W>, options: Options) -> Result<Self, Error> {
        let tree = PageTree::new(&mut doc);
        Ok(PdfSink {
            doc,
            tree,
            params: options.params,
            versioned_producer: options.versioned_producer,
            pages: 0,
            error: None,
            fonts: FontTable::default(),
            notes: Vec::new(),
            marks: Marks::default(),
            refused: Vec::new(),
            tally: Tally::default(),
        })
    }

    /// The parameters in force now.
    pub fn params(&self) -> &Params {
        &self.params
    }

    /// Every entry not applied so far, followed by what the parameters
    /// as they stand could not do: the resident faces `EmbedAllFonts`
    /// leaves unembedded, one-bit images subsampled where averaging was
    /// asked, and a version other than 1.7 without a seekable sink.
    pub fn not_honoured(&self) -> Vec<NotHonoured> {
        let mut all = self.refused.clone();
        if self.params.embed_all_fonts {
            all.extend(embed_all_refusals(&self.fonts.unembedded));
        }
        if self.tally.mono_subsampled {
            all.push((
                "MonoImageDownsampleType".to_string(),
                "/Average (one-bit images are subsampled)".to_string(),
            ));
        }
        let (major, minor) = self.params.compatibility_level;
        if (major, minor) != (1, 7) && !self.doc.version_patchable() {
            all.push((
                "CompatibilityLevel".to_string(),
                format!("{major}.{minor} (the output cannot be rewound; 1.7 written)"),
            ));
        }
        all
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

    /// Images reduced by downsampling so far.
    pub fn downsampled(&self) -> usize {
        self.tally.images
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
            params,
            ..
        } = self;
        if let Some(e) = error {
            return Err(e.into());
        }
        if doc.version_patchable() {
            let (major, minor) = params.compatibility_level;
            doc.set_version(major, minor)?;
        }
        fonts.embedded.write_all(
            &mut doc,
            filter,
            params.subset_fonts,
            params.embed_all_fonts,
        )?;
        let catalog = marks.write(&mut doc, tree.pages())?;
        let root = tree.finish_with(&mut doc, |d| catalog.entries(d))?;
        let producer = if self.versioned_producer {
            PRODUCER
        } else {
            BARE_PRODUCER
        };
        let info = write_info(&mut doc, |d| {
            marks.info(d);
            d.key("Producer").string(producer.as_bytes());
        })?;
        Ok(doc.finish(root, Some(info))?)
    }

    fn text_filter(&self) -> Filter {
        if self.params.compress_pages {
            Filter::Flate
        } else {
            Filter::None
        }
    }

    fn write_page(&mut self, page: &Page) -> Result<(), efterscript_pdf::Error> {
        let filter = self.text_filter();
        let number = self.pages + 1;
        let mut notes = Vec::new();
        let objects = Objects::write(
            &mut self.doc,
            page,
            filter,
            &self.params,
            &mut self.fonts,
            &mut notes,
            &mut self.tally,
        )?;
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

/// Why each face written unembedded stayed so under `EmbedAllFonts`,
/// one entry per reason naming its faces in order of first use.
fn embed_all_refusals(faces: &[ResidentFace]) -> Vec<NotHonoured> {
    let mut reasons: Vec<(&'static str, Vec<&'static str>)> = Vec::new();
    for face in faces {
        let reason = if face.outline_asset().is_none() {
            "no outline asset"
        } else if !efterscript_fonts::has_resident_outlines() {
            "outline assets absent from this build"
        } else {
            "written before the request"
        };
        match reasons.iter_mut().find(|(r, _)| *r == reason) {
            Some((_, names)) => names.push(face.postscript_name()),
            None => reasons.push((reason, vec![face.postscript_name()])),
        }
    }
    reasons
        .into_iter()
        .map(|(reason, names)| {
            (
                "EmbedAllFonts".to_string(),
                format!("true ({}: {reason})", names.join(", ")),
            )
        })
        .collect()
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
        match mark {
            DocMark::Params(entries) => {
                let refused = self.params.merge(&entries);
                self.refused.extend(refused);
            }
            other => self.marks.record(other),
        }
    }
}

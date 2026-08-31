// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Document-structure convenience: catalog, a flat page tree, and the Info
//! dictionary. Deliberately a sibling module of [`crate::doc`], so the
//! compiler proves this layer touches nothing but the crate's public
//! primitives — the convenience is not privileged.

use std::io::Write;

use crate::doc::{Document, Error};
use crate::obj::{DictBuilder, Ref};
use crate::stream::Filter;

/// Builds a flat page tree: one `Pages` node whose `Kids` are the pages in
/// insertion order. The node's id is allocated up front and written at
/// [`PageTree::finish`], so pages can name their parent before it exists.
pub struct PageTree {
    pages: Ref,
    kids: Vec<Ref>,
}

impl PageTree {
    pub fn new<W: Write>(doc: &mut Document<W>) -> Self {
        PageTree {
            pages: doc.alloc(),
            kids: Vec::new(),
        }
    }

    /// Writes the content stream and a `Page` dictionary; returns the page's
    /// id. `resources` fills the page's `Resources` dictionary.
    pub fn add_page<W: Write>(
        &mut self,
        doc: &mut Document<W>,
        media_box: [f32; 4],
        filter: Filter,
        content: &[u8],
        resources: impl FnOnce(&mut DictBuilder<'_>),
    ) -> Result<Ref, Error> {
        let contents = doc.alloc();
        doc.write_stream(contents, filter, content, |_| {})?;
        let page = doc.alloc();
        let parent = self.pages;
        doc.write_obj(page, |v| {
            v.dict(|d| {
                d.key("Type").name("Page");
                d.key("Parent").reference(parent);
                d.key("MediaBox").array(|a| {
                    for coord in media_box {
                        a.real(coord);
                    }
                });
                d.key("Resources").dict(resources);
                d.key("Contents").reference(contents);
            });
        })?;
        self.kids.push(page);
        Ok(page)
    }

    /// Writes the `Pages` node and the catalog; the returned id is the
    /// document's `Root`.
    pub fn finish<W: Write>(self, doc: &mut Document<W>) -> Result<Ref, Error> {
        doc.write_obj(self.pages, |v| {
            v.dict(|d| {
                d.key("Type").name("Pages");
                d.key("Kids").array(|a| {
                    for kid in &self.kids {
                        a.reference(*kid);
                    }
                });
                d.key("Count").int(self.kids.len() as i64);
            });
        })?;
        let catalog = doc.alloc();
        doc.write_obj(catalog, |v| {
            v.dict(|d| {
                d.key("Type").name("Catalog");
                d.key("Pages").reference(self.pages);
            });
        })?;
        Ok(catalog)
    }
}

/// Writes an Info dictionary (ISO 32000-1 §14.3.3) filled by `f`; pass the
/// returned id to `Document::finish`.
pub fn write_info<W: Write>(
    doc: &mut Document<W>,
    f: impl FnOnce(&mut DictBuilder<'_>),
) -> Result<Ref, Error> {
    let info = doc.alloc();
    doc.write_obj(info, |v| v.dict(f))?;
    Ok(info)
}

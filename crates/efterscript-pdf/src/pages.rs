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
        self.add_page_with(doc, media_box, filter, content, resources, |_| {})
    }

    /// As [`Self::add_page`], with `extra` adding entries to the page
    /// dictionary after the ones every page has (a crop box, a rotation,
    /// annotations).
    pub fn add_page_with<W: Write>(
        &mut self,
        doc: &mut Document<W>,
        media_box: [f32; 4],
        filter: Filter,
        content: &[u8],
        resources: impl FnOnce(&mut DictBuilder<'_>),
        extra: impl FnOnce(&mut DictBuilder<'_>),
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
                extra(d);
            });
        })?;
        self.kids.push(page);
        Ok(page)
    }

    /// The ids of the pages added so far, in order.
    pub fn pages(&self) -> &[Ref] {
        &self.kids
    }

    /// Writes the `Pages` node and the catalog; the returned id is the
    /// document's `Root`.
    pub fn finish<W: Write>(self, doc: &mut Document<W>) -> Result<Ref, Error> {
        self.finish_with(doc, |_| {})
    }

    /// As [`Self::finish`], with `catalog` adding entries to the catalog
    /// after `Type` and `Pages` (outlines, named destinations, viewer
    /// preferences).
    pub fn finish_with<W: Write>(
        self,
        doc: &mut Document<W>,
        catalog: impl FnOnce(&mut DictBuilder<'_>),
    ) -> Result<Ref, Error> {
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
        let root = doc.alloc();
        doc.write_obj(root, |v| {
            v.dict(|d| {
                d.key("Type").name("Catalog");
                d.key("Pages").reference(self.pages);
                catalog(d);
            });
        })?;
        Ok(root)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extra_entries_follow_the_fixed_ones() {
        let mut doc = Document::new(Vec::new()).unwrap();
        let mut tree = PageTree::new(&mut doc);
        let page = tree
            .add_page_with(
                &mut doc,
                [0.0, 0.0, 10.0, 10.0],
                Filter::None,
                b"",
                |_| {},
                |d| d.key("Rotate").int(90),
            )
            .unwrap();
        assert_eq!(tree.pages(), [page]);
        let root = tree
            .finish_with(&mut doc, |d| d.key("PageMode").name("UseOutlines"))
            .unwrap();
        let bytes = doc.finish(root, None).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.contains("/Contents 2 0 R /Rotate 90 >>"), "{text}");
        assert!(
            text.contains("/Pages 1 0 R /PageMode /UseOutlines >>"),
            "{text}"
        );
    }
}

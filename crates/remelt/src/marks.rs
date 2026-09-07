// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The document objects the marks become: link annotations
//! (ISO 32000-1 §12.5.6.5), the outline tree (§12.3.3), named
//! destinations in the catalog's `Dests` dictionary (§12.3.2.3), the
//! open action and viewer modes (§12.2), page crop boxes and rotations,
//! and the information entries (§14.3.3). Marks are collected as they
//! arrive; page attributes are consumed when the page is written, and
//! everything else at finish, when every page has an id. Annotations
//! are deferred to finish too, since a link may lead to a page not yet
//! written; the page dictionary references their ids from the start.

use std::collections::BTreeMap;
use std::io::Write;

use pdf_out::{ArrayBuilder, DictBuilder, Document, Error, Ref};
use ps_graphics::{Annot, DocMark, LinkTarget, PageAttrs, Target, View};

#[derive(Clone, Debug)]
struct Item {
    title: Vec<u8>,
    count: i32,
    target: Option<Target>,
}

/// An outline item with its children, as the count rule nests them.
struct Node {
    id: Ref,
    item: Item,
    children: Vec<Node>,
}

impl Node {
    /// Whether the item shows its children.
    fn open(&self) -> bool {
        self.item.count >= 0
    }

    /// The descendants a viewer shows once this item is open.
    fn visible(&self) -> usize {
        self.children.len()
            + self
                .children
                .iter()
                .filter(|c| c.open())
                .map(Node::visible)
                .sum::<usize>()
    }
}

#[derive(Default)]
pub(crate) struct Marks {
    items: Vec<Item>,
    dests: BTreeMap<Vec<u8>, (usize, View)>,
    info: Vec<(Vec<u8>, Vec<u8>)>,
    page_mode: Option<Vec<u8>>,
    page_layout: Option<Vec<u8>>,
    open: Option<Target>,
    defaults: PageAttrs,
    page_attrs: BTreeMap<usize, PageAttrs>,
    /// Annotations by the id their page dictionary references.
    annots: Vec<(Ref, Annot)>,
    /// Marks honoured: document marks and annotations.
    pub(crate) written: usize,
    /// Marks tolerated and dropped, by kind.
    pub(crate) ignored: BTreeMap<String, usize>,
}

impl Marks {
    pub(crate) fn record(&mut self, mark: DocMark) {
        self.written += 1;
        match mark {
            DocMark::Outline {
                title,
                count,
                target,
            } => self.items.push(Item {
                title,
                count,
                target,
            }),
            DocMark::Dest { name, page, view } => {
                self.dests.insert(name, (page, view));
            }
            DocMark::Info(entries) => {
                for (key, value) in entries {
                    match self.info.iter_mut().find(|(k, _)| *k == key) {
                        Some(slot) => slot.1 = value,
                        None => self.info.push((key, value)),
                    }
                }
            }
            DocMark::View {
                page_mode,
                page_layout,
                open,
            } => {
                self.page_mode = page_mode.or(self.page_mode.take());
                self.page_layout = page_layout.or(self.page_layout.take());
                self.open = open.or(self.open.take());
            }
            DocMark::PagesDefault(attrs) => self.defaults = attrs.over(&self.defaults),
            DocMark::PageAttr { page, attrs } => {
                let entry = self.page_attrs.entry(page).or_default();
                *entry = attrs.over(entry);
            }
            DocMark::Ignored { kind } => {
                self.written -= 1;
                *self
                    .ignored
                    .entry(String::from_utf8_lossy(&kind).into_owned())
                    .or_insert(0) += 1;
            }
        }
    }

    /// The attributes page `number` (from 1) is written with: its own
    /// over the document's defaults.
    pub(crate) fn attrs_for(&self, number: usize) -> PageAttrs {
        self.page_attrs
            .get(&number)
            .map_or_else(|| self.defaults.clone(), |own| own.over(&self.defaults))
    }

    /// Allocates an id per annotation for the page dictionary to name;
    /// the objects are written at finish.
    pub(crate) fn defer<W: Write>(&mut self, doc: &mut Document<W>, annots: &[Annot]) -> Vec<Ref> {
        let ids: Vec<Ref> = annots.iter().map(|_| doc.alloc()).collect();
        self.annots
            .extend(ids.iter().copied().zip(annots.iter().cloned()));
        self.written += annots.len();
        ids
    }

    /// The information entries the marks gave, in first-seen order; the
    /// producer is the writer's own and a job's value for it is dropped.
    pub(crate) fn info(&self, d: &mut DictBuilder<'_>) {
        for (key, value) in &self.info {
            if key != b"Producer" {
                d.key_bytes(key).string(value);
            }
        }
    }

    /// Writes the annotations, the outline tree, and the destinations
    /// dictionary; `pages` are the page ids in order. What the catalog
    /// must reference comes back.
    pub(crate) fn write<W: Write>(
        &self,
        doc: &mut Document<W>,
        pages: &[Ref],
    ) -> Result<Catalog, Error> {
        for (id, annot) in &self.annots {
            write_annot(doc, *id, annot, pages)?;
        }
        let outlines = write_outlines(doc, &self.items, pages)?;
        let dests = if self.dests.is_empty() {
            None
        } else {
            let id = doc.alloc();
            doc.write_obj(id, |v| {
                v.dict(|d| {
                    for (name, (page, view)) in &self.dests {
                        if let Some(page) = page_ref(pages, *page) {
                            d.key_bytes(name).array(|a| dest_array(a, page, view));
                        }
                    }
                });
            })?;
            Some(id)
        };
        Ok(Catalog {
            outlines,
            dests,
            page_mode: self.page_mode.clone(),
            page_layout: self.page_layout.clone(),
            open: self.open.clone(),
            pages: pages.to_vec(),
        })
    }
}

/// What the catalog gains from the marks.
pub(crate) struct Catalog {
    outlines: Option<Ref>,
    dests: Option<Ref>,
    page_mode: Option<Vec<u8>>,
    page_layout: Option<Vec<u8>>,
    open: Option<Target>,
    pages: Vec<Ref>,
}

impl Catalog {
    pub(crate) fn entries(&self, d: &mut DictBuilder<'_>) {
        if let Some(outlines) = self.outlines {
            d.key("Outlines").reference(outlines);
        }
        if let Some(dests) = self.dests {
            d.key("Dests").reference(dests);
        }
        if let Some(mode) = &self.page_mode {
            d.key("PageMode").name_bytes(mode);
        }
        if let Some(layout) = &self.page_layout {
            d.key("PageLayout").name_bytes(layout);
        }
        match &self.open {
            Some(Target::Named(name)) => d.key("OpenAction").name_bytes(name),
            Some(Target::Page { index, view }) => {
                if let Some(page) = page_ref(&self.pages, *index) {
                    d.key("OpenAction").array(|a| dest_array(a, page, view));
                }
            }
            None => {}
        }
    }
}

/// The id of page `number` (from 1), if the document has it.
fn page_ref(pages: &[Ref], number: usize) -> Option<Ref> {
    number.checked_sub(1).and_then(|i| pages.get(i)).copied()
}

fn optional(a: &mut ArrayBuilder<'_>, value: Option<f32>) {
    match value {
        Some(v) => a.real(v),
        None => a.null(),
    };
}

/// An explicit destination array (ISO 32000-1 §12.3.2.2).
fn dest_array(a: &mut ArrayBuilder<'_>, page: Ref, view: &View) {
    a.reference(page);
    match view {
        View::Fit => {
            a.name("Fit");
        }
        View::FitH(top) => {
            a.name("FitH");
            optional(a, *top);
        }
        View::Xyz { left, top, zoom } => {
            a.name("XYZ");
            optional(a, *left);
            optional(a, *top);
            optional(a, *zoom);
        }
    }
}

/// `Dest` or `A` for a target, when it can be expressed.
fn destination(d: &mut DictBuilder<'_>, target: &LinkTarget, pages: &[Ref]) {
    match target {
        LinkTarget::Named(name) => d.key("Dest").name_bytes(name),
        LinkTarget::Uri(uri) => d.key("A").dict(|a| {
            a.key("Type").name("Action");
            a.key("S").name("URI");
            a.key("URI").string(uri);
        }),
        LinkTarget::Page { index, view } => {
            if let Some(page) = page_ref(pages, *index) {
                d.key("Dest").array(|a| dest_array(a, page, view));
            }
        }
    }
}

fn write_annot<W: Write>(
    doc: &mut Document<W>,
    id: Ref,
    annot: &Annot,
    pages: &[Ref],
) -> Result<(), Error> {
    doc.write_obj(id, |v| {
        v.dict(|d| {
            d.key("Type").name("Annot");
            d.key("Subtype").name("Link");
            d.key("Rect").array(|a| {
                for coord in [
                    annot.rect.llx,
                    annot.rect.lly,
                    annot.rect.urx,
                    annot.rect.ury,
                ] {
                    a.real(coord);
                }
            });
            d.key("Border").array(|a| {
                for v in annot.border.unwrap_or([0.0, 0.0, 0.0]) {
                    a.real(v);
                }
            });
            if let Some(color) = &annot.color {
                d.key("C").array(|a| {
                    for v in color {
                        a.real(*v);
                    }
                });
            }
            destination(d, &annot.target, pages);
            if let Some(contents) = &annot.contents {
                d.key("Contents").string(contents);
            }
        });
    })
}

/// Nests the items by the count rule: each item's count names how many
/// of the items that follow are its children, sign aside. Ids are
/// allocated in mark order.
fn nest<W: Write>(
    doc: &mut Document<W>,
    items: &[Item],
    at: &mut usize,
    wanted: usize,
) -> Vec<Node> {
    let mut nodes = Vec::new();
    while nodes.len() < wanted && *at < items.len() {
        let item = items[*at].clone();
        *at += 1;
        let id = doc.alloc();
        let children = nest(doc, items, at, item.count.unsigned_abs() as usize);
        nodes.push(Node { id, item, children });
    }
    nodes
}

fn write_items<W: Write>(
    doc: &mut Document<W>,
    nodes: &[Node],
    parent: Ref,
    pages: &[Ref],
) -> Result<(), Error> {
    for (i, node) in nodes.iter().enumerate() {
        doc.write_obj(node.id, |v| {
            v.dict(|d| {
                d.key("Title").string(&node.item.title);
                d.key("Parent").reference(parent);
                if i > 0 {
                    d.key("Prev").reference(nodes[i - 1].id);
                }
                if let Some(next) = nodes.get(i + 1) {
                    d.key("Next").reference(next.id);
                }
                if let (Some(first), Some(last)) = (node.children.first(), node.children.last()) {
                    d.key("First").reference(first.id);
                    d.key("Last").reference(last.id);
                    let visible = node.visible() as i64;
                    d.key("Count")
                        .int(if node.open() { visible } else { -visible });
                }
                match &node.item.target {
                    Some(Target::Named(name)) => d.key("Dest").name_bytes(name),
                    Some(Target::Page { index, view }) => {
                        if let Some(page) = page_ref(pages, *index) {
                            d.key("Dest").array(|a| dest_array(a, page, view));
                        }
                    }
                    None => {}
                }
            });
        })?;
        write_items(doc, &node.children, node.id, pages)?;
    }
    Ok(())
}

/// The outline tree, root first then the items in mark order; `None`
/// without items.
fn write_outlines<W: Write>(
    doc: &mut Document<W>,
    items: &[Item],
    pages: &[Ref],
) -> Result<Option<Ref>, Error> {
    if items.is_empty() {
        return Ok(None);
    }
    let root = doc.alloc();
    let mut at = 0;
    let nodes = nest(doc, items, &mut at, usize::MAX);
    let visible = nodes.len()
        + nodes
            .iter()
            .filter(|n| n.open())
            .map(Node::visible)
            .sum::<usize>();
    doc.write_obj(root, |v| {
        v.dict(|d| {
            d.key("Type").name("Outlines");
            d.key("First").reference(nodes[0].id);
            d.key("Last").reference(nodes[nodes.len() - 1].id);
            d.key("Count").int(visible as i64);
        });
    })?;
    write_items(doc, &nodes, root, pages)?;
    Ok(Some(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(count: i32) -> Item {
        Item {
            title: Vec::new(),
            count,
            target: None,
        }
    }

    /// Each node's count with its children's counts and grandchildren's.
    type Shape = Vec<(i32, Vec<(i32, Vec<i32>)>)>;

    fn shape(nodes: &[Node]) -> Shape {
        nodes
            .iter()
            .map(|n| {
                (
                    n.item.count,
                    n.children
                        .iter()
                        .map(|c| {
                            (
                                c.item.count,
                                c.children.iter().map(|g| g.item.count).collect(),
                            )
                        })
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn counts_nest_the_following_items_sign_aside() {
        let items = [
            item(2),
            item(0),
            item(-1),
            item(0),
            item(0),
            item(3),
            item(0),
        ];
        let mut doc = Document::new(Vec::new()).unwrap();
        let mut at = 0;
        let nodes = nest(&mut doc, &items, &mut at, usize::MAX);
        assert_eq!(
            shape(&nodes),
            [
                (2, vec![(0, vec![]), (-1, vec![0])]),
                (0, vec![]),
                (3, vec![(0, vec![])]),
            ],
            "a short document ends a branch early"
        );
        assert_eq!(nodes[0].visible(), 2, "the closed grandchild is not shown");
        assert!(!nodes[0].children[1].open());
        assert_eq!(nodes[0].children[1].visible(), 1);
    }

    #[test]
    fn info_merges_by_key_and_view_fields_keep_the_last_value() {
        let mut marks = Marks::default();
        marks.record(DocMark::Info(vec![
            (b"Title".to_vec(), b"A".to_vec()),
            (b"Author".to_vec(), b"B".to_vec()),
        ]));
        marks.record(DocMark::Info(vec![(b"Title".to_vec(), b"C".to_vec())]));
        assert_eq!(
            marks.info,
            [
                (b"Title".to_vec(), b"C".to_vec()),
                (b"Author".to_vec(), b"B".to_vec())
            ]
        );
        marks.record(DocMark::View {
            page_mode: Some(b"UseOutlines".to_vec()),
            page_layout: None,
            open: None,
        });
        marks.record(DocMark::View {
            page_mode: None,
            page_layout: Some(b"OneColumn".to_vec()),
            open: None,
        });
        assert_eq!(marks.page_mode.as_deref(), Some(&b"UseOutlines"[..]));
        assert_eq!(marks.page_layout.as_deref(), Some(&b"OneColumn"[..]));
        marks.record(DocMark::Ignored {
            kind: b"X".to_vec(),
        });
        assert_eq!(marks.written, 4);
        assert_eq!(marks.ignored.get("X"), Some(&1));
    }

    #[test]
    fn page_attributes_layer_over_the_defaults() {
        let mut marks = Marks::default();
        marks.record(DocMark::PagesDefault(PageAttrs {
            crop_box: Some(ps_vm::Bounds::new(0.0, 0.0, 1.0, 1.0)),
            rotate: Some(90),
        }));
        marks.record(DocMark::PageAttr {
            page: 2,
            attrs: PageAttrs {
                crop_box: None,
                rotate: Some(180),
            },
        });
        assert_eq!(marks.attrs_for(1).rotate, Some(90));
        assert_eq!(marks.attrs_for(2).rotate, Some(180));
        assert!(marks.attrs_for(2).crop_box.is_some());
    }
}

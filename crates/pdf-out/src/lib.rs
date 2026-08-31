// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Low-level PDF serialization.
//!
//! Objects, cross-reference tables, streams, compression — and zero PostScript
//! knowledge. Deterministic output for golden testing.
//!
//! Hand-written and zero-dependency end to end, including the Flate
//! container — reuse of an existing writer was considered and declined in
//! favour of self-sufficiency on the product's core path.
//!
//! The API serializes immediately instead of building a value tree: allocate
//! ids with [`Document::alloc`], write each object in any order through
//! closure-scoped builders ([`Val`], [`DictBuilder`], [`ArrayBuilder`]), and
//! close the file with `Document::finish`. [`PageTree`] and [`write_info`]
//! add document structure on top of those primitives.

mod doc;
mod flate;
mod obj;
mod pages;
mod stream;
mod write;
mod xref;

pub use doc::{Document, Error};
pub use obj::{ArrayBuilder, DictBuilder, Ref, Val};
pub use pages::{PageTree, write_info};
pub use stream::Filter;

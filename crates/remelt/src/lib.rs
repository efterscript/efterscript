// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The distillation engine — the v1 product.
//!
//! Drives the VM, consumes the vector IR, applies distillation policies
//! (image recompression/downsampling, font-embedding rules, color strategy —
//! the distillation parameter set, including `setdistillerparams`
//! compatibility), handles `pdfmark` (bookmarks, links, DocInfo), and writes
//! PDF. Vector-preserving, text-preserving (with ToUnicode), color-preserving:
//! nothing is rasterized.
//!
//! Named for the letterpress practice of remelting hellbox lead into fresh
//! type: old type in, clean type out.

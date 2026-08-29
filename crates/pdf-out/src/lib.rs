// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Low-level PDF serialization.
//!
//! Objects, cross-reference tables, streams, compression — and zero PostScript
//! knowledge. Deterministic output for golden testing.
//!
//! Build-vs-reuse decision open: evaluate the `pdf-writer`
//! crate (Typst project) early; keep this layer thin either way.

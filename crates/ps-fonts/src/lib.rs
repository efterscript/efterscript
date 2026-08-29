// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Font layer: parsing, metrics, subsetting, ToUnicode.
//!
//! Memory-safe parsers for Type 1 (eexec + charstrings), CFF/Type 2, and
//! TrueType/Type 42 font programs; metrics for the resident-font substitution
//! table; charstring-to-outline extraction (for `charpath`); glyph-usage
//! tracking and subsetting for PDF embedding; ToUnicode derivation from real
//! encodings and CMaps rather than glyph-name guessing. CID-keyed fonts and
//! CMaps (composite text) are in scope for this layer.
//!
//! The VM owns font-dictionary *semantics*; this crate owns the glyph engine,
//! kept behind a trait. No rasterization.
//!
//! Independently useful for any document tooling.

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
//! What exists so far: the metrics of the fourteen standard fonts
//! ([`StdFont`]) read from the embedded AFM files, the two built-in text
//! encodings, name-level substitution ([`substitute`]), and glyph-name to
//! Unicode mapping ([`unicode`]) through the Adobe Glyph List.
//! Font-program parsing follows.
//!
//! Independently useful for any document tooling.

pub mod afm;
pub mod encoding;
mod glyph_list;
mod resident;
mod substitute;

pub use afm::{Afm, AfmError, CharMetric};
pub use encoding::{Encoding, ISO_LATIN1_ENCODING, STANDARD_ENCODING};
pub use glyph_list::unicode;
pub use resident::{Family, StdFont};
pub use substitute::substitute;

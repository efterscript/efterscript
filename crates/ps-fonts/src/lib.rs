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
//! What exists so far: the resident set — the fourteen standard fonts
//! ([`StdFont`]) and the thirty-five resident faces ([`ResidentFace`])
//! with metrics from the embedded Core 14 AFM files and from tables
//! derived from the outline programs ([`metrics`]), and outlines from the
//! embedded Liberation and TeX Gyre assets ([`outlines`], behind the
//! `resident-outlines` feature) — the two built-in text encodings,
//! name-level substitution ([`substitute`]), glyph-name to Unicode
//! mapping ([`unicode`]) through the Adobe Glyph List, and the glyph
//! engine ([`Program`]): Type 1 charstrings and TrueType glyph tables
//! interpreted into outlines and advances, a reader for Type 1 font
//! files ([`type1::parse_file`]), the subsetters that regenerate a
//! Type 1 program ([`type1::write`]) and rewrite a TrueType one
//! ([`truetype::write::subset`]) for embedding, and the synthesised
//! fonts of [`testing`], and CFF programs with Type 2 charstrings
//! ([`cff`], name- and CID-keyed).
//!
//! Independently useful for any document tooling.

pub mod afm;
pub mod cff;
pub mod encoding;
mod glyph_list;
mod mac_glyphs;
pub mod metrics;
pub mod outline;
pub mod outlines;
mod program;
mod resident;
mod substitute;
pub mod testing;
pub mod truetype;
pub mod type1;

pub use afm::{Afm, AfmError, CharMetric};
pub use cff::CffProgram;
pub use encoding::{Encoding, ISO_LATIN1_ENCODING, STANDARD_ENCODING};
pub use glyph_list::unicode;
pub use mac_glyphs::MAC_GLYPH_NAMES;
pub use metrics::{MetricTable, MetricsError};
pub use outline::{Glyph, Outline, OutlineOp};
pub use outlines::{OutlineAsset, ResidentOutlines};
pub use program::{FontError, Program, ProgramKind};
pub use resident::{Family, Metrics, ResidentFace, StdFont};
pub use substitute::substitute;
pub use truetype::TrueTypeProgram;
pub use type1::Type1Program;

/// Whether this build embeds the resident set's outline assets (the
/// `resident-outlines` feature), and so can outline resident fonts.
pub const fn has_resident_outlines() -> bool {
    cfg!(feature = "resident-outlines")
}

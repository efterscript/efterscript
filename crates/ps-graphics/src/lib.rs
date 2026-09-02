// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Graphics-state machine and vector IR.
//!
//! [`Graphics`] implements the graphics-operator trait exposed by `ps-vm`:
//! it keeps the graphics-state stack (PLRM3 §4.2), builds paths through
//! the CTM, and turns each paint into an operation of a PDF-shaped,
//! resolution-independent page IR ([`Page`], [`IrOp`]) that is delivered
//! to a [`PageSink`] at `showpage`. Colour is a colour-space resource plus
//! a component vector of the space's arity; nothing is converted. The IR
//! has one canonical text form ([`dump`]) for golden comparison.
//!
//! Independently useful as a vector-capture layer: the backend needs no
//! interpreter, only calls.

mod arc;
mod backend;
pub mod dump;
mod ir;
mod real;
mod state;

pub use backend::Graphics;
pub use ir::{
    FillRule, FontIndex, FontSpec, GlyphName, GlyphNames, GlyphProc, Image, ImageRef, IrOp, Op,
    Page, PageSink, Resources, SpaceRef, glyph_names,
};
pub use real::{fmt_real, fmt_reals};
pub use state::{ClipEntry, GState, Path};

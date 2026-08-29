// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Graphics-state machine and vector IR.
//!
//! Implements the graphics-operator trait exposed by `ps-vm`, maintaining
//! graphics state and producing a PDF-shaped, resolution-independent vector IR
//! (a per-page display list carrying font references, N-channel color spaces,
//! and group/soft-mask/blend hooks). Backends consume the IR through sink
//! traits; the v1 sink is the PDF writer, with raster/SVG/GPU slots reserved.
//!
//! Independently useful as a vector-capture layer.

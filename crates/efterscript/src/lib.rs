// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! EfterScript: a memory-safe, embeddable interpreter compatible with the
//! PostScript language, whose primary output is PDF.
//!
//! This crate is the one a user depends on. It defines nothing of its
//! own: [`mod@distill`] is the distillation engine's surface and [`session`]
//! the session library's, and the entry points of both are re-exported
//! here — [`distill()`], [`distill_into`], [`distill_seekable`], and
//! [`Distillation`] turn a whole program into a document; a [`Job`] is
//! fed the program in pieces, as a printer receives it, and answers its
//! queries along the way. The interpreter's own configuration, [`Config`],
//! is what a distillation call takes; the rest of the language virtual
//! machine is reachable as [`vm`] for a host that runs programs without
//! producing PDF.
//!
//! Nothing is rasterised: vectors stay vectors, text stays text, colour
//! spaces are preserved. The library holds no ambient authority — files,
//! the clock, and the output sink exist only as capabilities the embedder
//! injects through the configuration — and contains no unsafe code
//! outside the session library's C interface.

#![forbid(unsafe_code)]

/// The distillation engine: options, parameters, the report, the PDF
/// sink, and the calls that run a program into a document.
pub mod distill {
    pub use efterscript_remelt::*;
}

/// The session library: a job fed in pieces with replies and error
/// reports read back and a PDF at the end, plus its C interface.
pub mod session {
    pub use platen::*;
}

/// The language virtual machine: the interpreter, its configuration and
/// injected capabilities, the object model, and the scanner.
pub mod vm {
    pub use efterscript_vm::*;
}

pub use distill::{Distillation, Options, Params, Report, distill, distill_into, distill_seekable};
pub use session::{Finished, Job, JobConfig, Progress};
pub use vm::Config;

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Property-based PostScript program generation.
//!
//! [`grammar::generate`] writes a program from a profile, a seed, and an
//! index; [`properties::check`] runs it in process against the
//! oracle-free properties and the metamorphic relations; [`shrink`]
//! reduces a failing program. Seed files live in `corpus/generated/`;
//! bulk output stays under the build directory.

pub mod grammar;
pub mod model;
pub mod pdfcheck;
pub mod profile;
pub mod program;
pub mod properties;
pub mod rng;
pub mod runner;
pub mod shrink;

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Byte codecs shared by the PDF writer and the interpreter's filters.
//!
//! [`deflate`] compresses into the RFC 1950 container around RFC 1951
//! blocks; [`inflate`] decodes that format one input byte at a time;
//! [`lzw`] encodes and decodes the variable-width LZW variant of the
//! PostScript language and PDF `LZWDecode` filters; [`predictor`] undoes
//! the PNG row filters and the TIFF horizontal differencing that
//! `Predictor` parameters describe. Everything is hand-written, integer
//! arithmetic only, with no host resources, so it runs anywhere the
//! interpreter does.

#![forbid(unsafe_code)]

pub mod deflate;
pub mod inflate;
pub mod lzw;
pub mod predictor;

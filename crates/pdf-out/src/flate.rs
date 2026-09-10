// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Flate compression for stream data: the encoder lives in the shared
//! `codec` crate, so the interpreter's `FlateDecode` filter and this
//! writer agree on one implementation. Re-exported here so the stream
//! writer's call sites name the format they use.

pub(crate) use codec::deflate::compress;

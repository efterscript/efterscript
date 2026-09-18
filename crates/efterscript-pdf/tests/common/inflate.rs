// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Test-side inflater: the shared codec's inflater with every violation
//! turned into a panic carrying the reason, so a structural test over the
//! writer's output fails with a message.

#![allow(dead_code)]

/// Inflates a complete stream, verifying framing and checksum.
pub fn inflate(z: &[u8]) -> Vec<u8> {
    efterscript_codec::inflate::inflate(z).unwrap_or_else(|e| panic!("inflate: {e}"))
}

/// The block type of every block in `z`, in order: 0 stored, 1 fixed,
/// 2 dynamic. The stream is fully inflated and verified on the way.
pub fn block_kinds(z: &[u8]) -> Vec<u8> {
    efterscript_codec::inflate::block_kinds(z).unwrap_or_else(|e| panic!("inflate: {e}"))
}

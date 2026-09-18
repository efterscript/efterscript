// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Walks arbitrary bytes as a JPEG stream's markers, whole through
//! `stream_len` and one byte at a time, and requires the two to agree:
//! the walk ends where the length says, or both find the input
//! malformed or short. Nothing may panic.

#![no_main]

use efterscript_vm::jpeg::{MarkerWalker, Walk, stream_len};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let len = stream_len(data);
    if let Some(len) = len {
        assert!(0 < len && len <= data.len());
    }
    let mut walker = MarkerWalker::new();
    let mut ended = None;
    for (at, &byte) in data.iter().enumerate() {
        match walker.push(byte) {
            Ok(Walk::More) => assert!(!walker.is_done()),
            Ok(Walk::End) => {
                assert!(walker.is_done());
                ended = Some(at + 1);
                break;
            }
            Err(_) => break,
        }
    }
    assert_eq!(ended, len);
});

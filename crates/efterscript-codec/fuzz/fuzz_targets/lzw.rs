// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Decodes arbitrary bytes as LZW under both `EarlyChange` values, whole
//! and one byte at a time, and requires the two to agree. Any input must
//! end in output or an `Error`, never a panic.

#![no_main]

use efterscript_codec::lzw::{Decoder, Status, decode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for early_change in [false, true] {
        let whole = decode(data, early_change);
        let mut decoder = Decoder::new(early_change);
        let mut out = Vec::new();
        let mut failed = false;
        for &byte in data {
            match decoder.push(byte, &mut out) {
                Ok(Status::More) => {}
                Ok(Status::Done) => {
                    assert!(decoder.is_done());
                    break;
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
        }
        match whole {
            Ok(whole) => {
                assert!(!failed);
                assert_eq!(out, whole);
            }
            Err(_) => assert!(failed),
        }
    }
});

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Inflates arbitrary bytes twice — whole, and one byte at a time
//! stopping at the trailer — and requires the two to agree. Any input
//! must end in output or an `Error`, never a panic.

#![no_main]

use efterscript_codec::inflate::{Inflater, Status, inflate};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let whole = inflate(data);
    let mut inflater = Inflater::new();
    let mut out = Vec::new();
    let mut done = false;
    let mut failed = false;
    for &byte in data {
        match inflater.push(byte, &mut out) {
            Ok(Status::More) => {}
            Ok(Status::Done) => {
                done = true;
                break;
            }
            Err(_) => {
                failed = true;
                break;
            }
        }
    }
    if let Ok(whole) = whole {
        assert!(done && !failed);
        assert!(inflater.is_done());
        assert_eq!(out, whole);
    }
});

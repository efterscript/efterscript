// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The first four bytes choose the parameters — a predictor from the
//! allowed set (or, for `0xFF`, one outside it), `Colors` 1–4,
//! `BitsPerComponent` from the allowed set, `Columns` 1–256 — and the
//! rest is fed through the predictor a byte at a time and flushed. The
//! output can never exceed the input, and nothing may panic.

#![no_main]

use efterscript_codec::predictor::Predictor;
use libfuzzer_sys::fuzz_target;

const PREDICTORS: [i64; 8] = [1, 2, 10, 11, 12, 13, 14, 15];
const BITS: [i64; 5] = [1, 2, 4, 8, 16];

fuzz_target!(|data: &[u8]| {
    let Some(&[p, c, b, n]) = data.get(..4) else {
        return;
    };
    let predictor = if p == 0xFF {
        3
    } else {
        PREDICTORS[usize::from(p % 8)]
    };
    let colors = if c == 0xFF { 0 } else { i64::from(c % 4) + 1 };
    let bits = if b == 0xFF {
        3
    } else {
        BITS[usize::from(b % 5)]
    };
    let columns = if n == 0xFF { 0 } else { i64::from(n) + 1 };
    let Ok(Some(mut predictor)) = Predictor::new(predictor, colors, bits, columns) else {
        return;
    };
    let rows = &data[4..];
    let mut out = Vec::new();
    for &byte in rows {
        predictor.push(byte, &mut out);
    }
    predictor.flush(&mut out);
    assert!(out.len() <= rows.len());
});

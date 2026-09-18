// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! A decode chain of one or two filters, chosen with their parameters
//! by the leading bytes — some parameter values fall outside their
//! ranges, so the `rangecheck` path is covered too — over the remaining
//! bytes, through the helper the `fuzzing` feature exposes. Every
//! outcome but a panic is acceptable.
//!
//! The layout, which `cargo xtask fuzz-smoke`'s seeds follow: one byte
//! whose low bit adds a second filter, then per filter a kind byte
//! (modulo 7: hex, base-85, run-length, Flate, LZW, sub-file, eexec)
//! followed by its parameters — Flate four predictor bytes as in the
//! codec crate's predictor target; LZW an `EarlyChange` byte and the
//! four; sub-file a count byte, a length byte, and that many pattern
//! bytes — and the data after the last filter.

#![no_main]

use efterscript_vm::fuzzing::{Filter, PredictorParams, decode_chain};
use libfuzzer_sys::fuzz_target;

const PREDICTORS: [i64; 8] = [1, 2, 10, 11, 12, 13, 14, 15];
const BITS: [i64; 5] = [1, 2, 4, 8, 16];

/// A reader over the leading bytes; exhausted, it answers zero.
struct Spec<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Spec<'a> {
    fn byte(&mut self) -> u8 {
        let byte = self.bytes.get(self.at).copied().unwrap_or(0);
        self.at += 1;
        byte
    }

    fn take(&mut self, n: usize) -> &'a [u8] {
        let start = self.at.min(self.bytes.len());
        let end = (self.at + n).min(self.bytes.len());
        self.at += n;
        &self.bytes[start..end]
    }

    fn rest(&self) -> &'a [u8] {
        &self.bytes[self.at.min(self.bytes.len())..]
    }

    fn predictor(&mut self) -> PredictorParams {
        let [p, c, b, n] = [self.byte(), self.byte(), self.byte(), self.byte()];
        PredictorParams {
            predictor: if p == 0xFF {
                3
            } else {
                PREDICTORS[usize::from(p % 8)]
            },
            colors: if c == 0xFF { 0 } else { i64::from(c % 4) + 1 },
            bits_per_component: if b == 0xFF {
                3
            } else {
                BITS[usize::from(b % 5)]
            },
            columns: if n == 0xFF { 0 } else { i64::from(n) + 1 },
        }
    }

    fn filter(&mut self) -> Filter {
        match self.byte() % 7 {
            0 => Filter::AsciiHex,
            1 => Filter::Ascii85,
            2 => Filter::RunLength,
            3 => Filter::Flate(self.predictor()),
            4 => {
                let early_change = self.byte() & 1 == 1;
                Filter::Lzw {
                    early_change,
                    predictor: self.predictor(),
                }
            }
            5 => {
                let count = usize::from(self.byte());
                let len = usize::from(self.byte() % 4);
                Filter::SubFile {
                    count,
                    pattern: self.take(len).to_vec(),
                }
            }
            _ => Filter::Eexec,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut spec = Spec { bytes: data, at: 0 };
    let count = usize::from(spec.byte() & 1) + 1;
    let filters: Vec<Filter> = (0..count).map(|_| spec.filter()).collect();
    let _ = decode_chain(&filters, spec.rest());
});

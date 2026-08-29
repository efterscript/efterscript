// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Differential test harness (charter §7.2).
//!
//! Compares produced output against golden references semantically —
//! parse-and-compare PDF content — never byte-exact. Verdicts: pass, fail,
//! expected divergence (recorded as an OpenSpec delta).
//!
//! Private tier: activates when `EFTERSCRIPT_HELLBOX` points at a checkout of
//! the encumbered vault; skips with a clear message when unset (§7.3).

fn main() {
    eprintln!("difftest: not yet implemented (pre-implementation scaffold)");
    std::process::exit(2);
}

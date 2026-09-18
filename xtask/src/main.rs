// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Workspace automation, invoked as `cargo xtask <task>`.

mod check_wasm;
mod fetch_fonts;
mod fuzz_round;
mod fuzz_smoke;
mod lint_strings;
mod parse_survival;
mod sha256;
mod tiny_jpeg;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: cargo xtask <task>");
    eprintln!("tasks:");
    eprintln!("  parse-survival    scan every corpus .ps file (and the private tier if set)");
    eprintln!(
        "  fetch-fonts       download and audit the outline and CMap assets [--check] [--force]"
    );
    eprintln!("                    or extract the OpenType test font into target/ [--test-assets]");
    eprintln!("  lint-strings      scan tracked text files for the vault's denylisted strings");
    eprintln!(
        "  fuzz-round        generate and check the seed files' programs [--profile <p>] [--oracle <name>]"
    );
    eprintln!(
        "  fuzz-smoke        check the libFuzzer crates on stable and run each target briefly"
    );
    eprintln!("                    under nightly cargo-fuzz when installed [--seconds <n>]");
    eprintln!(
        "  check-wasm        cargo check the session front-end for wasm32-unknown-emscripten"
    );
    eprintln!("  tiny-jpeg         print the project's own baseline JPEG stream [--corpus]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parse-survival") => parse_survival::run(&args[1..]),
        Some("fetch-fonts") => fetch_fonts::run(&args[1..]),
        Some("lint-strings") => lint_strings::run(&args[1..]),
        Some("fuzz-round") => fuzz_round::run(&args[1..]),
        Some("fuzz-smoke") => fuzz_smoke::run(&args[1..]),
        Some("check-wasm") => check_wasm::run(&args[1..]),
        Some("tiny-jpeg") => tiny_jpeg::run(&args[1..]),
        Some(t) => {
            eprintln!("xtask: unknown task `{t}`");
            usage()
        }
        None => usage(),
    }
}

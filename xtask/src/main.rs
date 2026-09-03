// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Workspace automation, invoked as `cargo xtask <task>`.

mod fetch_fonts;
mod parse_survival;
mod sha256;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: cargo xtask <task>");
    eprintln!("tasks:");
    eprintln!("  parse-survival    scan every corpus .ps file (and the private tier if set)");
    eprintln!(
        "  fetch-fonts       download and audit the resident set's outline assets [--check] [--force]"
    );
    eprintln!("                    or extract the OpenType test font into target/ [--test-assets]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parse-survival") => parse_survival::run(&args[1..]),
        Some("fetch-fonts") => fetch_fonts::run(&args[1..]),
        Some(t) => {
            eprintln!("xtask: unknown task `{t}`");
            usage()
        }
        None => usage(),
    }
}

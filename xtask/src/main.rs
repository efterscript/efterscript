// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Workspace automation, invoked as `cargo xtask <task>`.

mod parse_survival;

use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("usage: cargo xtask <task>");
    eprintln!("tasks:");
    eprintln!("  parse-survival    scan every corpus .ps file (and the private tier if set)");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("parse-survival") => parse_survival::run(&args[1..]),
        Some(t) => {
            eprintln!("xtask: unknown task `{t}`");
            usage()
        }
        None => usage(),
    }
}

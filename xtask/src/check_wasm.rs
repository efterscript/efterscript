// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask check-wasm`: `cargo check -p platen --target
//! wasm32-unknown-emscripten`, the build-target gate the session
//! front-end promises (its archive links into an Emscripten program, so
//! it and every crate under it must compile for that target). The Rust
//! target must be installed (`rustup target add wasm32-unknown-emscripten`);
//! the Emscripten toolchain itself is not needed for a check.

use std::process::{Command, ExitCode};

pub const TARGET: &str = "wasm32-unknown-emscripten";

pub fn run(args: &[String]) -> ExitCode {
    if !args.is_empty() {
        eprintln!("usage: cargo xtask check-wasm");
        return ExitCode::from(2);
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let status = Command::new(cargo)
        .args(["check", "-p", "platen", "--target", TARGET])
        .status();
    match status {
        Ok(status) if status.success() => {
            println!("check-wasm: platen and its dependencies compile for {TARGET}");
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("check-wasm: the check failed for {TARGET}");
            eprintln!("check-wasm: is the target installed? `rustup target add {TARGET}`");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("check-wasm: cannot run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Workspace automation, invoked as `cargo xtask <task>`.

fn main() {
    let task = std::env::args().nth(1);
    match task.as_deref() {
        Some(t) => {
            eprintln!("xtask: unknown task `{t}` (no tasks defined yet)");
            std::process::exit(2);
        }
        None => {
            eprintln!("usage: cargo xtask <task>");
            std::process::exit(2);
        }
    }
}

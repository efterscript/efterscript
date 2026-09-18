// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The library and everything under it use no threads, no file system,
//! and no clock, so the archive links into an Emscripten program
//! without host shims: a scan of the workspace crates `platen` depends
//! on (its dependency closure has no external crates). Comment lines
//! are skipped; a test module is library source too, since it must
//! compile for the target.

use std::path::{Path, PathBuf};

const CRATES: &[&str] = &[
    "efterscript-codec",
    "efterscript-vm",
    "efterscript-fonts",
    "efterscript-graphics",
    "efterscript-pdf",
    "efterscript-remelt",
    "efterscript-platen",
];

const FORBIDDEN: &[&str] = &[
    "std::thread",
    "std::fs::",
    "std::time",
    "SystemTime",
    "Instant",
];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_threads_file_system_or_clock_in_the_library_closure() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for name in CRATES {
        sources(&root.join("crates").join(name).join("src"), &mut files);
    }
    assert!(files.len() > 20, "the scan found the sources");
    let mut hits = Vec::new();
    for path in &files {
        let text = std::fs::read_to_string(path).unwrap();
        for (number, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            for needle in FORBIDDEN {
                if line.contains(needle) {
                    hits.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        hits.is_empty(),
        "host resources in library code:\n{}",
        hits.join("\n")
    );
}

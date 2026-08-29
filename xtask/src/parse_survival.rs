// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask parse-survival`: scans every `.ps` file under `corpus/` and,
//! when `EFTERSCRIPT_HELLBOX` names an existing checkout, every `.ps` under
//! its `corpora/` directory. Prints one line per file and a histogram of
//! error kinds. A failure in the public corpus is a failure of the run; the
//! private tier is reported only.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ps_vm::{Memory, ScanError, line_of, scan_all};

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok {
        tokens: usize,
    },
    Error {
        kind: String,
        line: usize,
        error: ScanError,
    },
}

/// Scans `bytes` from a fresh VM with no immediate-name definitions.
pub fn scan(bytes: &[u8]) -> Outcome {
    let mut memory = Memory::new();
    match scan_all(bytes, &mut memory, &mut ()) {
        Ok(tokens) => Outcome::Ok {
            tokens: tokens.len(),
        },
        Err(error) => Outcome::Error {
            kind: format!("{:?}", error.kind),
            line: line_of(bytes, error.span.start),
            error,
        },
    }
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect(&path, out);
        } else if path.extension().is_some_and(|e| e == "ps") {
            out.push(path);
        }
    }
}

/// Every `.ps` file below `root`, in path order.
pub fn ps_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect(root, &mut files);
    files
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

struct Tier {
    label: &'static str,
    files: Vec<PathBuf>,
    failures: usize,
}

fn run_tier(label: &'static str, root: &Path, histogram: &mut BTreeMap<String, usize>) -> Tier {
    let files = ps_files(root);
    let mut failures = 0;
    for path in &files {
        let shown = path.strip_prefix(root).unwrap_or(path).display();
        match std::fs::read(path) {
            Ok(bytes) => match scan(&bytes) {
                Outcome::Ok { tokens } => println!("ok    {label}/{shown} ({tokens} tokens)"),
                Outcome::Error { kind, line, error } => {
                    failures += 1;
                    *histogram.entry(kind).or_default() += 1;
                    println!("ERROR {label}/{shown}: {error}, line {line}");
                }
            },
            Err(e) => {
                failures += 1;
                *histogram.entry("unreadable".to_string()).or_default() += 1;
                println!("ERROR {label}/{shown}: {e}");
            }
        }
    }
    Tier {
        label,
        files,
        failures,
    }
}

pub fn run(args: &[String]) -> ExitCode {
    if let Some(arg) = args.first() {
        eprintln!("parse-survival takes no arguments (got `{arg}`)");
        return ExitCode::from(2);
    }
    let root = workspace_root();
    let mut histogram = BTreeMap::new();
    let mut tiers = vec![run_tier("corpus", &root.join("corpus"), &mut histogram)];

    match std::env::var_os("EFTERSCRIPT_HELLBOX") {
        Some(hellbox) => {
            let corpora = Path::new(&hellbox).join("corpora");
            if corpora.is_dir() {
                tiers.push(run_tier("hellbox", &corpora, &mut histogram));
            } else {
                println!(
                    "private tier skipped: {} is not a directory",
                    corpora.display()
                );
            }
        }
        None => println!("private tier skipped: EFTERSCRIPT_HELLBOX unset"),
    }

    println!();
    for tier in &tiers {
        println!(
            "{}: {} files, {} failed",
            tier.label,
            tier.files.len(),
            tier.failures
        );
    }
    if histogram.is_empty() {
        println!("errors: none");
    } else {
        println!("errors by kind:");
        for (kind, count) in &histogram {
            println!("  {count:6}  {kind}");
        }
    }
    if tiers[0].failures > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outcomes_classify_input() {
        assert_eq!(scan(b"1 (a) {b}"), Outcome::Ok { tokens: 3 });
        assert_eq!(scan(b""), Outcome::Ok { tokens: 0 });
        match scan(b"ok\n\n(unterminated") {
            Outcome::Error { kind, line, .. } => {
                assert_eq!(kind, "SyntaxError");
                assert_eq!(line, 3);
            }
            other => panic!("{other:?}"),
        }
        match scan(b"\x80") {
            Outcome::Error { kind, .. } => assert_eq!(kind, "BinaryEncoding"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn the_public_corpus_scans_clean() {
        let files = ps_files(&workspace_root().join("corpus"));
        assert!(!files.is_empty());
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            let outcome = scan(&bytes);
            assert!(
                matches!(outcome, Outcome::Ok { .. }),
                "{}: {outcome:?}",
                path.display()
            );
        }
    }

    #[test]
    fn missing_directories_yield_no_files() {
        assert!(ps_files(Path::new("/nonexistent/efterscript")).is_empty());
    }
}

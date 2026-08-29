// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! Differential test harness.
//!
//! `difftest run [path…]` executes every `.ps` file under the given paths
//! (default: `corpus/unit`) in a fresh interpreter with capture streams and
//! checks the file's own declarations:
//!
//! - `% expect-output: text` — one line of standard output; several such
//!   lines concatenate in order, each ending in a newline. `\n`, `\t`,
//!   `\r`, and `\\` escapes are honoured. No such line means no output.
//! - `% expect-error: name` — the job must end in that uncaught error;
//!   absent, the job must end normally.
//!
//! Declarations are read from the leading comment block only. Golden-output
//! comparison of distilled PDFs (semantic, never byte-exact) and the private
//! tier under `EFTERSCRIPT_HELLBOX` are later additions to this tool.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ps_vm::{Config, Interp, Io, Outcome, SliceSource};

/// What a corpus file declares about its own run.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Expectation {
    pub output: String,
    pub error: Option<String>,
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

/// Parses the declarations in the leading comment block of a program.
pub fn expectation(program: &str) -> Expectation {
    let mut lines = Vec::new();
    let mut error = None;
    for line in program.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        if !line.starts_with('%') {
            break;
        }
        if let Some(rest) = line.strip_prefix("% expect-output:") {
            lines.push(unescape(rest.strip_prefix(' ').unwrap_or(rest)));
        } else if let Some(rest) = line.strip_prefix("% expect-error:") {
            error = Some(rest.trim().to_string());
        }
    }
    let mut output = lines.join("\n");
    if !lines.is_empty() {
        output.push('\n');
    }
    Expectation { output, error }
}

/// What actually happened when a program ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Actual {
    pub output: String,
    pub error: Option<String>,
    pub stderr: String,
}

pub fn execute(program: &[u8]) -> Actual {
    let (io, out, err) = Io::capture();
    let config = Config {
        io,
        ..Default::default()
    };
    let mut interp = Interp::with_config(config);
    let outcome = interp.run(&mut SliceSource::new(program));
    Actual {
        output: out.text(),
        error: match outcome {
            Outcome::Error(summary) => Some(summary.name),
            Outcome::Ok => None,
            Outcome::Suspended => Some("<suspended>".to_string()),
        },
        stderr: err.text(),
    }
}

/// The lines of a failure report, empty when the run met its declarations.
pub fn report(expected: &Expectation, actual: &Actual) -> Vec<String> {
    let mut lines = Vec::new();
    if expected.error != actual.error {
        let show = |e: &Option<String>| e.clone().unwrap_or_else(|| "ok".to_string());
        lines.push(format!(
            "  outcome: expected {}, got {}",
            show(&expected.error),
            show(&actual.error)
        ));
        if !actual.stderr.is_empty() {
            for line in actual.stderr.lines() {
                lines.push(format!("  stderr: {line}"));
            }
        }
    }
    if expected.output != actual.output {
        lines.push("  output differs (- expected, + actual):".to_string());
        let want: Vec<&str> = expected.output.lines().collect();
        let got: Vec<&str> = actual.output.lines().collect();
        for k in 0..want.len().max(got.len()) {
            match (want.get(k), got.get(k)) {
                (Some(w), Some(g)) if w == g => lines.push(format!("    {w}")),
                (w, g) => {
                    if let Some(w) = w {
                        lines.push(format!("  - {w}"));
                    }
                    if let Some(g) = g {
                        lines.push(format!("  + {g}"));
                    }
                }
            }
        }
        if !expected.output.is_empty()
            && !expected.output.ends_with('\n') != !actual.output.ends_with('\n')
        {
            lines.push("  (trailing newline differs)".to_string());
        }
    }
    lines
}

fn collect(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for entry in entries {
            collect(&entry, out);
        }
    } else if path.extension().is_some_and(|e| e == "ps") {
        out.push(path.to_path_buf());
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("difftest lives two levels below the workspace root")
        .to_path_buf()
}

fn run(paths: &[String]) -> ExitCode {
    let root = workspace_root();
    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![root.join("corpus").join("unit")]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };
    let mut files = Vec::new();
    for path in &roots {
        collect(path, &mut files);
    }
    let mut failures = 0;
    for path in &files {
        let shown = path.strip_prefix(&root).unwrap_or(path).display();
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                failures += 1;
                println!("FAIL  {shown}\n  unreadable: {e}");
                continue;
            }
        };
        let expected = expectation(&String::from_utf8_lossy(&bytes));
        let actual = execute(&bytes);
        let lines = report(&expected, &actual);
        if lines.is_empty() {
            println!("ok    {shown}");
        } else {
            failures += 1;
            println!("FAIL  {shown}");
            for line in lines {
                println!("{line}");
            }
        }
    }
    println!();
    println!(
        "{} files, {} passed, {} failed",
        files.len(),
        files.len() - failures,
        failures
    );
    if failures > 0 || files.is_empty() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: difftest run [path…]    (default: corpus/unit)");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("run") => run(&args[1..]),
        _ => usage(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_declarations_are_parsed() {
        let e = expectation(
            "%!PS\n% SPDX-License-Identifier: MIT\n% expect-output: a\n\n% expect-output: b\\nc\n% expect-error: typecheck\n1 =\n% expect-output: ignored\n",
        );
        assert_eq!(e.output, "a\nb\nc\n");
        assert_eq!(e.error.as_deref(), Some("typecheck"));
        assert_eq!(expectation("1 ="), Expectation::default());
        assert_eq!(expectation("% expect-output:\n").output, "\n");
    }

    #[test]
    fn escapes_are_honoured() {
        assert_eq!(unescape("a\\tb\\\\c\\q\\"), "a\tb\\c\\q\\");
    }

    #[test]
    fn execution_captures_output_and_outcome() {
        let ok = execute(b"1 2 add =");
        assert_eq!(ok.output, "3\n");
        assert_eq!(ok.error, None);
        let failed = execute(b"(x) = 1 0 div");
        assert_eq!(failed.output, "x\n");
        assert_eq!(failed.error.as_deref(), Some("undefinedresult"));
        assert!(failed.stderr.contains("OffendingCommand: div"));
    }

    #[test]
    fn reports_name_every_difference() {
        let expected = expectation("% expect-output: 3\n");
        assert!(report(&expected, &execute(b"1 2 add =")).is_empty());
        let lines = report(&expected, &execute(b"4 = 1 0 div"));
        assert!(lines[0].contains("expected ok, got undefinedresult"));
        assert!(lines.iter().any(|l| l == "  - 3"));
        assert!(lines.iter().any(|l| l == "  + 4"));
    }

    #[test]
    fn the_public_corpus_passes() {
        let mut files = Vec::new();
        collect(&workspace_root().join("corpus").join("unit"), &mut files);
        assert!(!files.is_empty());
        for path in files {
            let bytes = std::fs::read(&path).unwrap();
            let expected = expectation(&String::from_utf8_lossy(&bytes));
            let lines = report(&expected, &execute(&bytes));
            assert!(
                lines.is_empty(),
                "{}:\n{}",
                path.display(),
                lines.join("\n")
            );
        }
    }
}

// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The execution budget from the command line: a runaway program ends
//! by itself with status 3 and a report, `--budget` raises or removes
//! the allowance in every mode, `pdf` still writes the document, and a
//! bad value is a usage error.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn efterscript() -> Command {
    Command::new(env!("CARGO_BIN_EXE_efterscript"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn program(dir: &Path, name: &str, body: &str) -> PathBuf {
    let input = dir.join(name);
    std::fs::write(&input, format!("%!PS\n{body}\n")).unwrap();
    input
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// The report line for an allowance, as the tool prints it.
fn report(allowance: &str) -> String {
    format!(
        "%%[ Budget: spent after {allowance} objects; --budget <n> raises it, --budget unlimited removes it ]%%\n"
    )
}

/// Runs `{ } loop` with the given line and asserts it ended by the
/// budget within `bound`.
fn runaway(dir: &Path, args: &[&str], allowance: &str, bound: Duration) {
    let input = program(dir, "loop.ps", "{ } loop");
    let started = Instant::now();
    let output = efterscript().args(args).arg(&input).output().unwrap();
    let elapsed = started.elapsed();
    assert!(elapsed < bound, "took {elapsed:?}");
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = stderr_of(&output);
    // The interpreter's own report of the limitcheck comes first, then
    // the tool's line saying whose limit it was.
    assert!(stderr.starts_with("%%[ Error: limitcheck;"), "{stderr}");
    assert!(stderr.ends_with(&report(allowance)), "{stderr}");
}

// The runaway scenario, at an allowance a debug build spends in a
// second or two.
#[test]
fn a_runaway_program_ends_with_status_three_and_the_report() {
    let dir = scratch("budget-runaway");
    runaway(
        &dir,
        &["run", "--budget", "1000000"],
        "1000000",
        Duration::from_secs(60),
    );
}

// The same scenario under the default allowance. A debug build spends
// 100 million objects in about two minutes, a release build in seconds,
// so this one runs on request.
#[test]
#[ignore = "spends the default allowance, minutes in a debug build; run with --ignored"]
fn a_runaway_program_ends_under_the_default_budget() {
    let dir = scratch("budget-runaway-default");
    runaway(&dir, &["run"], "100000000", Duration::from_secs(600));
}

// The adjustable scenario: a few thousand objects pass under the default
// and fail under a thousand; `unlimited` is accepted.
#[test]
fn the_budget_is_adjustable_and_removable() {
    let dir = scratch("budget-adjustable");
    let input = program(&dir, "counted.ps", "0 1 5000 { pop } for (done) =");

    let output = efterscript()
        .args(["run", "--budget", "1000"])
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(stderr_of(&output).ends_with(&report("1000")), "{output:?}");
    assert!(output.stdout.is_empty());

    let output = efterscript().arg("run").arg(&input).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"done\n");
    assert!(output.stderr.is_empty());

    // The flag may stand anywhere on the line.
    let output = efterscript()
        .arg("run")
        .arg(&input)
        .args(["--budget", "unlimited"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"done\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn a_bad_budget_is_a_usage_error() {
    let dir = scratch("budget-usage");
    let input = program(&dir, "short.ps", "(hi) =");
    for value in ["0", "x", "-1", "1.5", "unlimited-ish"] {
        let output = efterscript()
            .args(["run", "--budget", value])
            .arg(&input)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "{value}: {output:?}");
        let stderr = stderr_of(&output);
        assert!(stderr.starts_with("efterscript: --budget"), "{stderr}");
        assert!(stderr.contains("usage:"), "{stderr}");
        assert!(output.stdout.is_empty());
    }
    let output = efterscript()
        .arg("run")
        .arg(&input)
        .arg("--budget")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(stderr_of(&output).starts_with("efterscript: --budget needs a value"));

    // The usage text names the flag and the default.
    let output = efterscript().output().unwrap();
    assert_eq!(output.status.code(), Some(2));
    let stderr = stderr_of(&output);
    assert!(stderr.contains("--budget <n>|unlimited"), "{stderr}");
    assert!(stderr.contains("default 100000000"), "{stderr}");
}

// The document written so far survives the budget, as it does an error.
#[test]
fn pdf_writes_the_document_before_exiting_three() {
    let dir = scratch("budget-pdf");
    let input = program(
        &dir,
        "page-then-loop.ps",
        "10 10 100 100 rectfill showpage { } loop",
    );
    let out = dir.join("page-then-loop.pdf");
    let output = efterscript()
        .args(["pdf", "--budget", "100000"])
        .arg(&input)
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr_of(&output).ends_with(&report("100000")),
        "{output:?}"
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-1."));
    assert!(bytes.ends_with(b"%%EOF\n"));
    assert!(String::from_utf8_lossy(&bytes).contains("/Count 1"));

    // Under the budget the same line is ordinary.
    let short = program(&dir, "page.ps", "10 10 100 100 rectfill showpage");
    let output = efterscript()
        .args(["pdf", "--budget", "100000"])
        .arg(&short)
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
}

#[test]
fn ir_takes_the_flag_too() {
    let dir = scratch("budget-ir");
    let short = program(&dir, "page.ps", "10 10 100 100 rectfill showpage");
    let output = efterscript()
        .args(["ir", "--budget", "100000"])
        .arg(&short)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!output.stdout.is_empty());
    assert!(output.stderr.is_empty());

    let output = efterscript()
        .args(["--budget", "unlimited", "ir"])
        .arg(&short)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    let looping = program(&dir, "loop.ps", "10 10 100 100 rectfill showpage { } loop");
    let output = efterscript()
        .args(["ir", "--budget", "100000"])
        .arg(&looping)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(
        stderr_of(&output).ends_with(&report("100000")),
        "{output:?}"
    );
    // The dump of the page produced before the budget ran out still
    // reaches standard output.
    assert!(!output.stdout.is_empty());
}

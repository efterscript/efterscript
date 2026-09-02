// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `efterscript pdf` end to end: output naming, stream routing, and exit
//! status, on corpus files copied into the test's scratch directory.

use std::path::{Path, PathBuf};
use std::process::Command;

fn efterscript() -> Command {
    Command::new(env!("CARGO_BIN_EXE_efterscript"))
}

fn corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/unit/graphics")
        .join(name)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn looks_like_a_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF-1.7\n") && bytes.ends_with(b"%%EOF\n")
}

#[test]
fn the_default_output_sits_beside_the_input() {
    let dir = scratch("default-output");
    let input = dir.join("stroked-line.ps");
    std::fs::copy(corpus("stroked-line.ps"), &input).unwrap();
    let expected = dir.join("stroked-line.pdf");
    let _ = std::fs::remove_file(&expected);

    let output = efterscript().arg("pdf").arg(&input).output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(looks_like_a_pdf(&std::fs::read(&expected).unwrap()));
}

#[test]
fn a_named_output_keeps_program_output_on_stdout() {
    let out = scratch("named-output").join("separation.pdf");
    let output = efterscript()
        .arg("pdf")
        .arg(corpus("separation-survives.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"0.6\n");
    assert!(looks_like_a_pdf(&std::fs::read(&out).unwrap()));
}

#[test]
fn a_dash_sends_the_pdf_to_stdout_and_program_output_to_stderr() {
    let output = efterscript()
        .arg("pdf")
        .arg(corpus("separation-survives.ps"))
        .arg("-")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(looks_like_a_pdf(&output.stdout));
    assert_eq!(output.stderr, b"0.6\n");
}

#[test]
fn an_error_outcome_exits_one_after_writing_the_document() {
    let out = scratch("error-outcome").join("error-after-page.pdf");
    let output = efterscript()
        .arg("pdf")
        .arg(corpus("error-after-page.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("undefined"));
    let bytes = std::fs::read(&out).unwrap();
    assert!(looks_like_a_pdf(&bytes));
    assert!(String::from_utf8_lossy(&bytes).contains("/Count 1"));
}

#[test]
fn host_failures_exit_two() {
    let missing = efterscript()
        .arg("pdf")
        .arg(scratch("missing").join("no-such-file.ps"))
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(2));
    let unwritable = efterscript()
        .arg("pdf")
        .arg(corpus("stroked-line.ps"))
        .arg(scratch("unwritable").join("no-such-dir").join("out.pdf"))
        .output()
        .unwrap();
    assert_eq!(unwritable.status.code(), Some(2));
    let usage = efterscript()
        .args(["pdf", "a.ps", "b.pdf", "extra"])
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(2));
}

fn text_corpus(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/unit/text")
        .join(name)
}

#[test]
fn substituted_fonts_are_reported_on_stderr_in_one_line() {
    let out = scratch("substitution").join("arial.pdf");
    let output = efterscript()
        .arg("pdf")
        .arg(text_corpus("substitution-arial.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "efterscript: 1 font substituted: Arial -> Helvetica\n"
    );
    assert!(looks_like_a_pdf(&std::fs::read(&out).unwrap()));
    let quiet = efterscript()
        .arg("pdf")
        .arg(text_corpus("text-operation-shape.ps"))
        .arg(scratch("substitution").join("hi.pdf"))
        .output()
        .unwrap();
    assert!(quiet.status.success());
    assert!(quiet.stderr.is_empty());
}

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
    bytes.starts_with(b"%PDF-1.") && bytes.ends_with(b"%%EOF\n")
}

/// The first content stream's dictionary line, as the file has it.
fn first_stream_dict(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines()
        .find(|line| line.starts_with("<< /Length"))
        .map(str::to_string)
        .unwrap_or_default()
}

fn policy_job(dir: &Path, name: &str, body: &str) -> PathBuf {
    let input = dir.join(name);
    std::fs::write(&input, format!("%!PS\n{body}\n")).unwrap();
    input
}

// The locked-key scenario: the line wins over the job and says so.
#[test]
fn a_locked_key_overrides_the_job_and_the_line_reports_it() {
    let dir = scratch("locked-key");
    let input = policy_job(
        &dir,
        "locked.ps",
        "<< /CompressPages true >> setdistillerparams 10 10 100 100 rectfill showpage",
    );
    let out = dir.join("locked.pdf");
    let output = efterscript()
        .args(["pdf", "--no-compress", "--lock", "CompressPages"])
        .arg(&input)
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let bytes = std::fs::read(&out).unwrap();
    assert!(looks_like_a_pdf(&bytes));
    assert!(!first_stream_dict(&bytes).contains("/Filter"), "{bytes:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "efterscript: 1 parameter(s) not honoured: CompressPages=true (locked at false)\n"
    );

    // Without the lock the job's request wins and nothing is reported.
    let output = efterscript()
        .args(["pdf", "--no-compress"])
        .arg(&input)
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert!(first_stream_dict(&std::fs::read(&out).unwrap()).contains("/FlateDecode"));
}

#[test]
fn parameters_on_the_line_reach_the_file_and_the_job() {
    let dir = scratch("line-params");
    let input = policy_job(
        &dir,
        "level.ps",
        "currentdistillerparams /CompatibilityLevel get = \
         currentdistillerparams /Custom get = \
         10 10 100 100 rectfill showpage",
    );
    let out = dir.join("level.pdf");
    let output = efterscript()
        .args([
            "pdf",
            "--param",
            "CompatibilityLevel=1.4",
            "--param",
            "Custom=/Thing",
            "--param",
            "AutoRotatePages=All",
        ])
        .arg(&input)
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"1.4\nThing\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "efterscript: 2 parameter(s) not honoured: Custom=/Thing, AutoRotatePages=/All\n"
    );
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF-1.4\n"));
    assert!(first_stream_dict(&bytes).contains("/FlateDecode"));

    // Standard output cannot be rewound: the header stays 1.7 and the
    // line says so.
    let output = efterscript()
        .args([
            "pdf",
            "--param",
            "CompatibilityLevel=1.4",
            "--param",
            "Custom=1",
        ])
        .arg(&input)
        .arg("-")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.starts_with(b"%PDF-1.7\n"));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("CompatibilityLevel=1.4 (the output cannot be rewound; 1.7 written)")
    );
}

#[test]
fn a_bad_option_shows_the_usage() {
    let output = efterscript()
        .args(["pdf", "--param", "NoValue", "x.ps"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
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

// The embed-all scenario from the command line.
#[cfg(feature = "resident-outlines")]
#[test]
fn embed_all_on_the_line_embeds_the_standard_faces() {
    let dir = scratch("embed-all");
    let out = dir.join("hi.pdf");
    let output = efterscript()
        .args(["pdf", "--embed-all"])
        .arg(text_corpus("text-operation-shape.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(text.contains("/Subtype /TrueType /BaseFont /"), "{text}");
    assert!(text.contains("+LiberationSans-Regular"));
    assert!(text.contains("/FontFile2 "));
    assert!(!text.contains("/BaseFont /Helvetica"));

    // The whole program, untagged, without subsetting.
    let output = efterscript()
        .args(["pdf", "--embed-all", "--no-subset"])
        .arg(text_corpus("text-operation-shape.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(
        text.contains("/BaseFont /LiberationSans-Regular "),
        "{text}"
    );

    // Without the flag the face stays the unembedded standard font.
    let output = efterscript()
        .arg("pdf")
        .arg(text_corpus("text-operation-shape.ps"))
        .arg(&out)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&std::fs::read(&out).unwrap()).into_owned();
    assert!(text.contains("/BaseFont /Helvetica"));
    assert!(!text.contains("/FontFile2"));
}

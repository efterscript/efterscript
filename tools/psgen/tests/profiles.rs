// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! The generator's contract over whole profiles: reproducible output,
//! well-typed programs that run clean, scanner-clean text, and the
//! command line's surface.

use std::path::{Path, PathBuf};
use std::process::Command;

use psgen::grammar::{self, HANDLER};
use psgen::profile::{CORE, GRAPHICS};
use psgen::program::Program;
use psgen::properties::{self, Checker};
use psgen::runner;
use psgen::shrink;

/// Programs per scenario; `PSGEN_PROGRAMS` raises it for a longer sweep.
fn programs() -> u64 {
    std::env::var("PSGEN_PROGRAMS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn psgen() -> Command {
    Command::new(env!("CARGO_BIN_EXE_psgen"))
}

/// Whether a `stopped` handler reported an error (a procedure printed
/// by `pstack` carries the handler's text too, but not at a line start).
fn caught_anything(output: &str) -> bool {
    output.lines().any(|l| l.starts_with("psgen: caught "))
}

/// Every token of `text` scans; the scanner's own error otherwise.
fn scans(text: &str) -> Result<usize, String> {
    let mut memory = efterscript_vm::Memory::new();
    efterscript_vm::scan_all(text.as_bytes(), &mut memory, &mut ())
        .map(|tokens| tokens.len())
        .map_err(|e| e.to_string())
}

#[test]
fn regeneration_is_byte_identical() {
    for profile in [&CORE, &GRAPHICS] {
        for index in 0..20 {
            let a = grammar::generate(profile, 11, index).render();
            let b = grammar::generate(profile, 11, index).render();
            assert_eq!(a, b);
        }
    }
    let first = scratch("gen-a");
    let second = scratch("gen-b");
    for dir in [&first, &second] {
        let status = psgen()
            .args([
                "gen",
                "--profile",
                "core",
                "--seed",
                "3",
                "--count",
                "5",
                "--out",
            ])
            .arg(dir)
            .status()
            .unwrap();
        assert!(status.success());
    }
    for index in 0..5 {
        let name = format!("core-3-{index:04}.ps");
        let a = std::fs::read(first.join(&name)).unwrap();
        let b = std::fs::read(second.join(&name)).unwrap();
        assert_eq!(a, b, "{name}");
        let text = String::from_utf8(a).unwrap();
        assert!(text.starts_with(
            "%!PS\n% SPDX-FileCopyrightText: 2026 EfterScript contributors\n% SPDX-License-Identifier: MIT\n% psgen: profile=core seed=3 index="
        ));
    }
}

#[test]
fn well_typed_core_programs_end_without_error() {
    let profile = CORE.with_ill_typed(0);
    let mut statements = 0;
    for index in 0..programs() {
        let program = grammar::generate(&profile, 1, index);
        let text = program.render();
        statements += program.statements.len();
        assert!(scans(&text).is_ok(), "{text}");
        let run = runner::execute(&text, profile.budget);
        assert_eq!(run.panic, None, "index {index}: {text}");
        assert_eq!(
            run.error,
            None,
            "index {index} ended in {}: {text}\n{}",
            run.outcome(),
            run.stderr
        );
        assert!(
            !caught_anything(&run.output),
            "index {index} caught an error: {text}\n{}",
            run.output
        );
        assert!(!run.budget_exceeded);
    }
    assert!(statements >= programs() as usize * CORE.bounds.statements.0);
}

#[test]
fn well_typed_graphics_programs_produce_one_page() {
    if !efterscript_fonts::has_resident_outlines() {
        eprintln!("skipped: the resident outlines are absent from this build");
        return;
    }
    let profile = GRAPHICS.with_ill_typed(0);
    for index in 0..programs() {
        let program = grammar::generate(&profile, 2, index);
        let text = program.render();
        assert!(scans(&text).is_ok(), "{text}");
        let run = runner::execute(&text, profile.budget);
        assert_eq!(run.panic, None, "index {index}: {text}");
        assert_eq!(
            run.error,
            None,
            "index {index} ended in {}: {text}\n{}",
            run.outcome(),
            run.stderr
        );
        assert!(
            !caught_anything(&run.output),
            "index {index} caught an error: {text}\n{}",
            run.output
        );
        assert_eq!(run.pages, 1, "index {index}: {text}");
    }
}

#[test]
fn ill_typed_programs_report_and_finish() {
    let profile = CORE.with_ill_typed(300);
    let mut caught = 0;
    for index in 0..30 {
        let program = grammar::generate(&profile, 4, index);
        let text = program.render();
        assert!(scans(&text).is_ok(), "{text}");
        let run = runner::execute(&text, profile.budget);
        assert_eq!(run.error, None, "index {index}: {text}\n{}", run.stderr);
        caught += run
            .output
            .lines()
            .filter(|l| l.starts_with("psgen: caught "))
            .count();
        assert!(program.statements.iter().any(|s| s.contains(HANDLER)) || caught == 0);
    }
    assert!(caught > 0);
}

#[test]
fn every_generated_file_scans() {
    for profile in [&CORE, &GRAPHICS] {
        for index in 0..programs() {
            let text = grammar::generate(profile, 7, index).render();
            let tokens = scans(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
            assert!(tokens > 4);
        }
    }
}

#[test]
fn check_reports_failures_with_seed_and_index() {
    let dir = scratch("check");
    std::fs::create_dir_all(&dir).unwrap();
    let looping = dir.join("looping.ps");
    std::fs::write(
        &looping,
        "%!PS\n% psgen: profile=core seed=9 index=4\n1 =\n{ } loop\npstack\n",
    )
    .unwrap();
    let clean = dir.join("clean.ps");
    std::fs::write(&clean, "%!PS\n1 2 add =\npstack\n").unwrap();
    let output = psgen()
        .arg("check")
        .arg(&looping)
        .arg(&clean)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let expected = format!("fail budget {} seed=9 index=4: ", looping.display());
    assert!(stdout.contains(&expected), "{stdout}");
    assert!(
        stdout.contains("psgen: 2 programs checked, 1 failed"),
        "{stdout}"
    );

    let output = psgen()
        .args([
            "check",
            "--profile",
            "core",
            "--seed",
            "1",
            "--count",
            "3",
            "--ill-typed",
            "0",
        ])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(
        stdout.contains("psgen: 3 programs checked, 0 failed"),
        "{stdout}"
    );
}

#[test]
fn a_planted_failure_shrinks_to_itself() {
    let mut program = grammar::generate(&CORE.with_ill_typed(0), 5, 1);
    let tail = program.statements.pop();
    while program.statements.len() < 50 {
        program.statements.push("1 pop".to_string());
    }
    program.statements.insert(31, "{ } loop".to_string());
    program.statements.extend(tail);
    let text = program.render();
    let failing = Checker::new(&program, CORE.budget);
    assert!(failing.property("budget").unwrap().is_some());
    let minimal = shrink::shrink(&program, &mut |candidate| {
        Checker::new(candidate, CORE.budget)
            .property("budget")
            .is_ok_and(|f| f.is_some())
    });
    assert_eq!(minimal.statements, ["{ } loop"]);

    let dir = scratch("shrink");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("planted.ps");
    std::fs::write(&path, &text).unwrap();
    let output = psgen()
        .args(["shrink"])
        .arg(&path)
        .args(["--property", "budget"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let written = std::fs::read_to_string(dir.join("planted.min.ps")).unwrap();
    let minimal = Program::parse(&written);
    assert_eq!(minimal.statements, ["{ } loop"]);
    assert!(
        minimal
            .header
            .iter()
            .any(|l| l == "% psgen: shrunk from profile=core seed=5 index=1 property=budget"),
        "{written}"
    );
    assert!(
        properties::check(&minimal, CORE.budget)
            .iter()
            .any(|f| f.property == "budget")
    );

    // The external predicate: a command that fails while the planted
    // statement is present.
    let output = psgen()
        .args(["shrink"])
        .arg(&path)
        .args(["--predicate", "! grep -q loop", "--out"])
        .arg(dir.join("external.ps"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let external = Program::parse(&std::fs::read_to_string(dir.join("external.ps")).unwrap());
    assert_eq!(external.statements, ["{ } loop"]);
    assert!(!dir.join("external.candidate.ps").exists());

    // A program that does not fail is refused.
    let output = psgen()
        .args(["shrink"])
        .arg(&path)
        .args(["--property", "no-panic"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn usage_errors_exit_with_two() {
    assert_eq!(psgen().status().unwrap().code(), Some(2));
    assert_eq!(
        psgen()
            .args([
                "gen",
                "--profile",
                "nope",
                "--seed",
                "1",
                "--count",
                "1",
                "--out",
                "x"
            ])
            .status()
            .unwrap()
            .code(),
        Some(2)
    );
    assert_eq!(
        psgen().args(["gen", "--seed"]).status().unwrap().code(),
        Some(2)
    );
    assert_eq!(
        psgen()
            .args([
                "check",
                "--profile",
                "core",
                "--seed",
                "1",
                "--count",
                "1",
                "--ill-typed",
                "2"
            ])
            .status()
            .unwrap()
            .code(),
        Some(2)
    );
}

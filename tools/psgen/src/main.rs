// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `psgen gen` writes programs, `psgen check` runs them against the
//! properties, `psgen shrink` reduces a failing one.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use psgen::grammar;
use psgen::profile::Profile;
use psgen::program::{Origin, Program};
use psgen::properties::{self, Checker, Failure};
use psgen::runner;
use psgen::shrink;

fn usage() -> ExitCode {
    eprintln!(
        "usage: psgen gen --profile <p> --seed <n> --count <k> --out <dir> [--ill-typed <share>]"
    );
    eprintln!(
        "       psgen check --profile <p> --seed <n> --count <k> [--out <dir>] [--ill-typed <share>]"
    );
    eprintln!("       psgen check <file>…");
    eprintln!(
        "       psgen shrink <file> (--property <name> | --predicate <command>) [--out <file>]"
    );
    eprintln!("properties: {}", properties::PROPERTIES.join(", "));
    ExitCode::from(2)
}

/// Whether a candidate program still fails.
type Predicate = Box<dyn FnMut(&Program) -> bool>;

/// The options the subcommands share.
struct Args {
    profile: Option<String>,
    seed: Option<u64>,
    count: Option<u64>,
    out: Option<PathBuf>,
    ill_typed: Option<u32>,
    property: Option<String>,
    predicate: Option<String>,
    files: Vec<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        profile: None,
        seed: None,
        count: None,
        out: None,
        ill_typed: None,
        property: None,
        predicate: None,
        files: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |flag: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "--profile" => parsed.profile = Some(value("--profile")?),
            "--seed" => {
                let raw = value("--seed")?;
                parsed.seed = Some(
                    raw.parse()
                        .map_err(|_| format!("--seed {raw}: not a number"))?,
                );
            }
            "--count" => {
                let raw = value("--count")?;
                parsed.count = Some(
                    raw.parse()
                        .map_err(|_| format!("--count {raw}: not a number"))?,
                );
            }
            "--out" => parsed.out = Some(PathBuf::from(value("--out")?)),
            "--ill-typed" => {
                let raw = value("--ill-typed")?;
                let share: f64 = raw
                    .parse()
                    .ok()
                    .filter(|s| (0.0..=1.0).contains(s))
                    .ok_or_else(|| {
                        format!("--ill-typed {raw}: a share between 0 and 1 is needed")
                    })?;
                parsed.ill_typed = Some((share * 1000.0).round() as u32);
            }
            "--property" => parsed.property = Some(value("--property")?),
            "--predicate" => parsed.predicate = Some(value("--predicate")?),
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            path => parsed.files.push(PathBuf::from(path)),
        }
    }
    Ok(parsed)
}

fn profile_of(args: &Args) -> Result<Profile, String> {
    let name = args.profile.as_deref().ok_or("--profile is needed")?;
    let profile = Profile::named(name).ok_or_else(|| format!("unknown profile `{name}`"))?;
    Ok(match args.ill_typed {
        Some(share) => profile.with_ill_typed(share),
        None => profile.clone(),
    })
}

fn file_name(origin: &Origin) -> String {
    format!("{}-{}-{:04}.ps", origin.profile, origin.seed, origin.index)
}

fn write_program(dir: &Path, program: &Program) -> Result<PathBuf, String> {
    let origin = program
        .origin()
        .ok_or("a generated program has an origin")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let path = dir.join(file_name(&origin));
    std::fs::write(&path, program.render())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(path)
}

fn generate_command(args: &Args) -> Result<(), String> {
    let profile = profile_of(args)?;
    let seed = args.seed.ok_or("--seed is needed")?;
    let count = args.count.ok_or("--count is needed")?;
    let out = args.out.as_deref().ok_or("--out is needed")?;
    for index in 0..count {
        let program = grammar::generate(&profile, seed, index);
        write_program(out, &program)?;
    }
    println!(
        "psgen: wrote {count} {} programs for seed {seed} to {}",
        profile.name,
        out.display()
    );
    Ok(())
}

/// Where a program came from, for the report.
fn label(program: &Program, path: Option<&Path>) -> String {
    match (program.origin(), path) {
        (Some(origin), Some(path)) => format!(
            "{} seed={} index={}",
            path.display(),
            origin.seed,
            origin.index
        ),
        (Some(origin), None) => format!("seed={} index={}", origin.seed, origin.index),
        (None, Some(path)) => format!("{} seed=- index=-", path.display()),
        (None, None) => "seed=- index=-".to_string(),
    }
}

fn report(failures: &[Failure], label: &str) {
    for f in failures {
        println!("fail {} {label}: {}", f.property, f.detail);
    }
}

fn check(args: &Args) -> Result<bool, String> {
    let mut programs: Vec<(Program, Option<PathBuf>, u64)> = Vec::new();
    if args.files.is_empty() {
        let profile = profile_of(args)?;
        let seed = args.seed.ok_or("--seed is needed")?;
        let count = args.count.ok_or("--count is needed")?;
        for index in 0..count {
            let program = grammar::generate(&profile, seed, index);
            let path = match &args.out {
                Some(dir) => Some(write_program(dir, &program)?),
                None => None,
            };
            programs.push((program, path, profile.budget));
        }
    } else {
        for path in &args.files {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let program = Program::parse(&text);
            let budget = program
                .origin()
                .and_then(|o| Profile::named(&o.profile))
                .map_or(psgen::profile::CORE.budget, |p| p.budget);
            programs.push((program, Some(path.clone()), budget));
        }
    }
    let mut failed = 0;
    for (program, path, budget) in &programs {
        let failures = properties::check(program, *budget);
        if !failures.is_empty() {
            failed += 1;
            report(&failures, &label(program, path.as_deref()));
        }
    }
    println!(
        "psgen: {} programs checked, {failed} failed",
        programs.len()
    );
    Ok(failed == 0)
}

fn shrink_command(args: &Args) -> Result<(), String> {
    let [path] = args.files.as_slice() else {
        return Err("shrink takes exactly one file".to_string());
    };
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let program = Program::parse(&text);
    let budget = program
        .origin()
        .and_then(|o| Profile::named(&o.profile))
        .map_or(psgen::profile::CORE.budget, |p| p.budget);
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| path.with_extension("min.ps"));
    let scratch = out.with_extension("candidate.ps");
    let (what, mut fails): (String, Predicate) = match (&args.property, &args.predicate) {
        (Some(name), None) => {
            if !properties::PROPERTIES.contains(&name.as_str()) {
                return Err(format!("unknown property `{name}`"));
            }
            let name = name.clone();
            let property = name.clone();
            (
                format!("property={name}"),
                Box::new(move |candidate: &Program| {
                    Checker::new(candidate, budget)
                        .property(&property)
                        .is_ok_and(|f| f.is_some())
                }),
            )
        }
        (None, Some(command)) => {
            let command = command.clone();
            let scratch = scratch.clone();
            (
                format!("predicate={command}"),
                Box::new(move |candidate: &Program| {
                    if std::fs::write(&scratch, candidate.render()).is_err() {
                        return false;
                    }
                    Command::new("sh")
                        .arg("-c")
                        .arg(format!("{command} {}", scratch.display()))
                        .status()
                        .is_ok_and(|status| !status.success())
                }),
            )
        }
        _ => return Err("exactly one of --property and --predicate is needed".to_string()),
    };
    if !fails(&program) {
        return Err(format!("{} does not fail the {what}", path.display()));
    }
    let mut minimal = shrink::shrink(&program, &mut *fails);
    let origin = program
        .origin()
        .map_or_else(|| path.display().to_string(), |o| o.to_string());
    minimal
        .header
        .push(format!("% psgen: shrunk from {origin} {what}"));
    let _ = std::fs::remove_file(&scratch);
    std::fs::write(&out, minimal.render())
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    println!(
        "psgen: {} statements -> {}, written to {}",
        program.statements.len(),
        minimal.statements.len(),
        out.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first() else {
        return usage();
    };
    let parsed = match parse(&args[1..]) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("psgen: {e}");
            return usage();
        }
    };
    let result = match command.as_str() {
        "gen" => generate_command(&parsed).map(|()| true),
        "check" => {
            runner::silence_panics();
            check(&parsed)
        }
        "shrink" => {
            runner::silence_panics();
            shrink_command(&parsed).map(|()| true)
        }
        _ => return usage(),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("psgen: {e}");
            ExitCode::from(2)
        }
    }
}

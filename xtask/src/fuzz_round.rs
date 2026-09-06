// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask fuzz-round [--profile <p>] [--oracle <name>]`: reads
//! `corpus/generated/<profile>.seeds` (lines of `seed count`, `#`
//! comments), generates and checks every pair through `psgen check`
//! into `target/psgen/<profile>/<seed>/`, and with `--oracle` runs
//! `difftest oracle --profile <name>` over the generated directories
//! (the private tier; the environment passes through). Exit is non-zero
//! on any failure; the summary counts programs and failures per profile.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// The `seed count` pairs of a seed file.
pub fn parse_seeds(text: &str) -> Result<Vec<(u64, u64)>, String> {
    let mut pairs = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split_whitespace();
        let seed = fields
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("line {}: expected `seed count`", index + 1))?;
        let count = fields
            .next()
            .and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("line {}: expected `seed count`", index + 1))?;
        if fields.next().is_some() {
            return Err(format!("line {}: expected `seed count`", index + 1));
        }
        pairs.push((seed, count));
    }
    Ok(pairs)
}

/// The counts in a `psgen check` summary line.
pub fn parse_summary(stdout: &str) -> Option<(u64, u64)> {
    let line = stdout
        .lines()
        .rev()
        .find(|l| l.starts_with("psgen: ") && l.contains("programs checked"))?;
    let rest = line.strip_prefix("psgen: ")?;
    let (programs, rest) = rest.split_once(" programs checked, ")?;
    let (failed, _) = rest.split_once(" failed")?;
    Some((programs.parse().ok()?, failed.parse().ok()?))
}

struct Options {
    profiles: Vec<String>,
    oracle: Option<String>,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        profiles: Vec::new(),
        oracle: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--profile" => options
                .profiles
                .push(it.next().cloned().ok_or("--profile needs a value")?),
            "--oracle" => {
                options.oracle = Some(it.next().cloned().ok_or("--oracle needs a value")?)
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if options.profiles.is_empty() {
        options.profiles = vec!["core".to_string(), "graphics".to_string()];
    }
    Ok(options)
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

struct Tally {
    profile: String,
    programs: u64,
    failures: u64,
    /// Directories generated, for the oracle.
    dirs: Vec<PathBuf>,
}

fn run_profile(root: &Path, profile: &str) -> Result<Tally, String> {
    let seeds_path = root
        .join("corpus")
        .join("generated")
        .join(format!("{profile}.seeds"));
    let text = std::fs::read_to_string(&seeds_path)
        .map_err(|e| format!("cannot read {}: {e}", seeds_path.display()))?;
    let pairs = parse_seeds(&text).map_err(|e| format!("{}: {e}", seeds_path.display()))?;
    let mut tally = Tally {
        profile: profile.to_string(),
        programs: 0,
        failures: 0,
        dirs: Vec::new(),
    };
    for (seed, count) in pairs {
        let dir = root
            .join("target")
            .join("psgen")
            .join(profile)
            .join(seed.to_string());
        let output = cargo()
            .current_dir(root)
            .args(["run", "-q", "-p", "psgen", "--", "check"])
            .args(["--profile", profile])
            .args(["--seed", &seed.to_string()])
            .args(["--count", &count.to_string()])
            .arg("--out")
            .arg(&dir)
            .output()
            .map_err(|e| format!("cannot run psgen: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        print!("{stdout}");
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
        match parse_summary(&stdout) {
            Some((programs, failures)) => {
                tally.programs += programs;
                tally.failures += failures;
            }
            None => {
                return Err(format!(
                    "psgen check for {profile} seed {seed} ended without a summary ({})",
                    output.status
                ));
            }
        }
        tally.dirs.push(dir);
    }
    Ok(tally)
}

fn run_oracle(root: &Path, name: &str, tally: &Tally) -> Result<bool, String> {
    println!(
        "oracle: {} over {} directories",
        tally.profile,
        tally.dirs.len()
    );
    let status = cargo()
        .current_dir(root)
        .args([
            "run",
            "-q",
            "-p",
            "difftest",
            "--",
            "oracle",
            "--profile",
            name,
        ])
        .args(&tally.dirs)
        .status()
        .map_err(|e| format!("cannot run difftest: {e}"))?;
    Ok(status.success())
}

pub fn run(args: &[String]) -> ExitCode {
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("fuzz-round: {e}");
            return ExitCode::from(2);
        }
    };
    let root = workspace_root();
    let mut failed = false;
    let mut tallies = Vec::new();
    for profile in &options.profiles {
        match run_profile(&root, profile) {
            Ok(tally) => tallies.push(tally),
            Err(e) => {
                eprintln!("fuzz-round: {e}");
                return ExitCode::from(2);
            }
        }
    }
    if let Some(name) = &options.oracle {
        for tally in &tallies {
            match run_oracle(&root, name, tally) {
                Ok(true) => {}
                Ok(false) => failed = true,
                Err(e) => {
                    eprintln!("fuzz-round: {e}");
                    return ExitCode::from(2);
                }
            }
        }
    }
    println!();
    for tally in &tallies {
        println!(
            "{}: {} programs, {} failed",
            tally.profile, tally.programs, tally.failures
        );
        if tally.failures > 0 {
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_files_list_pairs() {
        let text = "# comment\n1 100\n\n  2 50 # trailing\n";
        assert_eq!(parse_seeds(text).unwrap(), [(1, 100), (2, 50)]);
        assert!(parse_seeds("1").is_err());
        assert!(parse_seeds("1 2 3").is_err());
        assert!(parse_seeds("x 2").is_err());
        assert_eq!(parse_seeds("").unwrap(), []);
    }

    #[test]
    fn summaries_are_read() {
        assert_eq!(
            parse_summary("fail x seed=1 index=2: y\npsgen: 100 programs checked, 1 failed\n"),
            Some((100, 1))
        );
        assert_eq!(parse_summary("nothing"), None);
    }

    #[test]
    fn arguments_are_parsed() {
        let options = parse_args(&[]).unwrap();
        assert_eq!(options.profiles, ["core", "graphics"]);
        assert_eq!(options.oracle, None);
        let args: Vec<String> = ["--profile", "core", "--oracle", "default"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let options = parse_args(&args).unwrap();
        assert_eq!(options.profiles, ["core"]);
        assert_eq!(options.oracle.as_deref(), Some("default"));
        assert!(parse_args(&["--nope".to_string()]).is_err());
        assert!(parse_args(&["--oracle".to_string()]).is_err());
    }

    #[test]
    fn the_committed_seed_files_parse() {
        for profile in ["core", "graphics"] {
            let path = workspace_root()
                .join("corpus")
                .join("generated")
                .join(format!("{profile}.seeds"));
            let text = std::fs::read_to_string(&path).unwrap();
            let pairs = parse_seeds(&text).unwrap();
            assert_eq!(pairs.len(), 11, "{}", path.display());
            assert!(pairs.iter().all(|&(_, count)| count > 0));
        }
    }
}

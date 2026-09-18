// SPDX-FileCopyrightText: 2026 EfterScript contributors
// SPDX-License-Identifier: MIT

//! `cargo xtask fuzz-smoke [--seconds <n>]`: the libFuzzer targets'
//! bounded round. Always, on the stable toolchain in use, it `cargo
//! check`s every fuzz crate (they are outside the workspace, so nothing
//! else builds them and a target could otherwise rot) and seeds each
//! target's `corpus/<target>/` directory when it is empty: a few files
//! from `corpus/unit`, and streams the project's own encoders and test
//! font builders produce. Then, when a nightly toolchain (`rustup run
//! nightly cargo --version`) and `cargo fuzz` (`cargo fuzz --version`)
//! are both found, it runs every target for `--seconds` (default 30)
//! under `-max_total_time`; otherwise it prints one skip line naming
//! what is missing and exits successfully. A stable check failure or a
//! crash fails the run.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use efterscript_codec::{deflate, lzw};
use efterscript_fonts::testing::{
    corpus_cff, corpus_cid_cff, corpus_truetype, corpus_type1, eexec_binary,
};

use crate::tiny_jpeg;

/// A fuzz crate, relative to the workspace root, and its targets.
struct FuzzCrate {
    dir: &'static str,
    targets: &'static [&'static str],
}

const CRATES: &[FuzzCrate] = &[
    FuzzCrate {
        dir: "crates/efterscript-codec/fuzz",
        targets: &["inflate", "lzw", "predictor"],
    },
    FuzzCrate {
        dir: "crates/efterscript-vm/fuzz",
        targets: &["scan", "filters", "jpeg", "program"],
    },
    FuzzCrate {
        dir: "crates/efterscript-fonts/fuzz",
        targets: &["type1", "cff", "truetype"],
    },
    FuzzCrate {
        dir: "crates/efterscript-remelt/fuzz",
        targets: &["distill"],
    },
];

const DEFAULT_SECONDS: u64 = 30;

/// Corpus files taken per directory when seeding.
const FILES_PER_DIR: usize = 6;

struct Options {
    seconds: u64,
}

fn parse_args(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        seconds: DEFAULT_SECONDS,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--seconds" => {
                let value = it.next().ok_or("--seconds needs a value")?;
                options.seconds =
                    value.parse().ok().filter(|&s| s > 0).ok_or_else(|| {
                        format!("--seconds needs a positive count, got `{value}`")
                    })?;
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(options)
}

// --- toolchain detection -----------------------------------------------------

/// What `rustup run nightly cargo --version` printed, when it named a
/// nightly cargo.
pub fn nightly_version(stdout: &str) -> Option<String> {
    let line = stdout.lines().next()?.trim();
    (line.starts_with("cargo ") && line.contains("nightly")).then(|| line.to_string())
}

/// What `cargo fuzz --version` printed, when it named cargo-fuzz.
pub fn cargo_fuzz_version(stdout: &str) -> Option<String> {
    let line = stdout.lines().next()?.trim();
    line.starts_with("cargo-fuzz ").then(|| line.to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toolchain {
    pub nightly: Option<String>,
    pub cargo_fuzz: Option<String>,
}

/// The skip line when the fuzz run cannot happen; `None` when it can.
pub fn skip_message(toolchain: &Toolchain) -> Option<String> {
    let mut missing = Vec::new();
    if toolchain.nightly.is_none() {
        missing.push("no nightly toolchain");
    }
    if toolchain.cargo_fuzz.is_none() {
        missing.push("no cargo-fuzz");
    }
    (!missing.is_empty())
        .then(|| format!("fuzz-smoke: skipped the fuzz run: {}", missing.join(", ")))
}

/// A command's standard output when it ran and succeeded.
fn probe(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn detect() -> Toolchain {
    Toolchain {
        nightly: probe("rustup", &["run", "nightly", "cargo", "--version"])
            .and_then(|s| nightly_version(&s)),
        cargo_fuzz: probe("cargo", &["fuzz", "--version"]).and_then(|s| cargo_fuzz_version(&s)),
    }
}

// --- seeds -------------------------------------------------------------------

/// The first `.ps` files of a `corpus/unit` directory, by name.
fn corpus_files(root: &Path, subdir: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let dir = root.join("corpus").join("unit").join(subdir);
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ps"))
        .collect();
    paths.sort();
    paths
        .iter()
        .take(FILES_PER_DIR)
        .map(|path| {
            let name = format!(
                "{subdir}-{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            );
            let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            Ok((name, bytes))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> Vec<u8> {
    let mut out: Vec<u8> = bytes
        .iter()
        .flat_map(|b| format!("{b:02x}").into_bytes())
        .collect();
    out.push(b'>');
    out
}

/// Base-85 with the `~>` end marker, no `z` groups.
fn ascii85(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        let mut value = u32::from_be_bytes(word);
        let mut digits = [0u8; 5];
        for digit in digits.iter_mut().rev() {
            *digit = (value % 85) as u8 + b'!';
            value /= 85;
        }
        out.extend_from_slice(&digits[..chunk.len() + 1]);
    }
    out.extend_from_slice(b"~>");
    out
}

fn seed(name: &str, parts: &[&[u8]]) -> (String, Vec<u8>) {
    (name.to_string(), parts.concat())
}

/// The seeds of a target: a name and bytes each. The synthesized ones
/// follow the leading-byte layouts the `filters` and `predictor`
/// targets document.
fn seeds(root: &Path, target: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    const TEXT: &[u8] =
        b"Hello, world. Hello, world. Hello, world. 0 0 moveto 100 100 lineto stroke\n";
    // Eight rows of a tag byte and four columns for the PNG predictors;
    // the TIFF predictor takes the same bytes as four-column rows.
    let png_rows: Vec<u8> = (0..8u8).flat_map(|row| [row % 5, 1, 2, 3, 4]).collect();
    let compressed = deflate::compress(TEXT);
    Ok(match target {
        "scan" => [
            corpus_files(root, "interp")?,
            corpus_files(root, "filters")?,
        ]
        .concat(),
        "program" => [
            corpus_files(root, "interp")?,
            corpus_files(root, "graphics")?,
        ]
        .concat(),
        "distill" => [
            corpus_files(root, "graphics")?,
            corpus_files(root, "pdfmark")?,
            corpus_files(root, "patterns")?,
        ]
        .concat(),
        "filters" => {
            let mut seeds = corpus_files(root, "filters")?;
            seeds.extend([
                seed("hex", &[&[0, 0], &hex(TEXT)]),
                seed("a85", &[&[0, 1], &ascii85(TEXT)]),
                seed("runlength", &[&[0, 2, 4], b"Hello", &[0x80]]),
                seed("flate", &[&[0, 3, 0, 0, 3, 0], &compressed]),
                seed(
                    "flate-png",
                    &[&[0, 3, 2, 0, 3, 3], &deflate::compress(&png_rows)],
                ),
                seed(
                    "lzw-early",
                    &[&[0, 4, 1, 0, 0, 3, 0], &lzw::encode(TEXT, true)],
                ),
                seed(
                    "lzw-late",
                    &[&[0, 4, 0, 0, 0, 3, 0], &lzw::encode(TEXT, false)],
                ),
                seed("subfile", &[&[0, 5, 0, 3], b"EOD", b"kept EOD dropped"]),
                seed("eexec", &[&[0, 6], &eexec_binary(b"/Private 8 dict def")]),
                seed(
                    "chain-hex-flate",
                    &[&[1, 0, 3, 0, 0, 3, 0], &hex(&compressed)],
                ),
            ]);
            seeds
        }
        "jpeg" => {
            let jpeg = tiny_jpeg::bytes();
            vec![
                seed("tiny", &[&jpeg]),
                seed("tiny-truncated", &[&jpeg[..jpeg.len() / 2]]),
            ]
        }
        "inflate" => vec![
            seed("empty", &[&deflate::compress(b"")]),
            seed("text", &[&compressed]),
            seed("runs", &[&deflate::compress(&[0xAB; 3000])]),
        ],
        "lzw" => vec![
            seed("early", &[&lzw::encode(TEXT, true)]),
            seed("late", &[&lzw::encode(TEXT, false)]),
        ],
        "predictor" => vec![
            seed("tiff", &[&[1, 0, 3, 3], &png_rows]),
            seed("png", &[&[2, 0, 3, 3], &png_rows]),
            seed("png-16bit", &[&[7, 1, 4, 1], &png_rows]),
        ],
        "type1" => {
            let font = corpus_type1();
            vec![
                seed("corpus.pfb", &[&font.pfb()]),
                seed("corpus.pfa", &[font.pfa().as_bytes()]),
            ]
        }
        "cff" => vec![
            seed("corpus", &[&corpus_cff().build()]),
            seed("cid", &[&corpus_cid_cff().build()]),
        ],
        "truetype" => vec![seed("corpus", &[&corpus_truetype().build()])],
        other => return Err(format!("no seeds are defined for `{other}`")),
    })
}

/// Writes the target's seeds unless its corpus directory already has
/// files; answers how many were written.
fn seed_target(root: &Path, krate: &FuzzCrate, target: &str) -> Result<usize, String> {
    let dir = root.join(krate.dir).join("corpus").join(target);
    if let Ok(mut entries) = std::fs::read_dir(&dir)
        && entries.next().is_some()
    {
        return Ok(0);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let seeds = seeds(root, target)?;
    for (name, bytes) in &seeds {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(seeds.len())
}

// --- the run -----------------------------------------------------------------

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the workspace root")
        .to_path_buf()
}

fn cargo() -> Command {
    Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
}

/// `cargo check` of a fuzz crate on the toolchain in use, into one
/// shared target directory so the library crates compile once.
fn check_stable(root: &Path, krate: &FuzzCrate) -> Result<(), String> {
    let status = cargo()
        .current_dir(root)
        .env("CARGO_TARGET_DIR", root.join("target").join("fuzz-check"))
        .args(["check", "-q", "--manifest-path"])
        .arg(root.join(krate.dir).join("Cargo.toml"))
        .status()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("cargo check of {} failed ({status})", krate.dir))
    }
}

/// One target's bounded run: `cargo fuzz run` from the crate the fuzz
/// directory belongs to, under the nightly toolchain, through rustup
/// (the `CARGO` of a stable `cargo xtask` cannot switch toolchains).
fn run_target(root: &Path, krate: &FuzzCrate, target: &str, seconds: u64) -> Result<bool, String> {
    let crate_dir = root.join(krate.dir);
    let parent = crate_dir
        .parent()
        .ok_or_else(|| format!("{} has no parent", krate.dir))?;
    let status = Command::new("rustup")
        .current_dir(parent)
        .args(["run", "nightly", "cargo", "fuzz", "run", target, "--"])
        .arg(format!("-max_total_time={seconds}"))
        .status()
        .map_err(|e| format!("cannot run rustup: {e}"))?;
    Ok(status.success())
}

pub fn run(args: &[String]) -> ExitCode {
    let options = match parse_args(args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("fuzz-smoke: {e}");
            return ExitCode::from(2);
        }
    };
    let root = workspace_root();
    let mut targets = 0;
    for krate in CRATES {
        if let Err(e) = check_stable(&root, krate) {
            eprintln!("fuzz-smoke: {e}");
            return ExitCode::FAILURE;
        }
        println!("fuzz-smoke: {} checks on the stable toolchain", krate.dir);
        for target in krate.targets {
            match seed_target(&root, krate, target) {
                Ok(0) => {}
                Ok(n) => println!("fuzz-smoke: seeded {target} with {n} files"),
                Err(e) => {
                    eprintln!("fuzz-smoke: {e}");
                    return ExitCode::FAILURE;
                }
            }
            targets += 1;
        }
    }
    let toolchain = detect();
    if let Some(message) = skip_message(&toolchain) {
        println!("{message}");
        return ExitCode::SUCCESS;
    }
    let mut crashed = Vec::new();
    for krate in CRATES {
        for target in krate.targets {
            println!("fuzz-smoke: {target} for {} seconds", options.seconds);
            match run_target(&root, krate, target, options.seconds) {
                Ok(true) => {}
                Ok(false) => crashed.push(*target),
                Err(e) => {
                    eprintln!("fuzz-smoke: {e}");
                    return ExitCode::FAILURE;
                }
            }
        }
    }
    if crashed.is_empty() {
        println!(
            "fuzz-smoke: {targets} targets ran {} seconds each without a crash",
            options.seconds
        );
        ExitCode::SUCCESS
    } else {
        println!("fuzz-smoke: crashed: {}", crashed.join(", "));
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nightly_cargo_is_recognised_and_a_stable_one_is_not() {
        assert_eq!(
            nightly_version("cargo 1.99.0-nightly (0123abcd 2026-09-01)\n"),
            Some("cargo 1.99.0-nightly (0123abcd 2026-09-01)".to_string())
        );
        assert_eq!(
            nightly_version("cargo 1.98.0 (0123abcd 2026-08-01)\n"),
            None
        );
        assert_eq!(nightly_version(""), None);
        assert_eq!(
            nightly_version("error: toolchain 'nightly' is not installed"),
            None
        );
    }

    #[test]
    fn cargo_fuzz_is_recognised_by_its_version_line() {
        assert_eq!(
            cargo_fuzz_version("cargo-fuzz 0.12.0\n"),
            Some("cargo-fuzz 0.12.0".to_string())
        );
        assert_eq!(cargo_fuzz_version("error: no such command: `fuzz`"), None);
        assert_eq!(cargo_fuzz_version(""), None);
    }

    #[test]
    fn the_skip_line_names_what_is_missing() {
        let both = Toolchain {
            nightly: Some("cargo 1.99.0-nightly".to_string()),
            cargo_fuzz: Some("cargo-fuzz 0.12.0".to_string()),
        };
        assert_eq!(skip_message(&both), None);
        let neither = Toolchain {
            nightly: None,
            cargo_fuzz: None,
        };
        assert_eq!(
            skip_message(&neither).as_deref(),
            Some("fuzz-smoke: skipped the fuzz run: no nightly toolchain, no cargo-fuzz")
        );
        let fuzz_only = Toolchain {
            nightly: None,
            cargo_fuzz: both.cargo_fuzz.clone(),
        };
        assert_eq!(
            skip_message(&fuzz_only).as_deref(),
            Some("fuzz-smoke: skipped the fuzz run: no nightly toolchain")
        );
    }

    #[test]
    fn arguments_are_parsed() {
        assert_eq!(parse_args(&[]).unwrap().seconds, DEFAULT_SECONDS);
        let args: Vec<String> = ["--seconds", "5"].iter().map(|s| s.to_string()).collect();
        assert_eq!(parse_args(&args).unwrap().seconds, 5);
        assert!(parse_args(&["--seconds".to_string()]).is_err());
        assert!(parse_args(&["--seconds".to_string(), "0".to_string()]).is_err());
        assert!(parse_args(&["--nope".to_string()]).is_err());
    }

    #[test]
    fn every_target_has_seeds_and_a_binary_in_its_manifest() {
        let root = workspace_root();
        for krate in CRATES {
            let manifest = root.join(krate.dir).join("Cargo.toml");
            let text = std::fs::read_to_string(&manifest).unwrap();
            assert!(text.contains("cargo-fuzz = true"), "{}", manifest.display());
            for target in krate.targets {
                assert!(
                    text.contains(&format!("name = \"{target}\"")),
                    "{target} in {}",
                    manifest.display()
                );
                let seeds = seeds(&root, target).unwrap();
                assert!(!seeds.is_empty(), "{target}");
                assert!(seeds.iter().all(|(_, bytes)| !bytes.is_empty()), "{target}");
            }
        }
        assert!(seeds(&root, "nothing").is_err());
    }

    #[test]
    fn the_encoders_behind_the_seeds_round_trip() {
        assert_eq!(hex(b"\x01\xab"), b"01ab>");
        assert_eq!(ascii85(b""), b"~>");
        // Four zero bytes are five `!` digits without the `z` shortcut.
        assert_eq!(ascii85(&[0, 0, 0, 0]), b"!!!!!~>");
        assert_eq!(ascii85(b"a"), b"@/~>");
    }
}
